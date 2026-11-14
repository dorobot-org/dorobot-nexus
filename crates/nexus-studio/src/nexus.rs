//! The engine wiring.
//!
//! This module re-exports the engine's dependency-free `scene` module, so
//! scenes and recordings edited here are the same files the trainer reads. It
//! also lists real checkpoints, loads real recorded rollouts, and starts real
//! training, sweeps and MuJoCo sim-to-sim.
//!
//! All of that runs **in this process**. `nexus-engine` is a linked crate, not
//! a binary to find: there is one build of the physics and the console cannot
//! end up looking at another. The one thing still spawned is the MuJoCo
//! harness, which is Python and belongs to a different runtime — and even that
//! is spawned by the engine, not from here.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// dorobot-nexus's artifact-schema library (scenes, recordings) — the
/// dependency-free lib target it exposes for exactly this purpose.
pub use nexus_engine::scene;

/// The dorobot-nexus checkout this studio operates on — **its own workspace
/// root**. Override with DOROBOT_NEXUS_DIR.
///
/// This used to default to a sibling clone (`~/home/dorobot-nexus`), from when
/// the console and the engine lived in separate repositories. They do not any
/// more: the engine is `nexus-engine`, linked into this binary, and the data it
/// reads — `scenes/`, `recordings/`, `data/g1/g1.urdf`, checkpoints,
/// `zealot-stack/`, `mujoco-stack/` — sits at the root of *this* workspace.
/// Left pointing at the sibling, every real artifact came back missing and the
/// 3D robot never loaded, which reads as "this app has no data" rather than as
/// a stale path.
pub fn repo_dir() -> String {
    if let Ok(d) = std::env::var("DOROBOT_NEXUS_DIR") {
        return d;
    }
    // Walk up from the working directory: `cargo run` starts at the workspace
    // root, a launched binary may start anywhere beneath it. The marker is the
    // engine's own manifest, so this cannot latch onto an unrelated checkout.
    if let Ok(cwd) = std::env::current_dir() {
        let mut d: Option<&std::path::Path> = Some(cwd.as_path());
        while let Some(p) = d {
            if p.join("crates/nexus-engine/Cargo.toml").is_file() {
                return p.to_string_lossy().into_owned();
            }
            d = p.parent();
        }
    }
    // Compiled in: correct for any binary built from this checkout, wherever it
    // is run from. crates/nexus-studio → crates → workspace root.
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Point the schema library at the real scene library before anything reads it.
///
/// Also anchors the working directory. The engine resolves `zealot-stack/` and
/// `mujoco-stack/` relative to it, so without this the sim-to-sim arm is
/// reported as "not installed" whenever the console is launched from anywhere
/// but the workspace root — a false negative that looks exactly like a missing
/// dependency.
pub fn init_env() {
    let repo = repo_dir();
    std::env::set_var("DOROBOT_SCENES_DIR", format!("{repo}/scenes"));
    let _ = std::env::set_current_dir(&repo);
}

// ------------------------------------------------------------- mapping --

use crate::state::Scene as UiScene;

/// Real on-disk scene → studio scene. `stance` is zealot's crouch target
/// (`base_height`); `dur` is `seconds`; gains keep their zealot names.
pub fn to_ui(s: &scene::Scene) -> UiScene {
    UiScene {
        id: format!("disk:{}", s.name),
        name: s.name.clone(),
        terrain: s.terrain.clone(),
        stance: s.base_height as f64,
        vx: s.vx as f64,
        dur: s.seconds as f64,
        friction: s.friction as f64,
        kp: s.kp_scale as f64,
        mass: s.mass_dr as f64,
        push: s.push_vel as f64,
        amp: s.terrain_amp as f64,
        slope: s.terrain_slope_deg as f64,
        seed: format!("0x{:X}", s.seed),
        force_fam: false,
        spawn_dr: s.spawn_dr,
    }
}

pub fn from_ui(u: &UiScene) -> scene::Scene {
    let mut s = scene::Scene::default();
    s.name = u.name.clone();
    s.terrain = u.terrain.clone();
    s.base_height = u.stance as f32;
    s.vx = u.vx as f32;
    s.seconds = u.dur as f32;
    s.friction = u.friction as f32;
    s.kp_scale = u.kp as f32;
    s.mass_dr = u.mass as f32;
    s.push_vel = u.push as f32;
    s.terrain_amp = u.amp as f32;
    s.terrain_slope_deg = u.slope as f32;
    s.spawn_dr = u.spawn_dr;
    s.seed = u64::from_str_radix(u.seed.trim_start_matches("0x"), 16).unwrap_or(0xC0FFEE);
    s
}

pub fn is_disk_scene(id: &str) -> bool {
    id.starts_with("disk:")
}

/// Persist a studio scene to the real library. Returns the saved path.
pub fn save_scene(u: &UiScene) -> std::io::Result<std::path::PathBuf> {
    from_ui(u).save()
}

pub fn delete_scene(u: &UiScene) -> std::io::Result<()> {
    match std::fs::remove_file(from_ui(u).path()) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

// ---------------------------------------------------------- checkpoints --

#[derive(Clone, Debug)]
pub struct DiskCkpt {
    pub label: String,
    pub path: String,
    pub kb: u64,
}

/// Real checkpoint files in the dorobot-nexus repo, newest first.
pub fn real_ckpts() -> Vec<DiskCkpt> {
    checkpoints_in(std::path::Path::new(&repo_dir()))
}

fn checkpoints_in(repo: &std::path::Path) -> Vec<DiskCkpt> {
    let mut out = vec![];
    if let Ok(rd) = std::fs::read_dir(repo) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with("dorobot_nexus.safetensors") || name == "curriculum.safetensors" {
                let kb = e.metadata().map(|m| m.len() / 1024).unwrap_or(0);
                out.push(DiskCkpt {
                    label: name.clone(),
                    path: repo.join(&name).to_string_lossy().into_owned(),
                    kb,
                });
            }
        }
    }
    out.sort_by(|a, b| b.label.cmp(&a.label));
    out
}

/// FNV-1a-32 of a real artifact's bytes — the honest export fingerprint.
pub fn hash_file(path: &str) -> Option<(String, u64)> {
    let bytes = std::fs::read(path).ok()?;
    let mut h: u32 = 2166136261;
    for b in &bytes {
        h ^= *b as u32;
        h = h.wrapping_mul(16777619);
    }
    Some((format!("{h:08x}"), bytes.len() as u64 / 1024))
}

// ------------------------------------------------------------- rollouts --

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Replay {
    #[serde(default)]
    pub name: String,
    pub joint_names: Vec<String>,
    pub joints: Vec<Vec<f32>>,
    /// Base pose per frame: xyz + quaternion (x,y,z,w). Without it a
    /// trajectory replays with the robot marching in place at the origin,
    /// however far it actually travelled — and never on the terrain.
    pub base: Vec<[f32; 7]>,
    pub dt: f64,
}

