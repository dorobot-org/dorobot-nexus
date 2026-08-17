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
    std::fs::remove_file(from_ui(u).path())
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
    let mut out = vec![];
    let repo = repo_dir();
    if let Ok(rd) = std::fs::read_dir(&repo) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with("dorobot_nexus.safetensors") || name == "curriculum.safetensors" {
                let kb = e.metadata().map(|m| m.len() / 1024).unwrap_or(0);
                out.push(DiskCkpt { label: name.clone(), path: format!("{repo}/{name}"), kb });
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

pub struct Replay {
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
    let text = std::fs::read_to_string(path).ok()?;
    let dt = find_num(&text, "\"dt\"")?;
    let names_raw = find_array(&text, "\"joint_names\"")?;
    let joint_names: Vec<String> = names_raw
        .split(',')
        .map(|s| s.trim().trim_matches('"').to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let joints_raw = find_array(&text, "\"joints\"")?;
    let mut joints = vec![];
    for row in joints_raw.split("],") {
        let row = row.trim().trim_start_matches('[').trim_end_matches(']');
        if row.is_empty() {
            continue;
        }
        let vals: Vec<f32> = row.split(',').filter_map(|x| x.trim().parse().ok()).collect();
        if !vals.is_empty() {
            joints.push(vals);
        }
    }
    if joints.is_empty() {
        return None;
    }
    // Base pose rows (xyz + quat xyzw), same flat schema. Optional: an old
    // recording without them still replays, marching in place.
    let mut base = vec![];
    if let Some(base_raw) = find_array(&text, "\"base\"") {
        for row in base_raw.split("],") {
            let row = row.trim().trim_start_matches('[').trim_end_matches(']');
            if row.is_empty() {
                continue;
            }
            let vals: Vec<f32> = row.split(',').filter_map(|x| x.trim().parse().ok()).collect();
            if vals.len() >= 7 {
                base.push([vals[0], vals[1], vals[2], vals[3], vals[4], vals[5], vals[6]]);
            }
        }
    }
    Some(Replay { name: name.into(), joint_names, joints, base, dt })
}

fn find_num(text: &str, key: &str) -> Option<f64> {
    let i = text.find(key)? + key.len();
    let rest = &text[i..];
    let j = rest.find(':')? + 1;
    let tail = rest[j..].trim_start();
    let end = tail.find([',', '\n', '}'])?;
    tail[..end].trim().parse().ok()
}

/// Returns the raw contents between the key's opening `[` and its matching `]`.
fn find_array(text: &str, key: &str) -> Option<String> {
    let i = text.find(key)? + key.len();
    let rest = &text[i..];
    let start = rest.find('[')? + 1;
    let mut depth = 1;
    let bytes = rest.as_bytes();
    let mut end = start;
    for (k, b) in bytes.iter().enumerate().skip(start) {
        match b {
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    end = k;
                    break;
                }
            }
            _ => {}
        }
    }
    Some(rest[start..end].to_string())
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
    /// Human progress lines for the right rail's activity tail. Derived from
    /// the published state rather than being its source — nothing parses these.
    pub rx: Receiver<String>,
    done: Arc<AtomicBool>,
    surface: Option<Arc<Mutex<nexus_engine::sweep::Surface>>>,
    /// The live metric stream of a Train job — the source `drain_real_proc`
    /// folds into `st.live`, where every screen reads it.
    pub train: Option<nexus_engine::trainer::Handle>,
}

impl RealProc {
    pub fn finished(&self) -> bool {
        self.done.load(Ordering::Relaxed)
    }

    /// The sweep surface in the shape the heat map takes: rows of cells, `None`
    /// where unmeasured. Read live, so the grid fills in cell by cell.
    pub fn grid(&self) -> Option<Vec<Vec<Option<f64>>>> {
        use nexus_engine::sweep::{COLS, ROWS};
        let s = self.surface.as_ref()?.lock().ok()?;
        Some(
            (0..ROWS)
                .map(|r| (0..COLS).map(|c| s.cell(r, c).map(f64::from)).collect())
                .collect(),
        )
    }

    /// Ask a training run to stop at the next interval.
    pub fn stop(&self) {
        if let Some(h) = &self.train {
            h.stop();
        }
    }
}

/// What a real sweep would measure.
enum SweepTarget {
    /// The zealot export, swept across PD gain and friction on the GPU.
    Zealot(String),
    /// The built-in run's newest checkpoint, swept across mass and force.
    BuiltIn,
}

fn sweep_target() -> Option<SweepTarget> {
    let z = nexus_engine::cli::zealot_ckpt_path();
    if std::path::Path::new(&z).is_file() {
        return Some(SweepTarget::Zealot(z));
    }
    (!nexus_engine::ckpt::list(nexus_engine::trainer::RUN_ID).is_empty())
        .then_some(SweepTarget::BuiltIn)
}

/// Whether a real sweep has anything to measure.
///
/// This replaces a test for a built binary. The engine ships inside this one,
/// so the honest question is no longer "was it compiled" but "is there a
/// checkpoint" — offering the control on the strength of the former and then
/// failing on the latter is the dead button this codebase keeps arguing against.
pub fn can_sweep() -> bool {
    sweep_target().is_some()
}

