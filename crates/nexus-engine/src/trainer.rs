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
    pub fall_rate_unmeasured: bool,
    pub steps_per_sec: f64,
    pub episode_len: f32,
    /// Lean of a handful of environments, for the contact sheet.
    pub leans: Vec<f32>,
}

pub enum Cmd {
    Stop,
    Pause,
    Resume,
}

/// Everything the UI reads. Cheap to clone under the lock.
#[derive(Default)]
pub struct Shared {
    pub samples: Vec<Sample>,
    pub running: bool,
    pub envs: usize,
    pub total_steps: u64,
    pub paused: bool,
    pub error: Option<String>,
}

pub struct Handle {
    pub shared: Arc<Mutex<Shared>>,
    tx: Sender<Cmd>,
    pub can_pause: bool,
}

impl Handle {
    pub fn stop(&self) {
        let _ = self.tx.send(Cmd::Stop);
    }

    pub fn pause(&self) -> Result<(), String> {
        if !self.can_pause {
            return Err(
                "This backend cannot pause; Stop ends the job and preserves its checkpoints."
                    .into(),
            );
        }
        self.tx.send(Cmd::Pause).map_err(|e| e.to_string())
    }
    pub fn resume(&self) -> Result<(), String> {
        if !self.can_pause {
            return Err("This backend cannot resume in memory.".into());
        }
        self.tx.send(Cmd::Resume).map_err(|e| e.to_string())
    }

    /// Wrap a run this module did not start. The zealot backend produces the
    /// same `Shared` from a subprocess, and every screen stays a consumer of
    /// the stream rather than of whoever fills it.
    pub fn external(shared: Arc<Mutex<Shared>>, tx: Sender<Cmd>) -> Self {
        Self {
            shared,
            tx,
            can_pause: false,
        }
    }
}

/// Start a run. `envs` is the population, `total_steps` the budget in env-steps.
pub fn spawn(envs: usize, total_steps: u64, seed: u64) -> Handle {
    spawn_with(envs, total_steps, seed, true)
}

/// `randomize` off trains at nominal physics — the control case for the
/// robustness sweep.
pub fn spawn_with(envs: usize, total_steps: u64, seed: u64, randomize: bool) -> Handle {
    spawn_named(envs, total_steps, seed, randomize, RUN_ID)
}

/// Isolated output per run. The console supplies a unique run id.
pub fn spawn_named(
    envs: usize,
    total_steps: u64,
    seed: u64,
    randomize: bool,
    run_id: &str,
) -> Handle {
    spawn_from(envs, total_steps, seed, randomize, run_id, None)
}

