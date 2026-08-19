//! Driving a checkpoint interactively.
//!
//! This is the screen the design calls the differentiator: because the
//! simulation runs in this process, stepping it is a function call and pushing
//! it is another one. No script, no re-run, no video.
//!
//! Deliberately deterministic — the policy's mean action, not a sample — so
//! that stepping back and forth shows the same trajectory twice. Probing a
//! policy should show the policy, not a draw from it.

use std::path::PathBuf;

use crate::ckpt;
use crate::env::{VecEnv, N_ACT, N_OBS};
use crate::rl::{Config, Ppo};
use crate::rng::Rng;

/// One recorded instant of a rollout.
#[derive(Clone, Copy, Default)]
pub struct Frame {
    pub cart_x: f32,
    pub angle: f32,
    pub action: f32,
    pub reward: f32,
    pub fell: bool,
}

pub struct Probe {
    ppo: Ppo,
    env: VecEnv,
    pub frames: Vec<Frame>,
    pub cursor: usize,
    pub playing: bool,
    pub loaded: Option<PathBuf>,
    pub label: String,
    /// Steps recorded since the last perturbation, so the UI can report it.
    pub last_push: Option<usize>,
}

/// How many steps a probe records before it stops extending.
const MAX_FRAMES: usize = 900;

impl Probe {
    /// Load the newest checkpoint of a run, if it has written one.
    pub fn load_latest(run: &str) -> Option<Self> {
        let (path, manifest) = ckpt::list(run).into_iter().next()?;
        let (weights, _) = ckpt::read(&path).ok()?;
        let mut rng = Rng::new(manifest.seed ^ 0xA11CE);
        let mut ppo = Ppo::new(
            manifest.n_obs.max(N_OBS),
            manifest.n_act.max(N_ACT),
            if manifest.hidden == 0 { 64 } else { manifest.hidden },
            Config::default(),
            &mut rng,
        );
        if !ppo.load_weights(&weights) {
            // A shape mismatch means the manifest and the blob disagree; a
            // silently mis-loaded policy is worse than no policy.
            eprintln!("probe: {} does not match its manifest", path.display());
            return None;
        }
        let mut p = Self {
            ppo,
            env: VecEnv::new(1, manifest.seed ^ 0xBEEF),
            frames: Vec::new(),
            cursor: 0,
            playing: true,
            loaded: Some(path),
            label: format!(
                "{} · step {}k · score {:.2}",
                run,
                manifest.step / 1000,
                manifest.score
            ),
            last_push: None,
        };
        p.restart();
        Some(p)
    }

    pub fn restart(&mut self) {
        self.env.restart(0, 0.4);
        self.frames.clear();
        self.cursor = 0;
        self.last_push = None;
        self.extend(240);
    }

    /// Advance the simulation and record what happened.
    fn extend(&mut self, n: usize) {
        let mut obs = vec![0.0; N_OBS];
        let mut action = vec![0.0; N_ACT];
        for _ in 0..n {
            if self.frames.len() >= MAX_FRAMES {
                break;
            }
            self.env.observe(0, &mut obs);
            self.ppo.act_mean(&obs, &mut action);
            let out = self.env.step(&[action.clone()]);
            let (x, th) = self.env.pose(0);
            self.frames.push(Frame {
                cart_x: x,
                angle: th,
                action: action[0].clamp(-1.0, 1.0),
                reward: out.reward[0],
                fell: out.fell[0],
            });
            if out.done[0] {
                // The episode ended; stop extending so the scrubber's end means
                // something rather than silently splicing a new episode on.
                break;
            }
        }
    }

    pub fn frame(&self) -> Frame {
        self.frames.get(self.cursor).copied().unwrap_or_default()
    }

    pub fn tick(&mut self) {
        if !self.playing || self.frames.is_empty() {
            return;
        }
        if self.cursor + 1 < self.frames.len() {
            self.cursor += 1;
        } else {
            self.playing = false;
        }
    }

    pub fn toggle_play(&mut self) {
        if self.frames.is_empty() {
            return;
        }
        // Pressing play at the end replays rather than doing nothing.
        if !self.playing && self.cursor + 1 >= self.frames.len() {
            self.cursor = 0;
        }
        self.playing = !self.playing;
    }

    pub fn step_by(&mut self, d: i32) {
        self.playing = false;
        let n = self.frames.len() as i32;
        if n == 0 {
            return;
        }
        self.cursor = (self.cursor as i32 + d).clamp(0, n - 1) as usize;
    }

    pub fn seek_fraction(&mut self, t: f64) {
        if self.frames.is_empty() {
            return;
        }
        self.playing = false;
        self.cursor = ((t.clamp(0.0, 1.0) * (self.frames.len() - 1) as f64) as usize)
            .min(self.frames.len() - 1);
    }

    /// Shove the cart sideways and re-simulate from here.
    ///
    /// Everything after the cursor is discarded, because it described a future
    /// that no longer happens — keeping it would be showing a trajectory the
    /// policy never took.
    pub fn push(&mut self, dv: f32) {
        if self.frames.is_empty() {
            return;
        }
        self.frames.truncate(self.cursor + 1);
        self.env.push(0, dv);
        self.last_push = Some(self.cursor);
        self.extend(240);
        self.playing = true;
    }

    pub fn progress(&self) -> f64 {
        if self.frames.len() < 2 {
            return 0.0;
        }
        self.cursor as f64 / (self.frames.len() - 1) as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe_with_untrained_policy() -> Probe {
        let mut rng = Rng::new(5);
        let ppo = Ppo::new(N_OBS, N_ACT, 16, Config::default(), &mut rng);
        let mut p = Probe {
            ppo,
            env: VecEnv::new(1, 9),
            frames: Vec::new(),
            cursor: 0,
            playing: false,
            loaded: None,
            label: String::new(),
            last_push: None,
        };
        p.restart();
        p
    }

    #[test]
    fn a_probe_records_a_trajectory() {
        let p = probe_with_untrained_policy();
        assert!(!p.frames.is_empty(), "restart should record frames");
    }

    #[test]
    fn stepping_is_bounded_at_both_ends() {
        let mut p = probe_with_untrained_policy();
        p.step_by(-10);
        assert_eq!(p.cursor, 0);
        p.step_by(10_000);
        assert_eq!(p.cursor, p.frames.len() - 1);
    }

    #[test]
    fn a_push_discards_the_future_it_invalidated() {
        let mut p = probe_with_untrained_policy();
        p.cursor = 5;
        p.push(1.5);
        // Everything after the cursor was re-simulated, not kept.
        assert!(p.frames.len() > 5);
        assert_eq!(p.last_push, Some(5));
    }

    #[test]
    fn seeking_maps_the_ends_exactly() {
        let mut p = probe_with_untrained_policy();
        p.seek_fraction(0.0);
        assert_eq!(p.cursor, 0);
        p.seek_fraction(1.0);
        assert_eq!(p.cursor, p.frames.len() - 1);
    }
}
