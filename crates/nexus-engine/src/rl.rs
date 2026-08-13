//! PPO, GAE and Adam — the learning stack, on the CPU.
//!
//! This is the same algorithm `zealot-rl` implements on the GPU (itself a port
//! of `rsl_rl`). It is here rather than linked because zealot's GPU path cannot
//! currently be built outside its author's machine: `nexus3d` is unpublished,
//! and zealot's own manifest redirects `vortx`/`khal` to unpublished dimforge
//! forks plus a vendored `naga` carrying a Metal fix. See README.
//!
//! Small and hand-rolled on purpose — a dense MLP with explicit backward passes
//! and no autodiff crate, so the whole learning loop stays readable and the
//! gradient check below can verify it.

use crate::rng::Rng;

/// A dense layer with its cached forward pass.
#[derive(Clone)]
struct Linear {
    w: Vec<f32>, // [out * in]
    b: Vec<f32>, // [out]
    n_in: usize,
    n_out: usize,
    // Gradients, accumulated over a minibatch.
    gw: Vec<f32>,
    gb: Vec<f32>,
    // Adam moments.
    mw: Vec<f32>,
    vw: Vec<f32>,
    mb: Vec<f32>,
    vb: Vec<f32>,
}

impl Linear {
    fn new(n_in: usize, n_out: usize, gain: f32, rng: &mut Rng) -> Self {
        // Orthogonal-ish init is what rsl_rl uses; scaled uniform is close
        // enough at this size and keeps the code honest about what it does.
        let bound = gain * (1.0 / n_in as f32).sqrt();
        let w = (0..n_in * n_out).map(|_| rng.uniform(-bound, bound)).collect();
        Self {
            w,
            b: vec![0.0; n_out],
            n_in,
            n_out,
            gw: vec![0.0; n_in * n_out],
            gb: vec![0.0; n_out],
            mw: vec![0.0; n_in * n_out],
            vw: vec![0.0; n_in * n_out],
            mb: vec![0.0; n_out],
            vb: vec![0.0; n_out],
        }
    }

    fn forward(&self, x: &[f32], out: &mut [f32]) {
        for o in 0..self.n_out {
            let row = &self.w[o * self.n_in..(o + 1) * self.n_in];
            let mut acc = self.b[o];
            for i in 0..self.n_in {
                acc += row[i] * x[i];
            }
            out[o] = acc;
        }
    }

    /// Accumulate gradients for one sample and return dL/dx.
    fn backward(&mut self, x: &[f32], dout: &[f32], dx: &mut [f32]) {
        dx.iter_mut().for_each(|v| *v = 0.0);
        for o in 0..self.n_out {
            let g = dout[o];
            if g == 0.0 {
                continue;
            }
            self.gb[o] += g;
            let base = o * self.n_in;
            for i in 0..self.n_in {
                self.gw[base + i] += g * x[i];
                dx[i] += g * self.w[base + i];
            }
        }
    }

    fn zero_grad(&mut self) {
        self.gw.iter_mut().for_each(|v| *v = 0.0);
        self.gb.iter_mut().for_each(|v| *v = 0.0);
    }

    fn adam(&mut self, lr: f32, t: i32, scale: f32) {
        const B1: f32 = 0.9;
        const B2: f32 = 0.999;
        const EPS: f32 = 1e-8;
        let c1 = 1.0 - B1.powi(t);
        let c2 = 1.0 - B2.powi(t);
        for i in 0..self.w.len() {
            let g = self.gw[i] * scale;
            self.mw[i] = B1 * self.mw[i] + (1.0 - B1) * g;
            self.vw[i] = B2 * self.vw[i] + (1.0 - B2) * g * g;
            self.w[i] -= lr * (self.mw[i] / c1) / ((self.vw[i] / c2).sqrt() + EPS);
        }
        for i in 0..self.b.len() {
            let g = self.gb[i] * scale;
            self.mb[i] = B1 * self.mb[i] + (1.0 - B1) * g;
            self.vb[i] = B2 * self.vb[i] + (1.0 - B2) * g * g;
            self.b[i] -= lr * (self.mb[i] / c1) / ((self.vb[i] / c2).sqrt() + EPS);
        }
    }
}

/// Two hidden layers of tanh, then a linear head.
#[derive(Clone)]
pub struct Mlp {
    l0: Linear,
    l1: Linear,
    l2: Linear,
    // Forward caches, reused per sample to keep allocation out of the loop.
    h0: Vec<f32>,
    a0: Vec<f32>,
    h1: Vec<f32>,
    a1: Vec<f32>,
    out: Vec<f32>,
}