pub fn spawn_from(
    envs: usize,
    total_steps: u64,
    seed: u64,
    randomize: bool,
    run_id: &str,
    checkpoint: Option<std::path::PathBuf>,
) -> Handle {
    let shared = Arc::new(Mutex::new(Shared {
        samples: Vec::new(),
        running: true,
        envs,
        total_steps,
        ..Shared::default()
    }));
    let (tx, rx) = mpsc::channel();
    let s = Arc::clone(&shared);
    let run_id = run_id.to_string();
    thread::spawn(move || {
        if envs == 0 || total_steps == 0 || !ckpt::valid_id(&run_id) {
            let mut g = s.lock().unwrap();
            g.error =
                Some("envs and step budget must be positive; run id must be a filename".into());
            g.running = false;
            return;
        }
        run(
            envs,
            total_steps,
            seed,
            s,
            rx,
            randomize,
            &run_id,
            checkpoint.as_deref(),
        )
    });
    Handle {
        shared,
        tx,
        can_pause: true,
    }
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
    run_id: &str,
    checkpoint: Option<&std::path::Path>,
) {
    let mut rng = Rng::new(seed);
    // Train across the distribution rather than at one point, which is what
    // gives the robustness sweep anything to find.
    let dist = if randomize {
        Randomization::TRAIN
    } else {
        Randomization::NONE
    };
    let mut env = VecEnv::with_randomization(n_envs, seed ^ 0x5DEE_CE66, dist);
    let mut ppo = Ppo::new(N_OBS, N_ACT, 64, Config::default(), &mut rng);
    if let Some(path) = checkpoint {
        let result = ckpt::read(path).and_then(|(weights, manifest)| {
            if manifest.n_obs != N_OBS || manifest.n_act != N_ACT || !ppo.load_weights(&weights) {
                return Err(std::io::Error::other(
                    "CPU warm-start checkpoint shape mismatch",
                ));
            }
            let input = ckpt::run_dir(run_id).join("input");
            std::fs::create_dir_all(&input)?;
            std::fs::copy(path, input.join("warm.bin"))?;
            std::fs::copy(path.with_extension("txt"), input.join("warm.txt"))?;
            Ok(())
        });
        if let Err(e) = result {
            if let Ok(mut g) = shared.lock() {
                g.error = Some(format!("Warm start failed: {e}"));
                g.running = false;
            }
            return;
        }
    }

    let mut obs: Vec<Vec<f32>> = vec![vec![0.0; N_OBS]; n_envs];
    for i in 0..n_envs {
        env.observe(i, &mut obs[i]);
    }

    let mut env_steps: u64 = 0;
    let mut next_ckpt: u64 = CKPT_EVERY;
    let started = Instant::now();
    let mut ep_return = vec![0.0_f32; n_envs];
    let mut ep_len = vec![0u32; n_envs];

    let mut paused = false;
    while env_steps < total_steps {
        let cmd = if paused {
            rx.recv().map_err(|_| TryRecvError::Disconnected)
        } else {
            rx.try_recv()
        };
        match cmd {
            Ok(Cmd::Stop) | Err(TryRecvError::Disconnected) => break,
            Ok(Cmd::Pause) => paused = true,
            Ok(Cmd::Resume) => paused = false,
            Err(TryRecvError::Empty) => {}
        }
        if let Ok(mut g) = shared.lock() {
            g.paused = paused;
        }
        if paused {
            continue;
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
        let var =
            adv_all.iter().map(|a| (a - mean) * (a - mean)).sum::<f32>() / adv_all.len() as f32;
        let sd = var.sqrt().max(1e-6);
        batch.adv = adv_all.iter().map(|a| (a - mean) / sd).collect();

        ppo.update(&batch, &mut rng);

        // ---- publish ------------------------------------------------------
        let samples = (HORIZON * n_envs) as f32;
        let sample = Sample {
            fall_rate_unmeasured: ends == 0,
            step: env_steps,
            reward: reward_acc / samples,
            terms: term_acc.iter().map(|t| t / samples).collect(),
            fall_rate: if ends > 0 {
                falls as f32 / ends as f32
            } else {
                0.0
            },
            steps_per_sec: env_steps as f64 / started.elapsed().as_secs_f64().max(1e-6),
            episode_len: if ends > 0 {
                len_acc as f32 / ends as f32
            } else {
                0.0
            },
            leans: env.leans().into_iter().take(16).collect(),
        };

        if !sample.reward.is_finite() || ppo.to_weights().iter().any(|v| !v.is_finite()) {
            if let Ok(mut g) = shared.lock() {
                g.error = Some("non-finite learner state; checkpoint refused".into());
            }
            break;
        }
        if env_steps >= next_ckpt {
            next_ckpt = env_steps + CKPT_EVERY;
            if let Err(e) = save_checkpoint(run_id, seed, env_steps, sample.reward, &ppo) {
                if let Ok(mut g) = shared.lock() {
                    g.error = Some(format!("checkpoint failed: {e}"));
                }
                break;
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

    // Short and stopped runs also leave a usable policy, with its measured
    // current score (the historical best is not the score of these weights).
    let final_score = shared
        .lock()
        .ok()
        .and_then(|g| g.samples.last().map(|s| s.reward));
    if let Some(score) = final_score {
        let healthy = shared.lock().map(|g| g.error.is_none()).unwrap_or(false);
        if healthy {
            if let Err(e) = save_checkpoint(run_id, seed, env_steps, score, &ppo) {
                if let Ok(mut g) = shared.lock() {
                    g.error = Some(format!("final checkpoint failed: {e}"));
                }
            }
        }
    }
    if let Ok(mut g) = shared.lock() {
        g.running = false;
        g.paused = false;
    }
}

fn save_checkpoint(run: &str, seed: u64, step: u64, score: f32, ppo: &Ppo) -> std::io::Result<()> {
    let manifest = ckpt::Manifest {
        run: run.into(),
        scene: "balance + velocity tracking (CPU cart-pole)".into(),
        seed,
        step,
        score: score as f64,
        n_obs: N_OBS,
        n_act: N_ACT,
        hidden: 64,
        terms: TERM_NAMES
            .iter()
            .zip(TERM_WEIGHTS.iter())
            .map(|(n, w)| (n.to_string(), *w as f64))
            .collect(),
    };
    ckpt::write(
        run,
        &format!("ckpt-{step:012}"),
        &ppo.to_weights(),
        &manifest,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn until(h: &Handle, condition: impl Fn(&Shared) -> bool) {
        let deadline = Instant::now() + std::time::Duration::from_secs(8);
        loop {
            if condition(&h.shared.lock().unwrap()) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "trainer did not reach expected state"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
    #[test]
    fn cpu_warm_start_copies_source_and_trains_in_new_directory() {
        let source = format!("warm-source-{}", std::process::id());
        let target = format!("warm-target-{}", std::process::id());
        let h = spawn_named(4, 128, 7, true, &source);
        until(&h, |s| !s.running);
        let checkpoint = ckpt::list(&source)[0].0.clone();
        let original = std::fs::read(&checkpoint).unwrap();
        let h = spawn_from(4, 128, 8, true, &target, Some(checkpoint.clone()));
        until(&h, |s| !s.running);
        assert!(h.shared.lock().unwrap().error.is_none());
        assert_eq!(
            std::fs::read(ckpt::run_dir(&target).join("input/warm.bin")).unwrap(),
            original
        );
        assert_eq!(std::fs::read(checkpoint).unwrap(), original);
        assert_eq!(ckpt::list(&target).len(), 1);
        assert_eq!(ckpt::list(&target)[0].1.step, 128);
        std::fs::remove_dir_all(ckpt::run_dir(&source)).unwrap();
        std::fs::remove_dir_all(ckpt::run_dir(&target)).unwrap();
    }

    #[test]
    fn short_runs_save_final_policies_in_separate_directories() {
        let a = format!("review-short-a-{}", std::process::id());
        let b = format!("review-short-b-{}", std::process::id());
        let ha = spawn_named(4, 128, 17, true, &a);
        let hb = spawn_named(4, 128, 23, true, &b);
        until(&ha, |s| !s.running);
        until(&hb, |s| !s.running);
        assert!(ha.shared.lock().unwrap().error.is_none());
        let ca = ckpt::list(&a);
        let cb = ckpt::list(&b);
        assert_eq!(ca.len(), 1);
        assert_eq!(cb.len(), 1);
        assert_eq!(ca[0].1.seed, 17);
        assert_eq!(cb[0].1.seed, 23);
        assert_ne!(ca[0].0, cb[0].0);
        assert!(crate::probe::Probe::load_latest(&a).is_some());
        std::fs::remove_dir_all(ckpt::run_dir(&a)).unwrap();
        std::fs::remove_dir_all(ckpt::run_dir(&b)).unwrap();
    }
    #[test]
    fn pause_preserves_state_resume_advances_and_stop_finishes() {
        let run = format!("review-pause-{}", std::process::id());
        let h = spawn_named(4, 1_000_000, 1, false, &run);
        h.pause().unwrap();
        until(&h, |s| s.paused);
        let before = h.shared.lock().unwrap().samples.len();
        std::thread::sleep(std::time::Duration::from_millis(60));
        assert_eq!(h.shared.lock().unwrap().samples.len(), before);
        h.resume().unwrap();
        until(&h, |s| s.samples.len() > before);
        h.stop();
        until(&h, |s| !s.running);
        assert!(!ckpt::list(&run).is_empty());
        std::fs::remove_dir_all(ckpt::run_dir(&run)).unwrap();
    }
    #[test]
    fn invalid_population_finishes_with_error_instead_of_hanging() {
        let h = spawn_named(0, 100, 1, false, "review-invalid");
        until(&h, |s| !s.running);
        assert!(h.shared.lock().unwrap().error.is_some());
    }
}