/// Start the robustness sweep on the engine's own threads.
pub fn run_sweep() -> Result<RealProc, String> {
    let surface = match sweep_target() {
        // A zealot checkpoint with no zealot binary still sweeps — on the CPU,
        // over different axes. Refusing outright would be worse: the fallback
        // is a real measurement, and the surface labels which axes it varied.
        #[cfg(feature = "zealot")]
        Some(SweepTarget::Zealot(ck)) => nexus_engine::sweep::zealot_sweep::spawn(&ck)
            .or_else(|| nexus_engine::sweep::spawn(nexus_engine::trainer::RUN_ID)),
        #[cfg(not(feature = "zealot"))]
        Some(SweepTarget::Zealot(_)) => nexus_engine::sweep::spawn(nexus_engine::trainer::RUN_ID),
        Some(SweepTarget::BuiltIn) => nexus_engine::sweep::spawn(nexus_engine::trainer::RUN_ID),
        None => None,
    }
    .ok_or_else(|| "no checkpoint to sweep".to_string())?;

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
                        let _ = tx.send(format!("{collapsed} cells unmeasured — rollout collapsed"));
                    }
                    break;
                }
            }
            fin.store(true, Ordering::Relaxed);
        });
    }
    Ok(RealProc { kind: ProcKind::Sweep, rx, done, surface: Some(surface), train: None })
}

/// Start a real training run on the engine's own threads.
///
/// Makes the same backend choice `--headless` makes — zealot's GPU run when
/// that stack is built, the CPU trainer otherwise — at the same population, so
/// the env-step budget means the same thing whichever backend answers.
pub fn run_train(total_steps: u64) -> Result<RealProc, String> {
    const ENVS: usize = 256;
    // DOROBOT_ATTACH_LOG turns Train into an observer: instead of spawning a
    // trainer, tail the named log of one already running (whatever env count
    // and budget IT was launched with) into the same metric stream. Read-only
    // — Stop detaches the view, it does not kill someone else's run.
    let attached = std::env::var("DOROBOT_ATTACH_LOG")
        .ok()
        .and_then(|p| nexus_engine::zealot::attach_log(&p));
    let (h, backend) = if let Some(h) = attached {
        (h, "zealot · attached (read-only)")
    } else {
        #[cfg(feature = "zealot")]
        let pair = match nexus_engine::zealot::spawn(
            ENVS,
            // zealot counts iterations and emits 24 steps per env per iteration,
            // so the budget converts rather than changing meaning.
            (total_steps / (ENVS as u64 * 24)).max(1),
            &nexus_engine::cli::zealot_ckpt_path(),
        ) {
            Some(h) => (h, "zealot · GPU"),
            None => (nexus_engine::trainer::spawn(ENVS, total_steps, 1), "cpu"),
        };
        #[cfg(not(feature = "zealot"))]
        let pair = (nexus_engine::trainer::spawn(ENVS, total_steps, 1), "cpu");
        pair
    };

    let (tx, rx) = channel();
    let done = Arc::new(AtomicBool::new(false));
    let _ = tx.send(format!("training started · {backend} · {ENVS} envs · {total_steps} env-steps"));
    {
        let (shared, fin) = (Arc::clone(&h.shared), Arc::clone(&done));
        std::thread::spawn(move || {
            let mut seen = 0usize;
            loop {
                std::thread::sleep(Duration::from_millis(250));
                let Ok(g) = shared.lock() else { break };
                let running = g.running;
                let fresh: Vec<_> = g.samples[seen.min(g.samples.len())..].to_vec();
                seen = g.samples.len();
                drop(g);
                for s in fresh {
                    let _ = tx.send(format!(
                        "step {}k · reward {:.4} · falls {:.1}% · {:.0} steps/s",
                        s.step / 1000,
                        s.reward,
                        s.fall_rate * 100.0,
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
    Ok(RealProc { kind: ProcKind::Train, rx, done, surface: None, train: Some(h) })
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
            Err(_) => (String::new(), false, [blank(0), blank(1), blank(2), blank(3)]),
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
    Ok(MjRun { report, rollout, replayed: false })
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

#[cfg(test)]
mod tests {
    use super::*;

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

    fn have_repo() -> bool {
        std::path::Path::new(&repo_dir()).exists()
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
    /// `have_repo()` was false and it never ran. A test that names artifacts
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
    fn loads_real_recordings_and_rollout() {
        if !have_repo() {
            return;
        }
        init_env();
        let recs = scene::Recording::list();
        assert!(!recs.is_empty());
        let r = &recs[0];
        let path = std::path::Path::new(&repo_dir()).join(&r.rollout);
        let rp = load_rollout(&path, &r.name).expect("rollout parses");
        assert_eq!(rp.joints.len(), r.frames);
        assert_eq!(rp.joints[0].len(), rp.joint_names.len());
        assert!(rp.dt > 0.0);
    }

    #[test]
    fn real_ckpts_listed() {
        if !have_repo() {
            return;
        }
        let c = real_ckpts();
        assert!(c.iter().any(|c| c.label.contains("safetensors")));
    }
}