impl Mlp {
    pub fn new(n_in: usize, hidden: usize, n_out: usize, out_gain: f32, rng: &mut Rng) -> Self {
        Self {
            l0: Linear::new(n_in, hidden, 1.0, rng),
            l1: Linear::new(hidden, hidden, 1.0, rng),
            l2: Linear::new(hidden, n_out, out_gain, rng),
            h0: vec![0.0; hidden],
            a0: vec![0.0; hidden],
            h1: vec![0.0; hidden],
            a1: vec![0.0; hidden],
            out: vec![0.0; n_out],
        }
    }

    pub fn forward(&mut self, x: &[f32]) -> &[f32] {
        self.l0.forward(x, &mut self.h0);
        for i in 0..self.h0.len() {
            self.a0[i] = self.h0[i].tanh();
        }
        self.l1.forward(&self.a0, &mut self.h1);
        for i in 0..self.h1.len() {
            self.a1[i] = self.h1[i].tanh();
        }
        self.l2.forward(&self.a1, &mut self.out);
        &self.out
    }

    /// Backward through the cached forward pass. `x` must be the same input.
    pub fn backward(&mut self, x: &[f32], dout: &[f32]) {
        let mut d1 = vec![0.0; self.a1.len()];
        self.l2.backward(&self.a1, dout, &mut d1);
        for i in 0..d1.len() {
            d1[i] *= 1.0 - self.a1[i] * self.a1[i]; // tanh'
        }
        let mut d0 = vec![0.0; self.a0.len()];
        self.l1.backward(&self.a0, &d1, &mut d0);
        for i in 0..d0.len() {
            d0[i] *= 1.0 - self.a0[i] * self.a0[i];
        }
        let mut dx = vec![0.0; x.len()];
        self.l0.backward(x, &d0, &mut dx);
    }

    fn zero_grad(&mut self) {
        self.l0.zero_grad();
        self.l1.zero_grad();
        self.l2.zero_grad();
    }

    fn adam(&mut self, lr: f32, t: i32, scale: f32) {
        self.l0.adam(lr, t, scale);
        self.l1.adam(lr, t, scale);
        self.l2.adam(lr, t, scale);
    }
}

/// Hyperparameters, named as in rsl_rl so they can be compared to zealot's.
#[derive(Clone, Copy)]
pub struct Config {
    pub gamma: f32,
    pub lam: f32,
    pub clip: f32,
    pub lr: f32,
    pub epochs: usize,
    pub minibatches: usize,
    pub entropy_coef: f32,
    pub value_coef: f32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            gamma: 0.99,
            lam: 0.95,
            clip: 0.2,
            lr: 3e-4,
            epochs: 4,
            minibatches: 4,
            entropy_coef: 0.004,
            value_coef: 0.5,
        }
    }
}

/// A diagonal-Gaussian policy with a separate value head.
pub struct Ppo {
    pub actor: Mlp,
    pub critic: Mlp,
    pub log_std: Vec<f32>,
    g_log_std: Vec<f32>,
    m_log_std: Vec<f32>,
    v_log_std: Vec<f32>,
    pub cfg: Config,
    step: i32,
    n_act: usize,
}

impl Ppo {
    pub fn new(n_obs: usize, n_act: usize, hidden: usize, cfg: Config, rng: &mut Rng) -> Self {
        Self {
            // A small output gain keeps the initial policy near zero action,
            // which matters: a loud initial policy falls over before it learns.
            actor: Mlp::new(n_obs, hidden, n_act, 0.01, rng),
            critic: Mlp::new(n_obs, hidden, 1, 1.0, rng),
            log_std: vec![-0.5; n_act],
            g_log_std: vec![0.0; n_act],
            m_log_std: vec![0.0; n_act],
            v_log_std: vec![0.0; n_act],
            cfg,
            step: 0,
            n_act,
        }
    }

    pub fn act(&mut self, obs: &[f32], rng: &mut Rng, action: &mut [f32]) -> (f32, f32) {
        let mean: Vec<f32> = self.actor.forward(obs).to_vec();
        let mut logp = 0.0;
        for i in 0..self.n_act {
            let std = self.log_std[i].exp();
            let z = rng.normal();
            action[i] = mean[i] + std * z;
            logp += -0.5 * z * z - self.log_std[i] - 0.918_938_5; // ln(sqrt(2pi))
        }
        let value = self.critic.forward(obs)[0];
        (logp, value)
    }

    pub fn value(&mut self, obs: &[f32]) -> f32 {
        self.critic.forward(obs)[0]
    }

