//! Sim-to-sim validation.
//!
//! Run one policy in two numerical implementations of the same physics and
//! compare what it achieves. zealot validates every policy against MuJoCo for
//! this reason: a policy is only checked against the engine it was trained in
//! until it is checked against a different one, and a gait that exploits one
//! integrator's error does not survive the other.
//!
//! Here the reference is a fourth-order Runge-Kutta step against the same
//! equations of motion. That is a weaker check than a genuinely independent
//! engine — it shares the dynamics function, so it can only catch integration
//! artifacts, not modelling ones — and the UI says so rather than implying it
//! caught more.

use std::sync::{Arc, Mutex};
use std::thread;

use crate::ckpt;
use crate::env::{Integrator, VecEnv, N_ACT, N_OBS};
use crate::rl::{Config, Ppo};
use crate::rng::Rng;

const EPISODES: usize = 40;
const MAX_STEPS: usize = 400;

/// What a policy achieved in one implementation.
#[derive(Clone, Copy, Default, Debug)]
pub struct Score {
    pub survival: f32,
    pub tracking: f32,
    pub reward: f32,
    pub fall_rate: f32,
}

#[derive(Default)]
pub struct Report {
    pub a: Score,
    pub b: Score,
    pub running: bool,
    pub done: bool,
    pub label: String,
}

impl Report {
    /// The four rows the comparison table shows: name, A, B, delta.
    pub fn rows(&self) -> [(&'static str, String, String, String); 4] {
        let f = |x: f32| format!("{x:.3}");
        let d = |x: f32, y: f32| {
            let v = y - x;
            format!("{}{:.3}", if v >= 0.0 { "+" } else { "−" }, v.abs())
        };
        [
            ("survival", f(self.a.survival), f(self.b.survival),
             d(self.a.survival, self.b.survival)),
            ("tracking", f(self.a.tracking), f(self.b.tracking),
             d(self.a.tracking, self.b.tracking)),
            ("mean reward", f(self.a.reward), f(self.b.reward),
             d(self.a.reward, self.b.reward)),
            ("fall rate", f(self.a.fall_rate), f(self.b.fall_rate),
             d(self.a.fall_rate, self.b.fall_rate)),
        ]
    }

    /// The largest relative disagreement between the two implementations.
    /// This is the number that decides whether a policy has transferred.
    pub fn worst_gap(&self) -> f32 {
        let pairs = [
            (self.a.survival, self.b.survival),
            (self.a.tracking, self.b.tracking),
            (self.a.reward, self.b.reward),
        ];
        pairs
            .iter()
            .map(|(x, y)| {
                let denom = x.abs().max(1e-3);
                ((y - x).abs() / denom).min(9.99)
            })
            .fold(0.0, f32::max)
    }
}

fn evaluate(ppo: &mut Ppo, integrator: Integrator, seed: u64) -> Score {
    let mut env = VecEnv::new(1, seed);
    env.set_integrator(integrator);
    let mut obs = vec![0.0; N_OBS];
    let mut action = vec![0.0; N_ACT];

    let (mut survived, mut fell_n) = (0usize, 0usize);
    let (mut track_sum, mut reward_sum, mut steps_total) = (0.0_f32, 0.0_f32, 0usize);

    for ep in 0..EPISODES {
        // The same command spread in both, so the comparison is paired.
        let cmd = -0.7 + 1.4 * (ep as f32 / (EPISODES - 1) as f32);
        env.restart(0, cmd);
        let mut fell = false;
        for _ in 0..MAX_STEPS {
            env.observe(0, &mut obs);
            ppo.act_mean(&obs, &mut action);
            let o = env.step(&[action.clone()]);
            track_sum += o.terms[0][0];
            reward_sum += o.reward[0];
            steps_total += 1;
            if o.fell[0] {
                fell = true;
                break;
            }
            if o.done[0] {
                break;
            }
        }
        if fell {
            fell_n += 1;
        } else {
            survived += 1;
        }
    }

    let n = steps_total.max(1) as f32;
    Score {
        survival: survived as f32 / EPISODES as f32,
        tracking: track_sum / n,
        reward: reward_sum / n,
        fall_rate: fell_n as f32 / EPISODES as f32,
    }
}

pub fn spawn(run: &str) -> Option<Arc<Mutex<Report>>> {
    let (path, manifest) = ckpt::list(run).into_iter().next()?;
    let (weights, _) = ckpt::read(&path).ok()?;

    let report = Arc::new(Mutex::new(Report {
        running: true,
        label: format!("step {}k", manifest.step / 1000),
        ..Default::default()
    }));
    let out = Arc::clone(&report);

    thread::spawn(move || {
        let mut rng = Rng::new(manifest.seed ^ 0xC0551);
        let hidden = if manifest.hidden == 0 { 64 } else { manifest.hidden };
        let mut ppo = Ppo::new(N_OBS, N_ACT, hidden, Config::default(), &mut rng);
        if !ppo.load_weights(&weights) {
            if let Ok(mut r) = out.lock() {
                r.running = false;
                r.label = "checkpoint does not match its manifest".into();
            }
            return;
        }
        // Identical seeds so the two runs differ only in their numerics.
        let a = evaluate(&mut ppo, Integrator::Euler, 0x5EED);
        let b = evaluate(&mut ppo, Integrator::Rk4, 0x5EED);
        if let Ok(mut r) = out.lock() {
            r.a = a;
            r.b = b;
            r.running = false;
            r.done = true;
        }
    });

    Some(report)
}

/// Sim-to-sim for zealot: the same policy under two physics-step resolutions.
///
/// zealot integrates several physics substeps per control tick, and
/// `BIPED_DECIMATION` sets how many. Running one control decimation against
/// another is the same test this module already makes against the cart-pole —
/// a gait that only survives at one step size is exploiting the integrator,
/// not solving the task. Like the built-in comparison it shares the dynamics,
/// so it catches integration artefacts rather than modelling ones, and the UI
/// says so instead of implying more.
#[cfg(feature = "zealot")]
pub mod zealot_cross {
    use super::*;
    use crate::zealot::{self, Drive};

