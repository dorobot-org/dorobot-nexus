//! The training loop, on its own thread.
//!
//! The UI is a consumer. It never calls into the learner; it reads snapshots
//! off a channel. That is the boundary the design asks for — "the trainer is a
//! job behind a metric stream, so it can be swapped without touching a screen"
//! — and it is what will let a GPU backend replace this one later.

use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Instant;

use crate::ckpt;
use crate::env::{Randomization, VecEnv, N_ACT, N_OBS, TERM_NAMES, TERM_WEIGHTS};
use crate::rl::{gae, Batch, Config, Ppo};
use crate::rng::Rng;

/// One logged interval, as the UI consumes it.
#[derive(Clone, Debug, Default)]
pub struct Sample {
    pub step: u64,
    pub reward: f32,
    /// Mean contribution of each named term this interval.
    pub terms: Vec<f32>,
    pub fall_rate: f32,
    pub steps_per_sec: f64,
    pub episode_len: f32,
    /// Lean of a handful of environments, for the contact sheet.
    pub leans: Vec<f32>,
}

pub enum Cmd {
    Stop,
}

/// Everything the UI reads. Cheap to clone under the lock.
#[derive(Default)]
pub struct Shared {
    pub samples: Vec<Sample>,
    pub running: bool,
    pub envs: usize,
    pub total_steps: u64,
}

pub struct Handle {
    pub shared: Arc<Mutex<Shared>>,
    tx: Sender<Cmd>,
}

impl Handle {
    pub fn stop(&self) {
        let _ = self.tx.send(Cmd::Stop);
    }
}

/// Start a run. `envs` is the population, `total_steps` the budget in env-steps.
pub fn spawn(envs: usize, total_steps: u64, seed: u64) -> Handle {
    spawn_with(envs, total_steps, seed, true)
}

/// `randomize` off trains at nominal physics — the control case for the
/// robustness sweep.
pub fn spawn_with(envs: usize, total_steps: u64, seed: u64, randomize: bool) -> Handle {
    let shared = Arc::new(Mutex::new(Shared {
        samples: Vec::new(),
        running: true,
        envs,
        total_steps,
    }));
    let (tx, rx) = mpsc::channel();
    let s = Arc::clone(&shared);
    thread::spawn(move || run(envs, total_steps, seed, s, rx, randomize));
    Handle { shared, tx }
}

const HORIZON: usize = 32;
/// Write a checkpoint every this many env-steps. Frequent enough that a run you
/// kill at minute ten still leaves something promotable behind.
const CKPT_EVERY: u64 = 250_000;

pub const RUN_ID: &str = "balance-track-01";

