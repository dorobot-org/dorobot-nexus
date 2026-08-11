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