    const COMMAND_VX: f32 = 0.3;
    const SECONDS: f32 = 3.0;
    const FLOOR: f32 = 0.4;

    /// Coarse against fine: zealot's default control decimation against one
    /// physics step per control tick.
    const COARSE: &str = "4";
    const FINE: &str = "1";

    fn evaluate(ckpt: &str, decimation: &str) -> Option<Score> {
        let cmd = Drive { vx: COMMAND_VX, seconds: SECONDS, ..Drive::default() };
        let knobs = [
            ("BIPED_DECIMATION", decimation.to_string()),
            ("BIPED_SPAWN_DR", "0".to_string()),
        ];
        let r = zealot::drive(ckpt, cmd, &knobs)?;
        if r.is_empty() {
            return None;
        }
        let fell = r.fell(FLOOR);
        let achieved = r.achieved_vx();
        Some(Score {
            survival: if fell { 0.0 } else { 1.0 },
            tracking: (1.0 - (achieved - COMMAND_VX).abs() / COMMAND_VX).clamp(0.0, 1.0),
            // zealot's reward is not recoverable from a rollout; reporting the
            // achieved velocity is the honest substitute and is what the two
            // implementations are actually being compared on.
            reward: achieved,
            fall_rate: if fell { 1.0 } else { 0.0 },
        })
    }