fn run(
    n_envs: usize,
    total_steps: u64,
    seed: u64,
    shared: Arc<Mutex<Shared>>,
    rx: Receiver<Cmd>,
    randomize: bool,
) {
    let mut rng = Rng::new(seed);
    // Train across the distribution rather than at one point, which is what
    // gives the robustness sweep anything to find.
    let dist = if randomize { Randomization::TRAIN } else { Randomization::NONE };
    let mut env = VecEnv::with_randomization(n_envs, seed ^ 0x5DEE_CE66, dist);
    let mut ppo = Ppo::new(N_OBS, N_ACT, 64, Config::default(), &mut rng);

    let mut obs: Vec<Vec<f32>> = vec![vec![0.0; N_OBS]; n_envs];
    for i in 0..n_envs {
        env.observe(i, &mut obs[i]);
    }

    let mut env_steps: u64 = 0;
    let mut next_ckpt: u64 = CKPT_EVERY;
    let mut best = f32::MIN;
    let started = Instant::now();
    let mut ep_return = vec![0.0_f32; n_envs];
    let mut ep_len = vec![0u32; n_envs];

    while env_steps < total_steps {
        match rx.try_recv() {
            Ok(Cmd::Stop) | Err(TryRecvError::Disconnected) => break,
            Err(TryRecvError::Empty) => {}
        }

        // ---- collect ------------------------------------------------------
        let mut roll_obs = vec![Vec::with_capacity(HORIZON); n_envs];
        let mut roll_act = vec![Vec::with_capacity(HORIZON); n_envs];
        let mut roll_logp = vec![Vec::with_capacity(HORIZON); n_envs];
        let mut roll_val = vec![Vec::with_capacity(HORIZON + 1); n_envs];
        let mut roll_rew = vec![Vec::with_capacity(HORIZON); n_envs];
        let mut roll_done = vec![Vec::with_capacity(HORIZON); n_envs];

        let mut term_acc = vec![0.0_f32; TERM_NAMES.len()];
        let mut reward_acc = 0.0_f32;
        let mut falls = 0u32;
        let mut ends = 0u32;
        let mut len_acc = 0u32;

        for _ in 0..HORIZON {
            let mut actions = vec![vec![0.0; N_ACT]; n_envs];
            for i in 0..n_envs {
                let (logp, value) = ppo.act(&obs[i], &mut rng, &mut actions[i]);
                roll_obs[i].push(obs[i].clone());
                roll_act[i].push(actions[i].clone());
                roll_logp[i].push(logp);
                roll_val[i].push(value);
            }

            let out = env.step(&actions);
            for i in 0..n_envs {
                roll_rew[i].push(out.reward[i]);
                roll_done[i].push(out.done[i]);
                reward_acc += out.reward[i];
                ep_return[i] += out.reward[i];
                ep_len[i] += 1;
                if out.fell[i] {
                    falls += 1;
                }
                if out.done[i] {
                    ends += 1;
                    len_acc += ep_len[i];
                    ep_return[i] = 0.0;
                    ep_len[i] = 0;
                }
                env.observe(i, &mut obs[i]);
            }
            for (t, acc) in term_acc.iter_mut().enumerate() {
                *acc += out.terms[t].iter().sum::<f32>();
            }
            env_steps += n_envs as u64;
        }

        // Bootstrap value for the state the rollout ended in.
        for i in 0..n_envs {
            let v = ppo.value(&obs[i]);
            roll_val[i].push(v);
        }

        // ---- advantages ---------------------------------------------------
        let mut batch = Batch::default();
        let mut adv_all: Vec<f32> = Vec::with_capacity(n_envs * HORIZON);
        for i in 0..n_envs {
            let mut adv = vec![0.0; HORIZON];
            gae(
                &roll_rew[i],
                &roll_val[i],
                &roll_done[i],
                ppo.cfg.gamma,
                ppo.cfg.lam,
                &mut adv,
            );
            for t in 0..HORIZON {
                batch.obs.push(roll_obs[i][t].clone());
                batch.act.push(roll_act[i][t].clone());
                batch.logp.push(roll_logp[i][t]);
                batch.ret.push(adv[t] + roll_val[i][t]);
                adv_all.push(adv[t]);
            }
        }
        // Normalising advantages is what keeps the policy loss scale stable.
        let mean = adv_all.iter().sum::<f32>() / adv_all.len() as f32;
        let var = adv_all.iter().map(|a| (a - mean) * (a - mean)).sum::<f32>()
            / adv_all.len() as f32;
        let sd = var.sqrt().max(1e-6);
        batch.adv = adv_all.iter().map(|a| (a - mean) / sd).collect();

        ppo.update(&batch, &mut rng);

        // ---- publish ------------------------------------------------------
        let samples = (HORIZON * n_envs) as f32;
        let sample = Sample {
            step: env_steps,
            reward: reward_acc / samples,
            terms: term_acc.iter().map(|t| t / samples).collect(),
            fall_rate: if ends > 0 { falls as f32 / ends as f32 } else { 0.0 },
            steps_per_sec: env_steps as f64 / started.elapsed().as_secs_f64().max(1e-6),
            episode_len: if ends > 0 { len_acc as f32 / ends as f32 } else { 0.0 },
            leans: env.leans().into_iter().take(16).collect(),
        };
        best = best.max(sample.reward);

        // A checkpoint carries what produced it (Law 05), so a blob on disk is
        // always traceable back to a scene, a seed and a reward.
        if env_steps >= next_ckpt {
            next_ckpt += CKPT_EVERY;
            let manifest = ckpt::Manifest {
                run: RUN_ID.into(),
                scene: "balance + velocity tracking".into(),
                seed,
                step: env_steps,
                score: best as f64,
                n_obs: N_OBS,
                n_act: N_ACT,
                hidden: 64,
                terms: TERM_NAMES
                    .iter()
                    .zip(TERM_WEIGHTS.iter())
                    .map(|(n, w)| (n.to_string(), *w as f64))
                    .collect(),
            };
            let name = format!("ckpt-{:04}k", env_steps / 1000);
            if let Err(e) = ckpt::write(RUN_ID, &name, &ppo.to_weights(), &manifest) {
                eprintln!("checkpoint {name} failed: {e}");
            }
        }

        if let Ok(mut g) = shared.lock() {
            g.samples.push(sample);
            // The UI only ever plots a window; unbounded history would grow
            // without anyone looking at it.
            if g.samples.len() > 600 {
                g.samples.drain(0..100);
            }
        }
    }

    if let Ok(mut g) = shared.lock() {
        g.running = false;
    }
}
