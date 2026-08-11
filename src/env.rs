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

/// The physical parameters an episode is rolled with.
///
/// A policy trained at one point in this space and evaluated at the same point
/// has learned the point, not the task. Randomising them during training and
/// sweeping them during validation is the whole sim-to-real argument.
#[derive(Clone, Copy, Debug)]
pub struct Params {
    /// Multiplier on pole mass — the payload the legs did not expect.
    pub mass_scale: f32,
    /// Multiplier on actuator authority.
    pub force_scale: f32,
    /// Viscous drag on the cart, standing in for surface friction.
    pub damping: f32,
}

impl Default for Params {
    fn default() -> Self {
        Self { mass_scale: 1.0, force_scale: 1.0, damping: 0.0 }
    }
}

/// Ranges sampled per episode. Equal bounds mean the axis is not randomised.
#[derive(Clone, Copy, Debug)]
pub struct Randomization {
    pub mass: (f32, f32),
    pub force: (f32, f32),
    pub damping: (f32, f32),
}

impl Randomization {
    /// Off: every episode gets nominal physics.
    pub const NONE: Self = Self {
        mass: (1.0, 1.0),
        force: (1.0, 1.0),
        damping: (0.0, 0.0),
    };

    /// The default training distribution.
    pub const TRAIN: Self = Self {
        mass: (0.7, 1.6),
        force: (0.8, 1.2),
        damping: (0.0, 0.25),
    };

    fn sample(&self, rng: &mut Rng) -> Params {
        Params {
            mass_scale: rng.uniform(self.mass.0, self.mass.1),
            force_scale: rng.uniform(self.force.0, self.force.1),
            damping: rng.uniform(self.damping.0, self.damping.1),
        }
    }
}
/// Past this lean the episode ends. The equivalent of a humanoid falling.
const FALL_ANGLE: f32 = 0.42;
const MAX_STEPS: u32 = 400;
/// Track width. It has to fit the distance a full episode at the largest
/// command covers — 0.8 m/s for 8 s is 6.4 m — or the task contradicts itself:
/// tracking any sustained command would leave the box, and leaving it used to
/// be scored as a fall. The policy correctly learned that moving meant death,
/// and stood still. Measured with --track-check, not guessed.
const X_LIMIT: f32 = 30.0;

/// Cart and pole accelerations. Shared by both integrators so they are
/// provably solving the same physics.
fn accel(th: f32, dth: f32, dx: f32, force: f32, p: &Params) -> (f32, f32) {
    let (sin, cos) = th.sin_cos();
    let pole_m = POLE_M * p.mass_scale;
    let total_m = CART_M + pole_m;
    let pml = pole_m * POLE_L;
    let temp = (force + pml * dth * dth * sin) / total_m;
    let ddth =
        (GRAVITY * sin - cos * temp) / (POLE_L * (4.0 / 3.0 - pole_m * cos * cos / total_m));
    let ddx = temp - pml * ddth * cos / total_m - p.damping * dx;
    (ddx, ddth)
}

#[derive(Clone, Copy)]
struct State {
    x: f32,
    dx: f32,
    th: f32,
    dth: f32,
    cmd: f32,
    prev_a: f32,
    steps: u32,
    p: Params,
}

impl Default for State {
    fn default() -> Self {
        Self {
            x: 0.0, dx: 0.0, th: 0.0, dth: 0.0, cmd: 0.0,
            prev_a: 0.0, steps: 0, p: Params::default(),
        }
    }
}

/// How the equations of motion are integrated.
///
/// The training environment uses semi-implicit Euler, which is what fast
/// simulators use. `Rk4` solves the same dynamics with a fourth-order
/// Runge-Kutta step instead — a different numerical implementation of the same
/// physics. Running a policy in both is this project's analogue of zealot
/// validating against MuJoCo: a gait that exploits one integrator's error will
/// not survive the other, and finding that indoors is the entire point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Integrator {
    Euler,
    Rk4,
}