    /// One PPO update over a rollout batch. Returns (policy loss, value loss).
    pub fn update(&mut self, batch: &Batch, rng: &mut Rng) -> (f32, f32) {
        let n = batch.obs.len();
        let mut idx: Vec<usize> = (0..n).collect();
        let mb = (n / self.cfg.minibatches).max(1);
        let mut pl_sum = 0.0;
        let mut vl_sum = 0.0;
        let mut updates = 0;

        for _ in 0..self.cfg.epochs {
            rng.shuffle(&mut idx);
            for chunk in idx.chunks(mb) {
                self.actor.zero_grad();
                self.critic.zero_grad();
                self.g_log_std.iter_mut().for_each(|v| *v = 0.0);
                let scale = 1.0 / chunk.len() as f32;

                for &k in chunk {
                    let obs = &batch.obs[k];
                    let act = &batch.act[k];
                    let adv = batch.adv[k];
                    let ret = batch.ret[k];
                    let logp_old = batch.logp[k];

                    let mean: Vec<f32> = self.actor.forward(obs).to_vec();
                    let mut logp = 0.0;
                    for i in 0..self.n_act {
                        let std = self.log_std[i].exp();
                        let z = (act[i] - mean[i]) / std;
                        logp += -0.5 * z * z - self.log_std[i] - 0.918_938_5;
                    }
                    let ratio = (logp - logp_old).exp();
                    let clipped = ratio.clamp(1.0 - self.cfg.clip, 1.0 + self.cfg.clip);

                    // d(-min(r*A, clip(r)*A))/d(logp). Zero inside the clip when
                    // the clipped branch wins — that is the whole point of PPO.
                    let unclipped_wins = ratio * adv <= clipped * adv;
                    let dlogp = if unclipped_wins { -adv * ratio } else { 0.0 };

                    let mut dmean = vec![0.0; self.n_act];
                    for i in 0..self.n_act {
                        let std = self.log_std[i].exp();
                        let z = (act[i] - mean[i]) / std;
                        dmean[i] = dlogp * (z / std);
                        // d logp/d log_std = z^2 - 1; entropy adds +1 per dim.
                        self.g_log_std[i] +=
                            dlogp * (z * z - 1.0) - self.cfg.entropy_coef;
                    }
                    self.actor.backward(obs, &dmean);

                    let v = self.critic.forward(obs)[0];
                    let dv = self.cfg.value_coef * 2.0 * (v - ret);
                    self.critic.backward(obs, &[dv]);

                    pl_sum += -(ratio * adv).min(clipped * adv);
                    vl_sum += (v - ret) * (v - ret);
                    updates += 1;
                }

                self.step += 1;
                self.actor.adam(self.cfg.lr, self.step, scale);
                self.critic.adam(self.cfg.lr, self.step, scale);
                for i in 0..self.n_act {
                    let g = self.g_log_std[i] * scale;
                    self.m_log_std[i] = 0.9 * self.m_log_std[i] + 0.1 * g;
                    self.v_log_std[i] = 0.999 * self.v_log_std[i] + 0.001 * g * g;
                    let c1 = 1.0 - 0.9f32.powi(self.step);
                    let c2 = 1.0 - 0.999f32.powi(self.step);
                    self.log_std[i] -= self.cfg.lr * (self.m_log_std[i] / c1)
                        / ((self.v_log_std[i] / c2).sqrt() + 1e-8);
                    // Keep exploration from collapsing or exploding.
                    self.log_std[i] = self.log_std[i].clamp(-2.5, 0.5);
                }
            }
        }
        let d = updates.max(1) as f32;
        (pl_sum / d, vl_sum / d)
    }
}

impl Mlp {
    fn params(&self) -> impl Iterator<Item = &Linear> {
        [&self.l0, &self.l1, &self.l2].into_iter()
    }
    fn params_mut(&mut self) -> impl Iterator<Item = &mut Linear> {
        [&mut self.l0, &mut self.l1, &mut self.l2].into_iter()
    }
}

impl Ppo {
    /// Weights as one flat vector: actor, critic, then log_std.
    pub fn to_weights(&self) -> Vec<f32> {
        let mut out = Vec::new();
        for net in [&self.actor, &self.critic] {
            for l in net.params() {
                out.extend_from_slice(&l.w);
                out.extend_from_slice(&l.b);
            }
        }
        out.extend_from_slice(&self.log_std);
        out
    }

    /// Restore weights written by [`Ppo::to_weights`] into a net of the same
    /// shape. Returns false when the length disagrees, which is the only check
    /// worth making: a shape mismatch means the manifest and the blob disagree.
    pub fn load_weights(&mut self, w: &[f32]) -> bool {
        if w.len() != self.to_weights().len() {
            return false;
        }
        let mut i = 0;
        for net in [&mut self.actor, &mut self.critic] {
            for l in net.params_mut() {
                let nw = l.w.len();
                l.w.copy_from_slice(&w[i..i + nw]);
                i += nw;
                let nb = l.b.len();
                l.b.copy_from_slice(&w[i..i + nb]);
                i += nb;
            }
        }
        let n = self.log_std.len();
        self.log_std.copy_from_slice(&w[i..i + n]);
        true
    }

