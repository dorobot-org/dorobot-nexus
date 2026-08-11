//! A vectorised balance-and-track task.
//!
//! This is not the G1. It is the smallest task that carries the *shape* of
//! zealot's whole-body problem — stay upright while tracking a commanded
//! velocity, with the same named reward terms — so the training loop, the
//! reward decomposition and the diagnosis rail are exercised by real numbers
//! rather than generated ones.
//!
//! An inverted pendulum on a cart, integrated semi-implicitly at 50 Hz. The
//! dynamics are the standard ones; the interesting part is that the reward is
//! decomposed into the same named terms the Task screen displays, because a
//! term you cannot name is a term you cannot blame.

use crate::rng::Rng;

pub const N_OBS: usize = 5;
pub const N_ACT: usize = 1;

/// Reward terms, in the order the UI lists them.
pub const TERM_NAMES: [&str; 4] = ["track_lin_vel", "upright", "action_rate", "torque"];
pub const TERM_WEIGHTS: [f32; 4] = [1.0, 0.5, -0.02, -0.001];

const DT: f32 = 0.02;
const GRAVITY: f32 = 9.81;
const CART_M: f32 = 1.0;
const POLE_M: f32 = 0.1;
const POLE_L: f32 = 0.5;
const FORCE: f32 = 12.0;
/// Past this lean the episode ends. The equivalent of a humanoid falling.
const FALL_ANGLE: f32 = 0.42;
const MAX_STEPS: u32 = 400;

#[derive(Clone, Copy, Default)]
struct State {
    x: f32,
    dx: f32,
    th: f32,
    dth: f32,
    cmd: f32,
    prev_a: f32,
    steps: u32,
}

/// A population of independent environments stepped together.
pub struct VecEnv {
    s: Vec<State>,
    rng: Rng,
}

/// What one step produced, per environment.
pub struct StepOut {
    pub reward: Vec<f32>,
    /// Per-term contribution, `[term][env]`, before weighting.
    pub terms: Vec<Vec<f32>>,
    pub done: Vec<bool>,
    /// Terminated by falling rather than by the time limit.
    pub fell: Vec<bool>,
}

impl VecEnv {
    pub fn new(n: usize, seed: u64) -> Self {
        let mut rng = Rng::new(seed);
        let mut s = vec![State::default(); n];
        for st in s.iter_mut() {
            Self::reset_one(st, &mut rng);
        }
        Self { s, rng }
    }

    pub fn len(&self) -> usize {
        self.s.len()
    }

    fn reset_one(st: &mut State, rng: &mut Rng) {
        st.x = rng.uniform(-0.05, 0.05);
        st.dx = rng.uniform(-0.05, 0.05);
        st.th = rng.uniform(-0.05, 0.05);
        st.dth = rng.uniform(-0.05, 0.05);
        // A fresh velocity command per episode, which is what makes this a
        // tracking task rather than a balancing one.
        st.cmd = rng.uniform(-0.8, 0.8);
        st.prev_a = 0.0;
        st.steps = 0;
    }

    pub fn observe(&self, i: usize, out: &mut [f32]) {
        let s = &self.s[i];
        out[0] = s.x;
        out[1] = s.dx;
        out[2] = s.th;
        out[3] = s.dth;
        out[4] = s.cmd;
    }

    pub fn step(&mut self, actions: &[Vec<f32>]) -> StepOut {
        let n = self.s.len();
        let mut reward = vec![0.0; n];
        let mut terms = vec![vec![0.0; n]; TERM_NAMES.len()];
        let mut done = vec![false; n];
        let mut fell = vec![false; n];

        for i in 0..n {
            let a = actions[i][0].clamp(-1.0, 1.0);
            let force = a * FORCE;
            let st = &mut self.s[i];

            // Standard cart-pole dynamics, semi-implicit Euler.
            let (sin, cos) = st.th.sin_cos();
            let total_m = CART_M + POLE_M;
            let pml = POLE_M * POLE_L;
            let temp = (force + pml * st.dth * st.dth * sin) / total_m;
            let ddth = (GRAVITY * sin - cos * temp)
                / (POLE_L * (4.0 / 3.0 - POLE_M * cos * cos / total_m));
            let ddx = temp - pml * ddth * cos / total_m;

            st.dx += DT * ddx;
            st.x += DT * st.dx;
            st.dth += DT * ddth;
            st.th += DT * st.dth;
            st.steps += 1;

            // The same decomposition the Task screen shows.
            let t_track = (-3.0 * (st.dx - st.cmd).abs()).exp();
            let t_upright = (-6.0 * st.th.abs()).exp();
            let t_rate = (a - st.prev_a) * (a - st.prev_a);
            let t_torque = a * a;
            st.prev_a = a;

            terms[0][i] = t_track;
            terms[1][i] = t_upright;
            terms[2][i] = t_rate;
            terms[3][i] = t_torque;
            reward[i] = TERM_WEIGHTS[0] * t_track
                + TERM_WEIGHTS[1] * t_upright
                + TERM_WEIGHTS[2] * t_rate
                + TERM_WEIGHTS[3] * t_torque;

            let out_of_bounds = st.th.abs() > FALL_ANGLE || st.x.abs() > 3.0;
            let timeout = st.steps >= MAX_STEPS;
            if out_of_bounds {
                fell[i] = true;
            }
            if out_of_bounds || timeout {
                done[i] = true;
            }
        }

        // Reset after the observation of the terminal step has been taken.
        for i in 0..n {
            if done[i] {
                Self::reset_one(&mut self.s[i], &mut self.rng);
            }
        }
        StepOut { reward, terms, done, fell }
    }

    /// Cart position and pole angle, for drawing.
    pub fn pose(&self, i: usize) -> (f32, f32) {
        (self.s[i].x, self.s[i].th)
    }

    /// Apply an impulse to the cart. This is the "push" in Inspect: because the
    /// simulation is in this process, perturbing it is a function call.
    pub fn push(&mut self, i: usize, dv: f32) {
        self.s[i].dx += dv;
    }

    /// Force a fresh episode with a chosen command, for a repeatable probe.
    pub fn restart(&mut self, i: usize, cmd: f32) {
        let rng = &mut self.rng;
        Self::reset_one(&mut self.s[i], rng);
        self.s[i].cmd = cmd;
    }

    /// Lean of each environment, for the contact sheet. Normalised to ±1.
    pub fn leans(&self) -> Vec<f32> {
        self.s.iter().map(|s| (s.th / FALL_ANGLE).clamp(-1.0, 1.0)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unactuated_pole_falls() {
        let mut env = VecEnv::new(1, 3);
        // Nudge it off centre, then never push the cart.
        let zero = vec![vec![0.0_f32]];
        let mut fell = false;
        for _ in 0..MAX_STEPS {
            let out = env.step(&zero);
            if out.fell[0] {
                fell = true;
                break;
            }
        }
        assert!(fell, "an unactuated pendulum should fall within one episode");
    }

    #[test]
    fn upright_scores_better_than_leaning() {
        let mut env = VecEnv::new(2, 1);
        env.s[0].th = 0.0;
        env.s[1].th = 0.35;
        env.s[0].cmd = 0.0;
        env.s[1].cmd = 0.0;
        let out = env.step(&vec![vec![0.0], vec![0.0]]);
        assert!(
            out.terms[1][0] > out.terms[1][1],
            "the upright term must prefer upright"
        );
    }

    #[test]
    fn observations_are_the_declared_width() {
        let env = VecEnv::new(1, 5);
        let mut obs = vec![0.0; N_OBS];
        env.observe(0, &mut obs);
        assert_eq!(obs.len(), N_OBS);
    }
}