/// Minimal hand parser for the rollout JSON dorobot-nexus writes
/// (flat schema: dt, joint_names, resets, base, joints). No serde needed.
pub fn load_rollout(path: &std::path::Path, name: &str) -> Option<Replay> {
    #[derive(serde::Deserialize)]
    struct Data {
        dt: f64,
        joint_names: Vec<String>,
        joints: Vec<Vec<f32>>,
        #[serde(default)]
        base: Vec<[f32; 7]>,
    }
    let d: Data = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    if !d.dt.is_finite()
        || d.dt <= 0.0
        || d.joint_names.is_empty()
        || d.joint_names.iter().any(|n| n.is_empty())
        || d.joint_names
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len()
            != d.joint_names.len()
        || d.joints.is_empty()
        || d.joints
            .iter()
            .any(|r| r.len() != d.joint_names.len() || r.iter().any(|v| !v.is_finite()))
        || (!d.base.is_empty() && d.base.len() != d.joints.len())
        || d.base.iter().any(|b| {
            b.iter().any(|v| !v.is_finite())
                || (b[3..].iter().map(|q| q * q).sum::<f32>() - 1.0).abs() > 0.01
        })
    {
        return None;
    }
    Some(Replay {
        name: name.into(),
        dt: d.dt,
        joint_names: d.joint_names,
        joints: d.joints,
        base: d.base,
    })
}

// ------------------------------------------------------------ real work --
//
// These used to spawn a `dorobot-nexus` binary from a sibling checkout and
// scrape its printed output. There is no such binary and no such checkout: the
// engine is `nexus-engine`, linked into this process. Everything below starts
// an engine thread instead and reads the state it publishes — the same state
// `--headless` and `--sweep` print, with no text format to scrape, no second
// process to keep alive, and no way for the console to be looking at a
// different build of the physics than the one it links.

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ProcKind {
    Train,
    Sweep,
}

/// A real engine run, in flight, in this process.
pub struct RealProc {
    pub kind: ProcKind,
    pub run_id: Option<String>,
    pub artifact: Option<String>,
    pub backend: String,
    /// Human progress lines for the right rail's activity tail. Derived from
    /// the published state rather than being its source — nothing parses these.
    pub rx: Receiver<String>,
    pub(crate) done: Arc<AtomicBool>,
    pub job_update: Option<Arc<Mutex<crate::jobs::Update>>>,
    pub(crate) surface: Option<Arc<Mutex<nexus_engine::sweep::Surface>>>,
    /// The live metric stream of a Train job — the source `drain_real_proc`
    /// folds into `st.live`, where every screen reads it.
    pub train: Option<nexus_engine::trainer::Handle>,
}

impl RealProc {
    pub fn finished(&self) -> bool {
        self.done.load(Ordering::Relaxed)
    }

    /// The sweep surface and how many cells it has measured.
    ///
    /// Returned in the store's own grid shape rather than a fresh one: the
    /// engine's surface is 5 × 8 and so is `Store::sweep_grid`, so the real
    /// result lands in the field every existing consumer already reads — cell
    /// selection, the pass percentage, the heat map, "open in Inspect". A
    /// parallel grid beside it was how the screen ended up with two surfaces
    /// and a branch deciding which was showing.
    pub fn grid(&self) -> Option<([[Option<f64>; 8]; 5], usize)> {
        use nexus_engine::sweep::{COLS, ROWS};
        let s = self.surface.as_ref()?.lock().ok()?;
        let mut g = [[None; COLS]; ROWS];
        for (r, row) in g.iter_mut().enumerate() {
            for (c, cell) in row.iter_mut().enumerate() {
                *cell = s.cell(r, c).map(f64::from);
            }
        }
        Some((g, s.done))
    }

    /// What the surface says it varied, for the axis caption.
    pub fn axes(&self) -> Option<String> {
        let s = self.surface.as_ref()?.lock().ok()?;
        (!s.axes.is_empty()).then(|| s.axes.clone())
    }

    /// Ask a training run to stop at the next interval.
    pub fn stop(&self) {
        if let Some(h) = &self.train {
            h.stop();
        }
        if let Some(surface) = &self.surface {
            if let Ok(mut s) = surface.lock() {
                s.stop_requested = true;
            }
        }
    }
}

/// Artifact selected by the operator; validators never silently switch policy
/// or substitute a different robot when the requested backend is unavailable.
pub fn selected_artifact(st: &crate::state::Store) -> Option<String> {
    if let Some(selected) = &st.sel.ck {
        let run = st.sel.run.as_ref().and_then(|id| st.run(id))?;
        return run
            .ckpts
            .iter()
            .find(|c| &c.id == selected || &c.label == selected)
            .filter(|c| std::path::Path::new(&c.id).is_file())
            .map(|c| c.id.clone());
    }
    if let Some(id) = &st.sel.run {
        let run = st.run(id)?;
        if let Some(ck) = run
            .ckpts
            .iter()
            .filter(|c| std::path::Path::new(&c.id).is_file())
            .max_by_key(|c| std::fs::metadata(&c.id).and_then(|m| m.modified()).ok())
        {
            return Some(ck.id.clone());
        }
        if !matches!(
            run.state,
            crate::state::RunState::Running | crate::state::RunState::Paused
        ) {
            return None;
        }
        let gpu = nexus_engine::ckpt::run_dir(id).join("policy.safetensors");
        if gpu.is_file() {
            return Some(gpu.to_string_lossy().to_string());
        }
        return nexus_engine::ckpt::list(id)
            .first()
            .map(|(p, _)| p.to_string_lossy().to_string());
    }
    None
}

pub fn can_sweep(st: &crate::state::Store) -> bool {
    selected_artifact(st).is_some_and(|p| {
        p.ends_with(".bin")
            || (crate::artifacts::is_safetensors(&p)
                && nexus_engine::zealot::drive_path().is_file())
    })
}

pub fn run_sweep(checkpoint: &str) -> Result<RealProc, String> {
    let path = std::path::Path::new(checkpoint);
    let surface = if path.extension().is_some_and(|e| e == "bin") {
        nexus_engine::sweep::spawn_checkpoint(path)
    } else {
        #[cfg(feature = "zealot")]
        {
            nexus_engine::sweep::zealot_sweep::spawn(checkpoint)
        }
        #[cfg(not(feature = "zealot"))]
        {
            None
        }
    }
    .ok_or_else(|| "The selected checkpoint cannot be swept by this backend.".to_string())?;

    let (tx, rx) = channel();
    let done = Arc::new(AtomicBool::new(false));
    {
        let (s, fin) = (Arc::clone(&surface), Arc::clone(&done));
        std::thread::spawn(move || {
            let mut last = usize::MAX;
            loop {
                std::thread::sleep(Duration::from_millis(150));
                let Ok(g) = s.lock() else { break };
                let (running, cur, total) = (g.running, g.done, g.total);
                let (label, axes, collapsed) = (g.label.clone(), g.axes.clone(), g.collapsed);
                drop(g);
                if cur != last {
                    last = cur;
                    let _ = tx.send(format!("sweep {cur}/{total} · {label}"));
                }
                if !running {
                    if !axes.is_empty() {
                        let _ = tx.send(axes);
                    }
                    if collapsed > 0 {
                        let _ =
                            tx.send(format!("{collapsed} cells unmeasured — rollout collapsed"));
                    }
                    break;
                }
            }
            fin.store(true, Ordering::Relaxed);
        });
    }
    Ok(RealProc {
        kind: ProcKind::Sweep,
        run_id: None,
        artifact: None,
        backend: "sweep".into(),
        rx,
        done,
        job_update: None,
        surface: Some(surface),
        train: None,
    })
}