    /// The mean action, with no exploration noise. What Inspect drives with:
    /// probing a policy should show the policy, not a sample from it.
    pub fn act_mean(&mut self, obs: &[f32], action: &mut [f32]) {
        let mean = self.actor.forward(obs);
        action[..mean.len()].copy_from_slice(mean);
    }
}

/// A flattened rollout, ready for the update.
#[derive(Default)]
pub struct Batch {
    pub obs: Vec<Vec<f32>>,
    pub act: Vec<Vec<f32>>,
    pub logp: Vec<f32>,
    pub adv: Vec<f32>,
    pub ret: Vec<f32>,
}

/// Generalised advantage estimation over one environment's trajectory.
///
/// `values` has one extra entry: the bootstrap value of the state after the
/// last step. `dones` marks steps that terminated, which cuts the trace.
pub fn gae(
    rewards: &[f32],
    values: &[f32],
    dones: &[bool],
    gamma: f32,
    lam: f32,
    adv: &mut [f32],
) {
    let t = rewards.len();
    let mut acc = 0.0;
    for i in (0..t).rev() {
        let mask = if dones[i] { 0.0 } else { 1.0 };
        let delta = rewards[i] + gamma * values[i + 1] * mask - values[i];
        acc = delta + gamma * lam * mask * acc;
        adv[i] = acc;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weights_round_trip_through_a_flat_vector() {
        let mut rng = Rng::new(11);
        let mut a = Ppo::new(5, 1, 16, Config::default(), &mut rng);
        let mut b = Ppo::new(5, 1, 16, Config::default(), &mut rng);
        let w = a.to_weights();
        assert!(b.load_weights(&w));
        // Same weights must give the same deterministic action.
        let obs = [0.1_f32, -0.2, 0.3, 0.0, 0.5];
        let mut xa = [0.0];
        let mut xb = [0.0];
        a.act_mean(&obs, &mut xa);
        b.act_mean(&obs, &mut xb);
        assert!((xa[0] - xb[0]).abs() < 1e-9, "{} vs {}", xa[0], xb[0]);
    }

    #[test]
    fn a_wrong_length_blob_is_refused() {
        let mut rng = Rng::new(2);
        let mut p = Ppo::new(5, 1, 16, Config::default(), &mut rng);
        assert!(!p.load_weights(&[0.0; 3]));
    }

    #[test]
    fn gae_with_no_discount_is_a_reward_to_go_residual() {
        let rewards = [1.0, 1.0, 1.0];
        let values = [0.0, 0.0, 0.0, 0.0];
        let dones = [false, false, false];
        let mut adv = [0.0; 3];
        gae(&rewards, &values, &dones, 1.0, 1.0, &mut adv);
        // With zero values and no discount, advantage is the remaining return.
        assert_eq!(adv, [3.0, 2.0, 1.0]);
    }

    #[test]
    fn a_termination_cuts_the_trace() {
        let rewards = [1.0, 1.0, 1.0];
        let values = [0.0; 4];
        let dones = [false, true, false];
        let mut adv = [0.0; 3];
        gae(&rewards, &values, &dones, 1.0, 1.0, &mut adv);
        // Step 1 terminates, so step 0 sees only its own reward plus step 1's.
        assert_eq!(adv[1], 1.0);
        assert_eq!(adv[0], 2.0);
    }

    /// The backward pass must agree with finite differences, or nothing else
    /// in this file means anything.
    #[test]
    fn backward_matches_finite_differences() {
        let mut rng = Rng::new(7);
        let mut net = Mlp::new(3, 8, 2, 1.0, &mut rng);
        let x = [0.4_f32, -0.2, 0.9];

        // Loss = sum(out), so dL/dout is all ones and dL/dw is the gradient.
        net.forward(&x);
        net.zero_grad();
        net.backward(&x, &[1.0, 1.0]);
        let analytic = net.l2.gw[0];

        let eps = 1e-3;
        let mut probe = net.clone();
        probe.l2.w[0] += eps;
        let up: f32 = probe.forward(&x).iter().sum();
        let mut probe = net.clone();
        probe.l2.w[0] -= eps;
        let down: f32 = probe.forward(&x).iter().sum();
        let numeric = (up - down) / (2.0 * eps);

        assert!(
            (analytic - numeric).abs() < 1e-2,
            "analytic {analytic} vs numeric {numeric}"
        );
    }
}