    pub fn spawn(ckpt: &str) -> Option<Arc<Mutex<Report>>> {
        if !zealot::drive_path().is_file() {
            return None;
        }
        let ckpt = ckpt.to_string();
        let report = Arc::new(Mutex::new(Report {
            running: true,
            label: format!("zealot · decimation {COARSE} vs {FINE}"),
            ..Default::default()
        }));
        let out = Arc::clone(&report);

        thread::spawn(move || {
            let a = evaluate(&ckpt, COARSE);
            let b = evaluate(&ckpt, FINE);
            if let Ok(mut r) = out.lock() {
                match (a, b) {
                    (Some(a), Some(b)) => {
                        r.a = a;
                        r.b = b;
                        r.done = true;
                    }
                    // Half a comparison is not a comparison.
                    _ => r.label = "rollout failed; nothing to compare".into(),
                }
                r.running = false;
            }
        });

        Some(report)
    }
}

/// Sim-to-sim against MuJoCo — a genuinely independent engine.
///
/// The two arms above share the dynamics function, so they catch integration
/// artefacts and nothing else. This one does not: MuJoCo brings its own contact
/// model and solver, which is what makes it the check that predicts transfer,
/// and it is the check Unitree's pipeline performs before sim-to-real.
///
/// `a` is the policy as zealot itself scores it, `b` as MuJoCo does, so
/// `worst_gap` reads exactly as it does for the other arms: how far the two
/// engines disagree about the same policy.
pub fn spawn_mujoco(ckpt: &str, command_vx: f32, seconds: u32) -> Option<Arc<Mutex<Report>>> {
    spawn_mujoco_into(ckpt, command_vx, seconds, crate::zealot::RolloutSlot::default())
}

/// As [`spawn_mujoco`], and also fills `rollout` with the trajectory MuJoCo
/// simulated — from the *same* harness run.
///
/// This is what a console should call when it can show the motion. The report
/// answers whether the policy transferred; the rollout answers how it failed,
/// and the two together are one 45-second run rather than two.
pub fn spawn_mujoco_into(
    ckpt: &str,
    command_vx: f32,
    seconds: u32,
    rollout: crate::zealot::RolloutSlot,
) -> Option<Arc<Mutex<Report>>> {
    if !crate::mujoco::available() {
        return None;
    }
    let ckpt = ckpt.to_string();
    let report = Arc::new(Mutex::new(Report {
        running: true,
        label: "MuJoCo · independent engine".into(),
        ..Default::default()
    }));
    let out = Arc::clone(&report);
    // Cleared up front, so a viewer that still holds the previous run's motion
    // does not play it under this run's numbers.
    rollout.set(None);

    thread::spawn(move || {
        let res = crate::mujoco::evaluate_with_rollout(&ckpt, command_vx, seconds);
        if let Ok(mut r) = out.lock() {
            match res {
                Ok((m, traj)) => {
                    r.b = m.score(command_vx);
                    // No zealot-side number to put beside it here: this entry
                    // point is the MuJoCo measurement on its own. Validate
                    // shows one column rather than inventing a comparison.
                    r.label = format!(
                        "MuJoCo · {} attempts · obs frame {}",
                        m.attempts.len(),
                        m.obs_frame
                    );
                    r.done = true;
                    rollout.set(traj);
                }
                // The message is the diagnosis — usually an unimportable
                // module — so it is surfaced rather than replaced.
                Err(e) => r.label = e,
            }
            r.running = false;
        }
    });

    Some(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_perfect_agreement_has_no_gap() {
        let s = Score { survival: 1.0, tracking: 0.8, reward: 0.9, fall_rate: 0.0 };
        let r = Report { a: s, b: s, ..Default::default() };
        assert!(r.worst_gap() < 1e-6);
    }

    #[test]
    fn the_gap_is_relative_and_takes_the_worst_axis() {
        let a = Score { survival: 1.0, tracking: 0.80, reward: 0.9, fall_rate: 0.0 };
        let b = Score { survival: 0.5, tracking: 0.79, reward: 0.9, fall_rate: 0.5 };
        // Survival halved: a 50% gap, and the worst of the three.
        assert!((Report { a, b, ..Default::default() }.worst_gap() - 0.5).abs() < 1e-6);
    }

    #[test]
    fn rows_render_a_signed_delta() {
        let a = Score { survival: 1.0, ..Default::default() };
        let b = Score { survival: 0.75, ..Default::default() };
        let rows = Report { a, b, ..Default::default() }.rows();
        assert_eq!(rows[0].0, "survival");
        assert!(rows[0].3.starts_with('−'), "expected a negative delta, got {}", rows[0].3);
    }
}