/// Start a real training run on the engine's own threads.
///
/// Makes the same backend choice `--headless` makes — zealot's GPU run when
/// that stack is built, the CPU trainer otherwise — at the same population, so
/// the env-step budget means the same thing whichever backend answers.
pub fn run_train(total_steps: u64, run: &crate::state::Run) -> Result<RealProc, String> {
    const ENVS: usize = 256;
    let dir = nexus_engine::ckpt::run_dir(&run.id);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    if std::env::var_os("NEXUS_SUPERVISED").is_none() {
        std::fs::write(
            dir.join("run.json"),
            serde_json::to_vec_pretty(run).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
    }
    let artifact = dir.join("policy.safetensors").to_string_lossy().to_string();
    let seed = u64::from_str_radix(run.snapshot.seed.trim_start_matches("0x"), 16).unwrap_or(1);
    let attached = std::env::var("DOROBOT_ATTACH_LOG").ok();
    let use_cpu = run.snapshot.backend.as_deref() == Some("cpu");
    let mut output_artifact = (!use_cpu).then_some(artifact.clone());
    let (h, backend) = if let Some(profile) = &run.snapshot.training_profile {
        let path = dir.join("training-profile.json");
        std::fs::write(
            &path,
            serde_json::to_vec_pretty(profile).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let (h, checkpoint) = start_external(&path.to_string_lossy(), &dir, run)?;
        output_artifact = Some(checkpoint);
        (h, "External robot RL trainer")
    } else if let Some(path) = attached {
        (
            nexus_engine::zealot::attach_log(&path)
                .ok_or_else(|| format!("cannot read attached log {path}"))?,
            "zealot · attached (read-only)",
        )
    } else if use_cpu {
        (
            nexus_engine::trainer::spawn_from(
                ENVS,
                run.snapshot
                    .additional_iterations
                    .map(|i| i as u64 * ENVS as u64 * 32)
                    .unwrap_or(total_steps),
                seed,
                true,
                &run.id,
                run.snapshot
                    .warm
                    .as_deref()
                    .filter(|p| *p != "Fresh")
                    .map(std::path::PathBuf::from),
            ),
            "CPU cart-pole · balance and tracking",
        )
    } else {
        if run
            .snapshot
            .robot
            .as_deref()
            .is_some_and(|r| r != "Unitree G1-29dof")
        {
            return Err("Select a matching training backend profile for this robot.".into());
        }
        if !nexus_engine::zealot::binary_path().is_file() {
            return Err("G1 trainer is not installed. Run scripts/setup-zealot.sh, or explicitly choose NEXUS_BACKEND=cpu for the cart-pole learner.".into());
        }
        // Fresh is the default. Warm starts name an existing artifact explicitly.
        if let Some(warm) = run.snapshot.warm.as_deref().filter(|s| *s != "Fresh") {
            if !std::path::Path::new(warm).is_file() {
                return Err(format!("warm-start checkpoint does not exist: {warm}"));
            }
            std::fs::copy(warm, &artifact).map_err(|e| e.to_string())?;
        }
        let scenes: Vec<_> = run
            .snapshot
            .cfg
            .iter()
            .map(|c| {
                let mut sc = c.full.as_ref().map(from_ui).unwrap_or_default();
                sc.name = c.name.clone();
                sc.base_height = c.stance as f32;
                sc.terrain = c.terrain.clone();
                sc.friction = c.friction as f32;
                sc.kp_scale = c.kp as f32;
                sc.mass_dr = c.mass as f32;
                sc.push_vel = c.push as f32;
                sc.seed = seed;
                (sc.name.clone(), sc.knobs())
            })
            .collect();
        let scenes = if scenes.is_empty() {
            vec![("baseline".into(), scene::Scene::default().knobs())]
        } else {
            scenes
        };
        let h = nexus_engine::zealot::spawn_curriculum_budgeted(
            scenes,
            ENVS,
            run.iters_per as u64,
            6,
            &artifact,
            run.snapshot.additional_iterations.map(u64::from),
        )
        .ok_or_else(|| "could not start G1 training".to_string())?;
        (h, "Zealot GPU · Unitree G1")
    };

    let (tx, rx) = channel();
    let done = Arc::new(AtomicBool::new(false));
    let (reported_envs, reported_steps) = h
        .shared
        .lock()
        .map(|g| (g.envs, g.total_steps))
        .unwrap_or_default();
    let _ = tx.send(format!(
        "training started · {backend} · {} envs · {} env-steps",
        if reported_envs > 0 {
            reported_envs
        } else {
            ENVS
        },
        if reported_steps > 0 {
            reported_steps
        } else {
            total_steps
        }
    ));
    {
        let (shared, fin) = (Arc::clone(&h.shared), Arc::clone(&done));
        std::thread::spawn(move || {
            let mut last_step = 0;
            loop {
                std::thread::sleep(Duration::from_millis(250));
                let Ok(g) = shared.lock() else { break };
                let running = g.running;
                let fresh: Vec<_> = g
                    .samples
                    .iter()
                    .filter(|s| s.step > last_step)
                    .cloned()
                    .collect();
                if let Some(s) = fresh.last() {
                    last_step = s.step;
                }
                drop(g);
                for s in fresh {
                    let _ = tx.send(format!(
                        "step {}k · reward {:.4} · falls {} · {:.0} steps/s",
                        s.step / 1000,
                        s.reward,
                        if s.fall_rate_unmeasured {
                            "not measured".into()
                        } else {
                            format!("{:.1}%", s.fall_rate * 100.0)
                        },
                        s.steps_per_sec
                    ));
                }
                if !running {
                    break;
                }
            }
            fin.store(true, Ordering::Relaxed);
        });
    }
    Ok(RealProc {
        kind: ProcKind::Train,
        run_id: Some(run.id.clone()),
        artifact: output_artifact,
        backend: backend.into(),
        rx,
        done,
        job_update: None,
        surface: None,
        train: Some(h),
    })
}

// ------------------------------------------------------ sim-to-sim (MuJoCo) --

/// A MuJoCo sim-to-sim run, in flight.
///
/// Held as a named struct rather than as bare fields on `App` for the same
/// reason [`nexus_engine::zealot::RolloutSlot`] exists: makepad's `Script`
/// derive parses a restricted type grammar and rejects nested generics.
pub struct MjRun {
    /// The verdict. `running` while the harness is up, then either `done` with
    /// scores or a `label` carrying the diagnosis.
    pub report: Arc<Mutex<nexus_engine::crosssim::Report>>,
    /// The motion MuJoCo simulated, from the same run.
    pub rollout: nexus_engine::zealot::RolloutSlot,
    /// The rollout is handed to the 3D view once; this remembers that.
    pub replayed: bool,
}

impl MjRun {
    pub fn running(&self) -> bool {
        self.report.lock().map(|r| r.running).unwrap_or(false)
    }

    /// Status line and the four measured rows, for the stage.
    pub fn view(&self) -> (String, bool, [(&'static str, String); 4]) {
        let blank = |_| ("", String::new());
        match self.report.lock() {
            Ok(r) => {
                let rows = r.rows();
                (
                    r.label.clone(),
                    r.done,
                    [
                        (rows[0].0, rows[0].2.clone()),
                        (rows[1].0, rows[1].2.clone()),
                        (rows[2].0, rows[2].2.clone()),
                        (rows[3].0, rows[3].2.clone()),
                    ],
                )
            }
            Err(_) => (
                String::new(),
                false,
                [blank(0), blank(1), blank(2), blank(3)],
            ),
        }
    }
}

/// Start MuJoCo sim-to-sim against `ckpt`, or say why it cannot run.
///
/// `command_vx` is the speed the policy is told to hold and is scored against;
/// it has to be the one it was given, which is why the caller sets it rather
/// than the harness inferring it.
pub fn run_mujoco(ckpt: &str, command_vx: f32, seconds: u32) -> Result<MjRun, String> {
    if let Some(why) = nexus_engine::mujoco::why_unavailable() {
        return Err(why);
    }
    let rollout = nexus_engine::zealot::RolloutSlot::default();
    let report =
        nexus_engine::crosssim::spawn_mujoco_into(ckpt, command_vx, seconds, rollout.clone())
            .ok_or_else(|| "the MuJoCo arm is not available".to_string())?;
    Ok(MjRun {
        report,
        rollout,
        replayed: false,
    })
}

// ------------------------------------------------------ cross-sim (same physics) --

/// One policy run in two implementations of the *same* physics.
///
/// The weaker of the two sim-to-sim checks, and the screen has to say so: both
/// columns share the dynamics function, so a disagreement means the answer
/// depends on how the equations were stepped. That is worth knowing and is not
/// the check that predicts transfer — only a different engine can catch a
/// modelling error, which is what the MuJoCo arm is for.
pub struct CrossRun {
    pub report: Arc<Mutex<nexus_engine::crosssim::Report>>,
    /// Which arm ran. The two arms compare different things, and a panel that
    /// did not say which was running is what this replaced.
    pub arm: &'static str,
    pub col_a: &'static str,
    pub col_b: &'static str,
}

impl CrossRun {
    pub fn running(&self) -> bool {
        self.report.lock().map(|r| r.running).unwrap_or(false)
    }

    /// Status line, done flag, worst gap, and the four measured rows as
    /// (metric, a, b, delta).
    pub fn view(
        &self,
    ) -> (
        String,
        bool,
        f32,
        Vec<(&'static str, String, String, String)>,
    ) {
        match self.report.lock() {
            Ok(r) => (
                r.label.clone(),
                r.done,
                r.worst_gap(),
                r.rows().iter().cloned().collect(),
            ),
            Err(_) => (String::new(), false, 0.0, Vec::new()),
        }
    }
}

/// Start the cross-sim arm, preferring the one that measures the real robot.
///
/// zealot's arm drives the G1 at two control decimations; the built-in arm
/// steps the cart-pole with Euler against Runge-Kutta. Both are honest, they
/// answer different questions, and which one ran is carried out with the
/// result rather than assumed by the screen.
pub fn run_cross(checkpoint: &str) -> Result<CrossRun, String> {
    let path = std::path::Path::new(checkpoint);
    if path.extension().is_some_and(|e| e == "bin") {
        let report = nexus_engine::crosssim::spawn_checkpoint(path)
            .ok_or("Selected CPU checkpoint cannot be loaded")?;
        return Ok(CrossRun {
            report,
            arm: "CPU cart-pole · integrator",
            col_a: "euler",
            col_b: "rk4",
        });
    }
    #[cfg(feature = "zealot")]
    if let Some(report) = nexus_engine::crosssim::zealot_cross::spawn(checkpoint) {
        return Ok(CrossRun {
            report,
            arm: "Zealot G1 · control decimation",
            col_a: "dec 4",
            col_b: "dec 1",
        });
    }
    Err("No cross-simulator adapter for the selected checkpoint.".into())
}

/// An engine rollout in the shape the 3D view replays.
pub fn replay_from(r: &nexus_engine::zealot::Rollout, name: &str) -> Replay {
    Replay {
        name: name.into(),
        joint_names: r.joint_names.clone(),
        joints: r.joints.clone(),
        base: r.base.clone(),
        dt: r.dt as f64,
    }
}

/// A Train-mode policy preview in flight: the checkpoint the run is
/// producing, rolled out on the scene the stage shows. The slot fills on a
/// worker thread because `zealot::drive` blocks for seconds.
pub struct PreviewJob {
    pub slot: nexus_engine::zealot::RolloutSlot,
    pub done: Arc<AtomicBool>,
}

/// Roll `ckpt` out on `scene`'s physics in the background.
///
/// `None` when the drive binary is not on this machine; the caller treats
/// that as "nothing to preview" rather than an error, the same stance the
/// sim-to-sim arms take.
pub fn spawn_preview(ckpt: &str, scene: &UiScene) -> Option<PreviewJob> {
    if !nexus_engine::zealot::drive_path().is_file() {
        return None;
    }
    let mut knobs = from_ui(scene).knobs();
    // Deterministic spawn: successive previews should differ by policy, not
    // by starting-state luck — the same override the sweep and cross-sim
    // make when they measure.
    knobs.push(("BIPED_SPAWN_DR", "0".into()));
    let ckpt = ckpt.to_string();
    let job = PreviewJob {
        slot: nexus_engine::zealot::RolloutSlot::default(),
        done: Arc::new(AtomicBool::new(false)),
    };
    let (slot, done) = (job.slot.clone(), Arc::clone(&job.done));
    std::thread::spawn(move || {
        // Long enough to read a gait, short enough that the preview keeps up
        // with the checkpoints as they are written.
        let cmd = nexus_engine::zealot::Drive {
            vx: 0.3,
            seconds: 6.0,
            ..Default::default()
        };
        slot.set(nexus_engine::zealot::drive(&ckpt, cmd, &knobs));
        done.store(true, Ordering::Relaxed);
    });
    Some(job)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_case(name: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!(
            "nexus-audit-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn selected_artifact_rejects_another_runs_checkpoint_and_missing_selection() {
        let dir = temp_case("artifact");
        let a = dir.join("a.pt");
        let b = dir.join("b.pt");
        std::fs::write(&a, b"a").unwrap();
        std::fs::write(&b, b"b").unwrap();
        let mut st = crate::state::Store::seed();
        st.runs[0].ckpts = vec![crate::state::Ckpt {
            score_known: true,
            id: a.to_string_lossy().into_owned(),
            label: "selected".into(),
            score: 1.0,
            st: crate::state::CkSt::Ord,
            pname: None,
        }];
        st.sel.run = Some(st.runs[0].id.clone());
        st.sel.ck = Some(b.to_string_lossy().into_owned());
        assert!(selected_artifact(&st).is_none());
        st.sel.ck = Some("selected".into());
        assert_eq!(selected_artifact(&st).unwrap(), a.to_string_lossy());
        std::fs::remove_file(&a).unwrap();
        assert!(selected_artifact(&st).is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn invalid_replay_joint_names_and_base_orientation_are_rejected() {
        let dir = temp_case("replay");
        let path = dir.join("replay.json");
        let valid = serde_json::json!({"dt":0.02,"joint_names":["joint"],"joints":[[0.1]],"base":[[0,0,0.1,0,0,0,1]]});
        std::fs::write(&path, valid.to_string()).unwrap();
        assert!(load_rollout(&path, "valid").is_some());
        let mut bad = valid.clone();
        bad["base"][0][6] = 0.into();
        std::fs::write(&path, bad.to_string()).unwrap();
        assert!(load_rollout(&path, "bad orientation").is_none());
        bad = valid;
        bad["joint_names"] = serde_json::json!(["joint", "joint"]);
        bad["joints"] = serde_json::json!([[0, 0]]);
        std::fs::write(&path, bad.to_string()).unwrap();
        assert!(load_rollout(&path, "duplicate names").is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn recordings_match_robot_joints_and_reject_ambiguous_legacy_identity() {
        let dir = temp_case("robot");
        let path = dir.join("robot.urdf");
        std::fs::write(&path, r#"<robot name="one"><link name="a"/><link name="b"/><joint name="joint" type="fixed"><parent link="a"/><child link="b"/></joint></robot>"#).unwrap();
        let mut robot = crate::state::Store::seed().robots[0].clone();
        robot.name = "one".into();
        robot.urdf = Some(path.to_string_lossy().into_owned());
        let mut other = robot.clone();
        other.name = "two".into();
        let robots = vec![robot, other];
        let replay = Replay {
            name: "test".into(),
            dt: 0.02,
            joint_names: vec!["joint".into()],
            joints: vec![vec![0.0]],
            base: vec![],
        };
        assert!(recording_robot(&robots, None, &replay).is_err());
        assert_eq!(
            recording_robot(&robots, Some("one"), &replay).unwrap().name,
            "one"
        );
        assert!(recording_robot(&robots, Some("missing"), &replay).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn maps_roundtrip() {
        let mut u = UiScene::base("s1", "roundtrip-test");
        u.stance = 0.62;
        u.dur = 8.0;
        u.kp = 1.2;
        let n = from_ui(&u);
        assert_eq!(n.base_height, 0.62_f32);
        assert_eq!(n.seconds, 8.0);
        let back = to_ui(&n);
        assert_eq!(back.name, "roundtrip-test");
        assert!((back.kp - 1.2).abs() < 1e-6);
    }

    /// The regression that broke everything downstream: `repo_dir()` defaulted
    /// to `~/home/dorobot-nexus`, a sibling checkout from when the engine was a
    /// separate repository. Once it was deleted, every real artifact came back
    /// missing — no scenes, no checkpoints, and no G1 URDF, so the console fell
    /// back to its schematic figure and looked like an app with no data.
    #[test]
    fn repo_dir_resolves_to_the_workspace_that_contains_the_engine() {
        let d = std::path::PathBuf::from(repo_dir());
        assert!(
            d.join("crates/nexus-engine/Cargo.toml").is_file(),
            "repo_dir() = {} — does not contain the engine",
            d.display()
        );
    }

    /// Note what this does *not* assert. The previous version required the
    /// library to contain `flat-easy`, which only ever existed in the sibling
    /// checkout — and it passed for as long as it did precisely because
    /// a checkout-existence guard was false and it never ran. A test that names artifacts
    /// this workspace does not ship is a test that only reports on its own
    /// skip condition.
    #[test]
    fn the_scene_library_resolves_to_this_workspace() {
        init_env();
        assert_eq!(
            std::env::var("DOROBOT_SCENES_DIR").unwrap(),
            format!("{}/scenes", repo_dir())
        );
        // No `.scene.json` files are committed here, so the engine synthesises
        // its baseline rather than returning nothing.
        assert!(!scene::list().is_empty());
    }

    #[test]
    fn loads_portable_rollout_without_local_recordings() {
        let dir = temp_case("rollout");
        let path = dir.join("rollout.json");
        std::fs::write(&path, r#"{"dt":0.02,"joint_names":["hip"],"joints":[[0.1],[0.2]],"base":[[0,0,0.8,0,0,0,1],[0,0,0.8,0,0,0,1]]}"#).unwrap();
        let rp = load_rollout(&path, "synthetic").expect("rollout parses");
        assert_eq!(rp.joints.len(), 2);
        assert_eq!(rp.joints[0].len(), rp.joint_names.len());
        assert_eq!(rp.joints[1], vec![0.2]);
        assert_eq!(rp.dt, 0.02);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn checkpoint_listing_uses_only_the_requested_directory() {
        let dir = temp_case("checkpoints");
        assert!(checkpoints_in(&dir).is_empty());
        std::fs::write(dir.join("dorobot_nexus.safetensors"), vec![0_u8; 2048]).unwrap();
        std::fs::write(dir.join("unrelated.txt"), b"ignore").unwrap();
        let c = checkpoints_in(&dir);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].label, "dorobot_nexus.safetensors");
        assert_eq!(c[0].kb, 2);
        assert_eq!(std::path::Path::new(&c[0].path), dir.join("dorobot_nexus.safetensors"));
        std::fs::remove_dir_all(dir).unwrap();
    }
}

/// Older workspaces store metric samples in the trainer's run directory.
pub fn load_run_metrics(run: &mut crate::state::Run) {
    use std::io::{BufRead, Read};
    if !run.metrics.is_empty() {
        return;
    }
    let path = nexus_engine::ckpt::run_dir(&run.id).join("metrics.jsonl");
    let Ok(file) = std::fs::File::open(path) else {
        return;
    };
    for line in std::io::BufReader::new(file.take(64 * 1024 * 1024))
        .lines()
        .map_while(Result::ok)
    {
        if let Ok(Some(sample)) = external_sample(&line) {
            run.metrics.push(crate::state::RunMetric {
                fall_rate_unmeasured: sample.fall_rate_unmeasured,
                step: sample.step,
                reward: sample.reward as f64,
                fall_rate: sample.fall_rate as f64,
                steps_per_sec: sample.steps_per_sec,
            });
        }
    }
    let excess = run.metrics.len().saturating_sub(1200);
    run.metrics.drain(..excess);
}

/// Infer legacy recordings only when all named joints belong to one robot.
pub fn recording_robot<'a>(
    robots: &'a [crate::state::Robot],
    explicit: Option<&str>,
    replay: &Replay,
) -> Result<&'a crate::state::Robot, String> {
    let matches: Vec<_> = robots
        .iter()
        .filter(|r| explicit.is_none_or(|name| r.model_name() == name || r.name == name))
        .filter(|r| {
            let Some(path) = &r.urdf else { return false };
            let path = std::path::Path::new(&repo_dir()).join(path);
            let Ok(model) = urdf_rs::read_file(path) else {
                return false;
            };
            replay
                .joint_names
                .iter()
                .all(|name| model.joints.iter().any(|j| &j.name == name))
        })
        .collect();
    match matches.as_slice() {
        [robot] => Ok(robot),
        [] => Err("Recording does not match an imported robot's joints.".into()),
        _ => Err("Recording robot is ambiguous; import it with robot provenance.".into()),
    }
}

/// Parse a trainer's JSON-lines metric contract; plain log lines are retained
/// in the log but do not become measurements.
pub(crate) fn external_sample(line: &str) -> Result<Option<nexus_engine::trainer::Sample>, String> {
    if !line.trim_start().starts_with('{') {
        return Ok(None);
    }
    let v: serde_json::Value =
        serde_json::from_str(line).map_err(|e| format!("malformed metric JSON: {e}"))?;
    if v["event"].as_str() == Some("error") {
        return Err(v["message"]
            .as_str()
            .unwrap_or("External trainer failed")
            .to_string());
    }
    if v["event"].as_str() != Some("sample") {
        return Ok(None);
    }
    let num = |key: &str| {
        v[key]
            .as_f64()
            .filter(|n| n.is_finite())
            .ok_or_else(|| format!("metric {key} must be finite"))
    };
    Ok(Some(nexus_engine::trainer::Sample {
        fall_rate_unmeasured: v["fall_rate_measured"].as_bool() == Some(false),
        step: v["step"]
            .as_u64()
            .ok_or("metric step must be an unsigned integer")?,
        reward: num("reward")? as f32,
        fall_rate: num("fall_rate")? as f32,
        steps_per_sec: num("steps_per_sec")?,
        episode_len: num("episode_len")? as f32,
        terms: Vec::new(),
        leans: Vec::new(),
    }))
}
#[derive(serde::Deserialize)]
pub struct ExternalRecipe {
    pub task: String,
    pub num_envs: usize,
    pub iterations: u32,
    #[serde(default = "default_rollout_steps")]
    pub steps_per_env: u32,
}
fn default_rollout_steps() -> u32 {
    24
}
impl ExternalRecipe {
    fn validate(&self) -> Result<(), String> {
        if self.task.trim().is_empty()
            || self.num_envs == 0
            || self.iterations == 0
            || self.steps_per_env == 0
            || (self.num_envs as u64)
                .checked_mul(self.iterations as u64)
                .and_then(|n| n.checked_mul(self.steps_per_env as u64))
                .is_none()
        {
            return Err(
                "External recipe requires a task and positive environment/iteration budgets".into(),
            );
        }
        Ok(())
    }
}

fn read_profile(path: &std::path::Path) -> Result<serde_json::Value, String> {
    let path = path.canonicalize().map_err(|e| e.to_string())?;
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    if let Some(cwd) = value["cwd"].as_str() {
        if !std::path::Path::new(cwd).is_absolute() {
            value["cwd"] = path
                .parent()
                .unwrap()
                .join(cwd)
                .to_string_lossy()
                .into_owned()
                .into();
        }
    }
    Ok(value)
}

/// Recover older runs from their saved recipe, never from today's registry.
pub fn restore_run_recipe(run: &mut crate::state::Run) -> Result<(), String> {
    if run.snapshot.backend.is_some() {
        return Ok(());
    }
    let path = nexus_engine::ckpt::run_dir(&run.id).join("training-profile.json");
    if path.is_file() {
        run.snapshot.training_profile = Some(read_profile(&path)?);
        run.snapshot.backend = Some("external".into());
    } else if run.ckpts.iter().any(|c| c.id.ends_with(".bin")) {
        run.snapshot.backend = Some("cpu".into());
    } else if run.snapshot.robot.as_deref() == Some("Unitree G1-29dof") {
        run.snapshot.backend = Some("zealot".into());
    } else {
        return Err(
            "Original training recipe is missing; start a new run with an explicit profile.".into(),
        );
    }
    Ok(())
}

/// Continue in a new output directory. Never overwrite the source checkpoint.
pub fn continuation(
    source: &crate::state::Run,
    checkpoint: &str,
    iterations: u32,
) -> Result<crate::state::Run, String> {
    let checkpoint = std::path::Path::new(checkpoint)
        .canonicalize()
        .map_err(|e| format!("Checkpoint is unavailable: {e}"))?;
    if !checkpoint.is_file() || iterations == 0 {
        return Err("Choose a checkpoint and positive additional iteration budget".into());
    }
    let checkpoint = checkpoint.to_string_lossy().into_owned();
    let mut run = source.clone();
    restore_run_recipe(&mut run)?;
    match run.snapshot.backend.as_deref() {
        Some("external") => {
            let profile = run
                .snapshot
                .training_profile
                .as_mut()
                .ok_or("Saved training profile is missing")?;
            let flag = profile
                .get("resume_arg")
                .and_then(|v| v.as_str())
                .unwrap_or("--resume-checkpoint")
                .to_string();
            let args = profile
                .get_mut("args")
                .and_then(|v| v.as_array_mut())
                .ok_or("Profile args are missing")?;
            if let Some(index) = args.iter().position(|v| v.as_str() == Some(flag.as_str())) {
                if index + 1 == args.len() {
                    return Err("Profile resume flag has no value".into());
                }
                args[index + 1] = checkpoint.clone().into();
            } else {
                args.push(flag.into());
                args.push(checkpoint.clone().into());
            }
            if let Some(index) = args.iter().position(|v| v.as_str() == Some("--iterations")) {
                if index + 1 < args.len() {
                    args[index + 1] = "{iterations}".into();
                }
            }
            profile["resume_checkpoint"] = checkpoint.clone().into();
            profile["recipe"]["iterations"] = iterations.into();
            run.iters_per = iterations;
        }
        Some("cpu") => {
            let (_, meta) = nexus_engine::ckpt::read(std::path::Path::new(&checkpoint))
                .map_err(|e| e.to_string())?;
            if meta.n_obs != nexus_engine::env::N_OBS || meta.n_act != nexus_engine::env::N_ACT {
                return Err("Checkpoint does not match the CPU cart-pole task".into());
            }
        }
        Some("zealot") if crate::artifacts::is_safetensors(&checkpoint) => {}
        _ => return Err("The saved backend does not support this checkpoint format".into()),
    }
    run.snapshot.warm = Some(checkpoint);
    run.snapshot.additional_iterations = Some(iterations);
    Ok(run)
}

/// A robot-specific task owns its environment configuration, rather than G1 scenes.
pub fn configure_external_run(
    run: &mut crate::state::Run,
    task: Option<&crate::task_config::Config>,
) -> Result<(), String> {
    if run.snapshot.backend.is_none() {
        run.snapshot.training_profile = if let Some(task) = task {
            Some(authored_profile(task)?)
        } else {
            training_profile(run.snapshot.robot.as_deref())?
                .map(|p| read_profile(std::path::Path::new(&p)))
                .transpose()?
        };
        run.snapshot.backend = Some(if run.snapshot.training_profile.is_some() {
            "external".into()
        } else {
            std::env::var("NEXUS_BACKEND").unwrap_or_else(|_| "zealot".into())
        });
    }
    let Some(value) = &run.snapshot.training_profile else {
        return Ok(());
    };
    if value["robot"].as_str() != run.snapshot.robot.as_deref() {
        return Err("Training profile does not match the run's robot".into());
    }
    run.snapshot.warm = value
        .get("resume_checkpoint")
        .and_then(|v| v.as_str())
        .map(str::to_owned);
    if let Some(checkpoint) = &run.snapshot.warm {
        if !std::path::Path::new(checkpoint).is_file() {
            return Err(format!("Resume checkpoint does not exist: {checkpoint}"));
        }
    }
    if let Some(recipe) = value.get("recipe") {
        let recipe: ExternalRecipe =
            serde_json::from_value(recipe.clone()).map_err(|e| e.to_string())?;
        recipe.validate()?;
        if run.name == "studio run" || run.name == "run" {
            run.name = recipe.task.clone();
        }
        run.set = "robot training task".into();
        run.iters_per = recipe.iterations;
        run.stages = vec![recipe.task];
        run.stage = 0;
        run.round = 1;
        run.snapshot.cfg.clear();
        run.snapshot.scenes = 0;
    }
    Ok(())
}

/// A profile registry lets robot selection choose a trainer without restarting Studio.
/// Existing single-profile files remain supported.
fn resolve_profile(path: &std::path::Path, robot: Option<&str>) -> Result<Option<String>, String> {
    let value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    if let Some(profiles) = value.get("profiles") {
        let profiles = profiles
            .as_object()
            .ok_or("Training profiles must be a robot-to-path map")?;
        let Some(entry) = robot.and_then(|r| profiles.get(r)) else {
            return Ok(None);
        };
        let entry = entry
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or("Training profile path must be nonempty")?;
        let entry = std::path::Path::new(entry);
        let selected = if entry.is_absolute() {
            entry.to_path_buf()
        } else {
            path.parent()
                .unwrap_or(std::path::Path::new("."))
                .join(entry)
        };
        let selected_value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&selected).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        if selected_value.get("robot").and_then(|r| r.as_str()) != robot {
            return Err("Registry training profile must match the selected robot".into());
        }
        return Ok(Some(selected.to_string_lossy().into_owned()));
    }
    Ok(Some(path.to_string_lossy().into_owned()))
}

fn training_profile(robot: Option<&str>) -> Result<Option<String>, String> {
    match std::env::var("NEXUS_TRAINING_PROFILE") {
        Ok(path) => resolve_profile(std::path::Path::new(&path), robot),
        Err(_) => Ok(None),
    }
}

pub fn robot_training_task(robot: &crate::state::Robot) -> Result<Option<String>, String> {
    if robot.use_task {
        return robot
            .task
            .as_ref()
            .map(|c| Some(c.name.clone()))
            .ok_or("The selected authored task is missing".into());
    }
    let name = robot.model_name();
    let Some(path) = training_profile(Some(name))? else {
        return Ok(None);
    };
    let value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    if value.get("robot").and_then(|v| v.as_str()) != Some(name) {
        return Err("Choose a training profile matching this robot".into());
    }
    Ok(Some(
        value
            .get("recipe")
            .and_then(|r| r.get("task"))
            .and_then(|v| v.as_str())
            .unwrap_or("External robot RL trainer")
            .to_string(),
    ))
}

fn authored_profile(task: &crate::task_config::Config) -> Result<serde_json::Value, String> {
    task.validate_model()?;
    if !["generic_mujoco", "open_duck", "microduck"].contains(&task.kind.as_str()) {
        return Err("This editor currently launches configurable URDF tasks; select the official trainer profile for the original duck task".into());
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let python = root.join("training-stack/microduck_rl/.venv/bin/python");
    let worker = root.join(match task.kind.as_str() {
        "open_duck" => "tools/training/open_duck_worker.py",
        "microduck" => "tools/training/microduck_authored_worker.py",
        _ => "tools/training/generic_worker.py",
    });
    if !python.is_file() || !worker.is_file() {
        return Err("Install the MuJoCo / PyTorch / RSL-RL training environment before launching an authored task".into());
    }
    Ok(
        serde_json::json!({"robot":task.robot,"program":python,"cwd":root,
        "args":[worker.to_string_lossy(),"--task-config","{run_dir}/task.json","--run-dir","{run_dir}","--seed","{seed}","--iterations","{iterations}"],
        "checkpoint":"{run_dir}/policy.pt","task_config":task,
        "recipe":{"task":task.name,"num_envs":task.ppo.num_envs,"iterations":task.ppo.iterations,"steps_per_env":task.ppo.steps_per_env}}),
    )
}

fn start_external(
    profile: &str,
    dir: &std::path::Path,
    run: &crate::state::Run,
) -> Result<(nexus_engine::trainer::Handle, String), String> {
    #[derive(serde::Deserialize)]
    struct Profile {
        program: String,
        args: Vec<String>,
        cwd: std::path::PathBuf,
        checkpoint: String,
        robot: String,
        #[serde(default)]
        env: std::collections::BTreeMap<String, String>,
        #[serde(default)]
        recipe: Option<ExternalRecipe>,
        #[serde(default)]
        task_config: Option<crate::task_config::Config>,
        #[serde(default)]
        parallel: Option<serde_json::Value>,
    }
    let path = std::path::Path::new(profile);
    let p: Profile = serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    if let Some(recipe) = &p.recipe {
        recipe.validate()?;
    }
    if p.robot.trim().is_empty() || run.snapshot.robot.as_deref().is_some_and(|r| r != p.robot) {
        return Err("training profile robot must match the selected robot name".into());
    }
    let dir = dir.canonicalize().map_err(|e| e.to_string())?;
    if let Some(task) = &p.task_config {
        task.validate()?;
        let processes = p
            .parallel
            .as_ref()
            .map(|v| v["processes"].as_u64().unwrap_or(0))
            .unwrap_or(1);
        if !(1..=64).contains(&processes) {
            return Err("Captured parallel recipe has an invalid process count".into());
        }
        if task.robot != p.robot
            || p.recipe.as_ref().is_none_or(|r| {
                r.num_envs != task.ppo.num_envs as usize * processes as usize
                    || r.steps_per_env != task.ppo.steps_per_env
            })
        {
            return Err("Captured task disagrees with its training profile".into());
        }
        std::fs::write(
            dir.join("task.json"),
            serde_json::to_vec_pretty(task).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
    }
    let replace = |s: &str| {
        s.replace("{run_dir}", &dir.to_string_lossy())
            .replace("{seed}", &run.snapshot.seed)
            .replace(
                "{num_envs}",
                &p.recipe
                    .as_ref()
                    .map(|r| r.num_envs.to_string())
                    .unwrap_or_default(),
            )
            .replace(
                "{iterations}",
                &p.recipe
                    .as_ref()
                    .map(|r| r.iterations.to_string())
                    .unwrap_or_default(),
            )
    };
    let checkpoint = replace(&p.checkpoint);
    let ck_path = std::path::PathBuf::from(&checkpoint);
    if !ck_path.is_absolute()
        || !ck_path.starts_with(&dir)
        || ck_path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(
            "checkpoint must be an absolute path inside {run_dir} without parent traversal".into(),
        );
    }
    if ck_path.exists() {
        return Err("training checkpoint already exists; choose a fresh output".into());
    }
    let cwd = if p.cwd.is_absolute() {
        p.cwd
    } else {
        path.parent()
            .unwrap_or(std::path::Path::new("."))
            .join(p.cwd)
    };
    std::fs::write(
        dir.join("training-profile.json"),
        std::fs::read(path).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    if std::env::var_os("NEXUS_SUPERVISED").is_none() {
        std::fs::write(
            dir.join("run.json"),
            serde_json::to_vec_pretty(run).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
    }
    let spec = nexus_engine::external::Spec {
        program: p.program,
        args: p.args.iter().map(|a| replace(a)).collect(),
        cwd,
        env: p.env.into_iter().map(|(k, v)| (k, replace(&v))).collect(),
        log: dir.join("metrics.jsonl"),
        checkpoint: ck_path,
    };
    let handle = nexus_engine::external::spawn(spec, external_sample)?;
    if let Some(recipe) = p.recipe {
        if let Ok(mut state) = handle.shared.lock() {
            state.envs = recipe.num_envs;
            state.total_steps =
                recipe.num_envs as u64 * recipe.iterations as u64 * recipe.steps_per_env as u64;
        }
    }
    Ok((handle, checkpoint))
}

#[cfg(test)]
mod external_contract_tests {
    use super::*;
    #[test]
    fn continuation_captures_new_checkpoint_and_budget_without_mutating_original() {
        let path = std::env::temp_dir().join(format!("nexus-continue-{}.pt", std::process::id()));
        std::fs::write(&path, b"checkpoint").unwrap();
        let mut source = crate::state::Store::seed().runs[0].clone();
        source.snapshot.backend = Some("external".into());
        source.snapshot.training_profile = Some(
            serde_json::json!({"robot":"Microduck","args":["--resume-checkpoint","old.pt","--iterations","1000"],"recipe":{"iterations":1000,"num_envs":32,"task":"task"}}),
        );
        let next = continuation(&source, &path.to_string_lossy(), 7).unwrap();
        assert_eq!(next.snapshot.additional_iterations, Some(7));
        let cfg = next.snapshot.training_profile.unwrap();
        assert_eq!(cfg["recipe"]["iterations"], 7);
        assert_eq!(cfg["args"][3], "{iterations}");
        assert_eq!(
            source.snapshot.training_profile.unwrap()["recipe"]["iterations"],
            1000
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn robot_registry_selects_matching_profiles_and_preserves_legacy_profiles() {
        let root =
            std::env::temp_dir().join(format!("nexus-profile-registry-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let registry = root.join("robots.json");
        std::fs::write(root.join("mini.json"), r#"{"robot":"Open Duck Mini v2"}"#).unwrap();
        std::fs::write(root.join("micro.json"), r#"{"robot":"Microduck"}"#).unwrap();
        std::fs::write(
            &registry,
            r#"{"profiles":{"Open Duck Mini v2":"mini.json","Microduck":"micro.json"}}"#,
        )
        .unwrap();
        assert_eq!(
            resolve_profile(&registry, Some("Open Duck Mini v2")).unwrap(),
            Some(root.join("mini.json").to_string_lossy().into_owned())
        );
        assert_eq!(
            resolve_profile(&registry, Some("Microduck")).unwrap(),
            Some(root.join("micro.json").to_string_lossy().into_owned())
        );
        assert_eq!(
            resolve_profile(&registry, Some("Unitree G1-29dof")).unwrap(),
            None
        );
        assert!(resolve_profile(&root.join("micro.json"), Some("Microduck"))
            .unwrap()
            .is_some());
        std::fs::write(root.join("mini.json"), r#"{"robot":"Microduck"}"#).unwrap();
        assert!(resolve_profile(&registry, Some("Open Duck Mini v2")).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn preserves_actionable_worker_errors() {
        let error =
            external_sample(r#"{"event":"error","message":"Microduck requires a CUDA host"}"#)
                .unwrap_err();
        assert_eq!(error, "Microduck requires a CUDA host");
        assert!(
            external_sample(r#"{"event":"status","message":"compiling"}"#)
                .unwrap()
                .is_none()
        );
    }
    #[test]
    fn accepts_measured_progress_and_rejects_missing_values() {
        let early = external_sample(r#"{"event":"sample","step":32,"reward":0.1,"fall_rate_measured":false,"fall_rate":0,"steps_per_sec":100,"episode_len":8}"#).unwrap().unwrap();
        assert!(early.fall_rate_unmeasured);
        assert_eq!(early.step, 32);
        let sample = external_sample(r#"{"event":"sample","step":98304,"reward":0.25,"fall_rate":0.5,"steps_per_sec":1000,"episode_len":25}"#).unwrap().unwrap();
        assert_eq!(sample.step, 98304);
        assert_eq!(sample.fall_rate, 0.5);
        assert!(external_sample(r#"{"event":"sample","step":98304,"reward":null}"#).is_err());
    }
    #[test]
    fn validates_external_training_budgets() {
        let mut recipe = ExternalRecipe {
            steps_per_env: 24,
            task: "Microduck walking".into(),
            num_envs: 4096,
            iterations: 6000,
        };
        assert!(recipe.validate().is_ok());
        recipe.num_envs = 0;
        assert!(recipe.validate().is_err());
        recipe.num_envs = usize::MAX;
        assert!(recipe.validate().is_err());
    }
}