/// A population of independent environments stepped together.
pub struct VecEnv {
    s: Vec<State>,
    rng: Rng,
    rand: Randomization,
    integrator: Integrator,
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
        Self::with_randomization(n, seed, Randomization::NONE)
    }

    pub fn with_randomization(n: usize, seed: u64, rand: Randomization) -> Self {
        let mut rng = Rng::new(seed);
        let mut s = vec![State::default(); n];
        for st in s.iter_mut() {
            Self::reset_one(st, &mut rng, &rand);
        }
        Self { s, rng, rand, integrator: Integrator::Euler }
    }

    pub fn set_integrator(&mut self, i: Integrator) {
        self.integrator = i;
    }

    /// Pin every environment to one point in parameter space. What the sweep
    /// uses: a cell of the robustness surface is a fixed physics, not a draw.
    pub fn set_fixed(&mut self, p: Params) {
        self.rand = Randomization {
            mass: (p.mass_scale, p.mass_scale),
            force: (p.force_scale, p.force_scale),
            damping: (p.damping, p.damping),
        };
        for st in self.s.iter_mut() {
            st.p = p;
        }
    }

    pub fn len(&self) -> usize {
        self.s.len()
    }

    fn reset_one(st: &mut State, rng: &mut Rng, rand: &Randomization) {
        st.x = rng.uniform(-0.05, 0.05);
        st.dx = rng.uniform(-0.05, 0.05);
        st.th = rng.uniform(-0.05, 0.05);
        st.dth = rng.uniform(-0.05, 0.05);
        // A fresh velocity command per episode, which is what makes this a
        // tracking task rather than a balancing one.
        st.cmd = rng.uniform(-0.8, 0.8);
        st.prev_a = 0.0;
        st.steps = 0;
        // Physics is drawn per episode, which is what makes it randomisation
        // rather than a one-off perturbation at construction.
        st.p = rand.sample(rng);
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
            let integrator = self.integrator;
            let st = &mut self.s[i];
            let force = a * FORCE * st.p.force_scale;

            match integrator {
                Integrator::Euler => {
                    let (ddx, ddth) = accel(st.th, st.dth, st.dx, force, &st.p);
                    st.dx += DT * ddx;
                    st.x += DT * st.dx;
                    st.dth += DT * ddth;
                    st.th += DT * st.dth;
                }
                Integrator::Rk4 => {
                    // y = [x, dx, th, dth]
                    let y = [st.x, st.dx, st.th, st.dth];
                    let f = |y: [f32; 4]| -> [f32; 4] {
                        let (ddx, ddth) = accel(y[2], y[3], y[1], force, &st.p);
                        [y[1], ddx, y[3], ddth]
                    };
                    let add = |y: [f32; 4], k: [f32; 4], h: f32| {
                        [y[0] + h * k[0], y[1] + h * k[1], y[2] + h * k[2], y[3] + h * k[3]]
                    };
                    let k1 = f(y);
                    let k2 = f(add(y, k1, DT * 0.5));
                    let k3 = f(add(y, k2, DT * 0.5));
                    let k4 = f(add(y, k3, DT));
                    for (j, slot) in [&mut st.x, &mut st.dx, &mut st.th, &mut st.dth]
                        .into_iter()
                        .enumerate()
                    {
                        *slot = y[j]
                            + DT / 6.0 * (k1[j] + 2.0 * k2[j] + 2.0 * k3[j] + k4[j]);
                    }
                }
            }
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

            // Falling and running out of track are different failures. Only
            // the first is a fall.
            let toppled = st.th.abs() > FALL_ANGLE;
            let off_track = st.x.abs() > X_LIMIT;
            let timeout = st.steps >= MAX_STEPS;
            if toppled {
                fell[i] = true;
            }
            if toppled || off_track || timeout {
                done[i] = true;
            }
        }

        // Reset after the observation of the terminal step has been taken.
        for i in 0..n {
            if done[i] {
                let r = self.rand;
                Self::reset_one(&mut self.s[i], &mut self.rng, &r);
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
        let r = self.rand;
        Self::reset_one(&mut self.s[i], &mut self.rng, &r);
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
    fn a_heavier_pole_falls_differently_than_a_light_one() {
        // Same initial lean, same (zero) action, different mass: the
        // trajectories must diverge, or randomisation is doing nothing.
        let mut light = VecEnv::new(1, 4);
        let mut heavy = VecEnv::new(1, 4);
        light.set_fixed(Params { mass_scale: 0.7, ..Default::default() });
        heavy.set_fixed(Params { mass_scale: 1.6, ..Default::default() });
        for e in [&mut light, &mut heavy] {
            e.s[0].th = 0.15;
            e.s[0].dth = 0.0;
            e.s[0].dx = 0.0;
        }
        for _ in 0..20 {
            light.step(&vec![vec![0.0]]);
            heavy.step(&vec![vec![0.0]]);
        }
        let (_, a) = light.pose(0);
        let (_, b) = heavy.pose(0);
        assert!((a - b).abs() > 1e-4, "mass had no effect: {a} vs {b}");
    }

    #[test]
    fn damping_slows_the_cart() {
        let mut free = VecEnv::new(1, 8);
        let mut drag = VecEnv::new(1, 8);
        free.set_fixed(Params { damping: 0.0, ..Default::default() });
        drag.set_fixed(Params { damping: 3.0, ..Default::default() });
        for e in [&mut free, &mut drag] {
            e.s[0].th = 0.0;
            e.s[0].dx = 1.0;
        }
        for _ in 0..10 {
            free.step(&vec![vec![0.0]]);
            drag.step(&vec![vec![0.0]]);
        }
        assert!(drag.s[0].dx.abs() < free.s[0].dx.abs(), "damping did nothing");
    }

    #[test]
    fn the_two_integrators_agree_closely_but_not_exactly() {
        // Same physics, different numerics: they must track each other over a
        // short horizon, and must not be bit-identical — otherwise one of them
        // is not doing what it claims.
        let mut a = VecEnv::new(1, 21);
        let mut b = VecEnv::new(1, 21);
        b.set_integrator(Integrator::Rk4);
        for e in [&mut a, &mut b] {
            e.s[0].th = 0.10;
            e.s[0].dth = 0.0;
            e.s[0].dx = 0.0;
            e.s[0].x = 0.0;
        }
        for _ in 0..25 {
            a.step(&vec![vec![0.2]]);
            b.step(&vec![vec![0.2]]);
        }
        let (_, ta) = a.pose(0);
        let (_, tb) = b.pose(0);
        assert!((ta - tb).abs() < 0.05, "integrators diverged wildly: {ta} vs {tb}");
        assert!((ta - tb).abs() > 1e-7, "integrators are identical; rk4 is not running");
    }

    #[test]
    fn observations_are_the_declared_width() {
        let env = VecEnv::new(1, 5);
        let mut obs = vec![0.0; N_OBS];
        env.observe(0, &mut obs);
        assert_eq!(obs.len(), N_OBS);
    }
}
