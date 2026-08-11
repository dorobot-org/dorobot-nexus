//! The robustness sweep.
//!
//! Take a checkpoint and run it across a grid of physical parameters, several
//! episodes per cell, and record how often it survives. A policy that is green
//! in the middle and red at the edges has learned the average of its training
//! distribution rather than the task — and that shape is the best sim-to-real
//! prediction this product can compute indoors.
//!
//! Runs on its own thread, like training, and reports progress as it goes: a
//! sweep that cannot be watched is a sweep nobody runs twice.

use std::sync::{Arc, Mutex};
use std::thread;

use crate::ckpt;
use crate::env::{Params, VecEnv, N_ACT, N_OBS};
use crate::rl::{Config, Ppo};
use crate::rng::Rng;

pub const COLS: usize = 8;
pub const ROWS: usize = 5;
/// Episodes per cell. Enough that a pass rate means something, few enough that
/// the whole grid finishes while you are looking at it.
const EPISODES: usize = 12;
const MAX_STEPS: usize = 400;

/// Axis bounds. Deliberately wider than the training distribution
/// (force 0.8–1.2, mass 0.7–1.6): a sweep confined to what the policy already
/// saw cannot tell you anything you did not already know.
pub const FORCE_RANGE: (f32, f32) = (0.25, 2.0);
pub const MASS_RANGE: (f32, f32) = (0.3, 5.0);

/// A cell scores how well the policy did the task there, from 0 to 1: the mean
/// tracking quality over its episodes, with a fall scoring zero.
///
/// Two earlier attempts were worse and are worth recording. Scoring *survival*
/// made a randomised and a non-randomised policy look identical and perfect
/// everywhere — a measurement that cannot fail is not a measurement. Replacing
/// it with a pass/fail tracking threshold then made every cell identical again,
/// because whether a threshold is met depends mostly on how large the commanded
/// velocity is, which is the same in every cell. A continuous score varies with
/// the physics, which is the thing the axes actually change.

#[derive(Default)]
pub struct Surface {
    /// Pass rate per cell, row-major. Empty until the sweep has run a cell.
    pub cells: Vec<f32>,
    pub done: usize,
    pub total: usize,
    pub running: bool,
    pub label: String,
}

impl Surface {
    pub fn cell(&self, row: usize, col: usize) -> Option<f32> {
        self.cells.get(row * COLS + col).copied().filter(|v| *v >= 0.0)
    }

    /// Share of evaluated cells where the policy did the task acceptably.
    pub fn pass_fraction(&self) -> f32 {
        let scored: Vec<f32> = self.cells.iter().copied().filter(|v| *v >= 0.0).collect();
        if scored.is_empty() {
            return 0.0;
        }
        scored.iter().filter(|v| **v > 0.5).count() as f32 / scored.len() as f32
    }
}

pub fn axis_force(col: usize) -> f32 {
    let t = col as f32 / (COLS - 1) as f32;
    FORCE_RANGE.0 + t * (FORCE_RANGE.1 - FORCE_RANGE.0)
}

pub fn axis_mass(row: usize) -> f32 {
    let t = row as f32 / (ROWS - 1) as f32;
    MASS_RANGE.0 + t * (MASS_RANGE.1 - MASS_RANGE.0)
}

/// Start a sweep over the newest checkpoint of `run`. Returns `None` when there
/// is nothing to sweep yet, rather than reporting an empty surface as a result.
pub fn spawn(run: &str) -> Option<Arc<Mutex<Surface>>> {
    let (path, manifest) = ckpt::list(run).into_iter().next()?;
    let (weights, _) = ckpt::read(&path).ok()?;

    let surface = Arc::new(Mutex::new(Surface {
        cells: vec![-1.0; ROWS * COLS],
        done: 0,
        total: ROWS * COLS,
        running: true,
        label: format!("step {}k", manifest.step / 1000),
    }));

    let out = Arc::clone(&surface);
    thread::spawn(move || {
        let mut rng = Rng::new(manifest.seed ^ 0x5177EE);
        let hidden = if manifest.hidden == 0 { 64 } else { manifest.hidden };
        let mut ppo = Ppo::new(N_OBS, N_ACT, hidden, Config::default(), &mut rng);
        if !ppo.load_weights(&weights) {
            if let Ok(mut s) = out.lock() {
                s.running = false;
                s.label = "checkpoint does not match its manifest".into();
            }
            return;
        }

        let mut obs = vec![0.0; N_OBS];
        let mut action = vec![0.0; N_ACT];

        for row in 0..ROWS {
            for col in 0..COLS {
                let p = Params {
                    mass_scale: axis_mass(row),
                    force_scale: axis_force(col),
                    damping: 0.0,
                };
                let mut env = VecEnv::new(1, (row * COLS + col) as u64 ^ 0xC0FFEE);
                env.set_fixed(p);

                let mut score = 0.0_f32;
                for ep in 0..EPISODES {
                    // A spread of commands per cell, so a cell measures the
                    // task rather than one lucky target.
                    let cmd = -0.7 + 1.4 * (ep as f32 / (EPISODES - 1) as f32);
                    env.restart(0, cmd);
                    env.set_fixed(p);
                    let mut fell = false;
                    let mut track = 0.0_f32;
                    let mut steps = 0usize;
                    for _ in 0..MAX_STEPS {
                        env.observe(0, &mut obs);
                        ppo.act_mean(&obs, &mut action);
                        let o = env.step(&[action.clone()]);
                        // Term 0 is track_lin_vel: how well it held the command.
                        track += o.terms[0][0];
                        steps += 1;
                        if o.fell[0] {
                            fell = true;
                            break;
                        }
                        if o.done[0] {
                            break;
                        }
                    }
                    // A fall scores zero however well it tracked beforehand:
                    // falling over is not a partial success.
                    score += if fell { 0.0 } else { track / steps.max(1) as f32 };
                }

                if let Ok(mut s) = out.lock() {
                    s.cells[row * COLS + col] = score / EPISODES as f32;
                    s.done += 1;
                }
            }
        }

        if let Ok(mut s) = out.lock() {
            s.running = false;
        }
    });

    Some(surface)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_axes_span_their_declared_ranges() {
        assert!((axis_force(0) - FORCE_RANGE.0).abs() < 1e-6);
        assert!((axis_force(COLS - 1) - FORCE_RANGE.1).abs() < 1e-6);
        assert!((axis_mass(0) - MASS_RANGE.0).abs() < 1e-6);
        assert!((axis_mass(ROWS - 1) - MASS_RANGE.1).abs() < 1e-6);
    }

    #[test]
    fn unevaluated_cells_are_not_reported_as_scores() {
        let s = Surface {
            cells: vec![-1.0; ROWS * COLS],
            ..Default::default()
        };
        assert_eq!(s.cell(0, 0), None);
        assert_eq!(s.pass_fraction(), 0.0);
    }

    #[test]
    fn pass_fraction_counts_only_cells_above_half() {
        let mut cells = vec![-1.0; ROWS * COLS];
        cells[0] = 1.0;
        cells[1] = 0.9;
        cells[2] = 0.2;
        let s = Surface { cells, ..Default::default() };
        assert!((s.pass_fraction() - 2.0 / 3.0).abs() < 1e-6);
    }
}
