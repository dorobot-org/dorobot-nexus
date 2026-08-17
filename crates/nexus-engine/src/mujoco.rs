//! Sim-to-sim against MuJoCo, by driving zealot's own harness.
//!
//! ## Why this is not a new evaluator
//!
//! Running a policy in a second engine means reproducing its observation frame
//! exactly, and zealot's has conventions no one would guess:
//!
//! * `last_action` is **lag-2** — the observation at decision *t* carries the
//!   action from *t−2*, zeros for the first two steps of an episode.
//! * `joint_vel` is a **finite difference**, `(q_t − q_{t−1})/control_dt`, not
//!   the true joint velocity the engine could report.
//! * the PD target is `clamp(default + 0.5·action, joint range)`, driven as an
//!   explicit torque at 200 Hz with the model's own actuators **disabled**.
//! * the frame is stacked 5 deep, oldest→newest, reset-replicated, then
//!   normalised with the checkpoint's Welford statistics.
//! * the gait clock is `φ(t) = max(0, t−1)·control_dt / 0.7`.
//!
//! Get any one wrong and the policy falls over in MuJoCo for reasons that are
//! export drift rather than physics — which reads as a transfer failure and is
//! not one. The console has carried the string "MuJoCo replica — catches export
//! drift, not modelling error" since before this module existed.
//!
//! `zealot-stack/zealot/scripts/sim2sim_g1_mujoco.py` already encodes all of
//! it, against mujoco_playground's official G1, and auto-detects the 45/48/53
//! observation width from the checkpoint. So this module launches that and
//! reads its stdout. Writing a second harness would mean re-deriving those
//! conventions and being subtly wrong.
//!
//! ## Why a subprocess
//!
//! The same reason zealot itself is one, and it is forced rather than
//! stylistic: MuJoCo is a C library and the harness is Python, so linking
//! either would put a system dependency into `nexus-engine`, which has none.
//! A machine without the harness simply is not offered the option.
//!
//! ## What it is worth
//!
//! [`crate::crosssim`]'s two existing arms compare Euler against RK4, and one
//! control decimation against another. Both share the dynamics function, so
//! both catch integration artefacts and nothing else — the module says so
//! itself. MuJoCo has its own contact model and solver, so this is the first
//! arm that can catch a *modelling* error, and it is the check Unitree's own
//! pipeline performs before sim-to-real.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::crosssim::Score;

/// zealot's checkout, as `scripts/setup-zealot.sh` lays it out.
const HARNESS: &str = "zealot-stack/zealot/scripts/sim2sim_g1_mujoco.py";
/// Interpreter with MuJoCo importable. A virtualenv rather than whatever
/// `python3` resolves to: the system one here is 3.9, which has no MuJoCo wheel
/// and tries to build from source.
const VENV_PY: &str = "mujoco-stack/venv/bin/python3";
/// The scene the harness expects — flat terrain, feet-only collision. Cloned
/// rather than pip-installed because `mujoco_playground` is not on PyPI.
const SCENE: &str =
    "mujoco-stack/playground/mujoco_playground/_src/locomotion/g1/xmls/scene_mjx_feetonly_flat_terrain.xml";

pub fn harness_path() -> PathBuf {
    PathBuf::from(HARNESS)
}

/// Is the MuJoCo arm available?
///
/// Only the harness is checked here. Whether *this* interpreter has `mujoco`
/// and `safetensors` importable is not knowable without paying to start
/// Python, so a missing package surfaces as a run failure with the import
/// error attached rather than as a silently absent option.
pub fn available() -> bool {
    harness_path().is_file()
}

/// What is missing, for a control that says why instead of failing when
/// pressed.
pub fn why_unavailable() -> Option<String> {
    if !harness_path().is_file() {
        return Some(format!(
            "no MuJoCo harness at {} — run scripts/setup-zealot.sh",
            harness_path().display()
        ));
    }
    if !Path::new(VENV_PY).is_file() {
        return Some(format!("no MuJoCo interpreter at {VENV_PY}"));
    }
    if !Path::new(SCENE).is_file() {
        return Some(format!("no G1 scene at {SCENE}"));
    }
    None
}

/// One episode the harness reported.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Attempt {
    pub seconds: f32,
    pub metres: f32,
    /// `false` means it reached the time limit still upright.
    pub fell: bool,
}

/// What a run measured.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    pub attempts: Vec<Attempt>,
    /// Observation frame width the checkpoint turned out to use: 45, 48 or 53.
    /// Recorded because it says which policy generation was evaluated, and a
    /// mismatch against the training config is worth seeing.
    pub obs_frame: usize,
}

impl Report {
    pub fn fall_rate(&self) -> f32 {
        if self.attempts.is_empty() {
            return 0.0;
        }
        self.attempts.iter().filter(|a| a.fell).count() as f32 / self.attempts.len() as f32
    }

    /// Mean forward speed actually achieved, m/s.
    pub fn achieved_vx(&self) -> f32 {
        let (d, t): (f32, f32) = self
            .attempts
            .iter()
            .fold((0.0, 0.0), |(d, t), a| (d + a.metres, t + a.seconds));
        if t <= 0.0 {
            0.0
        } else {
            d / t
        }
    }

    /// Scores in the shape `crosssim::Report` already renders.
    ///
    /// `commanded` is the vx the harness was told to hold, which the caller
    /// sets — it is not recoverable from stdout, and inferring it would mean
    /// scoring a policy against a command it was never given.
    pub fn score(&self, commanded: f32) -> Score {
        let fall_rate = self.fall_rate();
        let achieved = self.achieved_vx();
        Score {
            survival: 1.0 - fall_rate,
            tracking: if commanded.abs() < 1e-6 {
                0.0
            } else {
                (1.0 - (achieved - commanded).abs() / commanded.abs()).clamp(0.0, 1.0)
            },
            // MuJoCo does not compute zealot's reward — that lives in the
            // training environment. Reporting achieved speed is the honest
            // substitute and is what the two engines are being compared on;
            // `crosssim::zealot_cross` makes the same choice for the same
            // reason.
            reward: achieved,
            fall_rate,
        }
    }
}

/// Parse the harness's stdout.
///
/// Tolerant of everything else on the stream: MuJoCo, EGL and the harness's own
/// diagnostics (`TRACE`, `TREMOR`, `SPECTRUM`, `STANCE`) all print freely, so a
/// parser that required a clean stream would fail on a working machine.
pub fn parse(out: &str) -> Report {
    let mut r = Report::default();
    for line in out.lines() {
        let l = line.trim();
        // "obs frame 45 (no gyro)"
        if let Some(rest) = l.strip_prefix("obs frame ") {
            if let Some(n) = rest.split_whitespace().next().and_then(|t| t.parse().ok()) {
                r.obs_frame = n;
            }
            continue;
        }
        // "attempt 3: fell after 3.2s, traveled 0.94 m"
        if !l.starts_with("attempt ") {
            continue;
        }
        let Some((why, tail)) = l.split_once(" after ") else { continue };
        let fell = why.contains("fell");
        let Some((secs, rest)) = tail.split_once("s, traveled ") else { continue };
        let Some(metres) = rest.split_whitespace().next() else { continue };
        let (Ok(seconds), Ok(metres)) = (secs.trim().parse(), metres.parse()) else { continue };
        r.attempts.push(Attempt { seconds, metres, fell });
    }
    r
}

/// Where the harness dumps the trajectory it simulated.
fn rollout_json_path() -> PathBuf {
    std::env::temp_dir().join("nexus_mujoco_rollout.json")
}

/// The one place the harness is invoked, returning its stdout.
///
/// Every caller below wants the same run with the same environment; they differ
/// only in what they read back afterwards. Keeping that in one function is what
/// makes [`evaluate_with_rollout`] possible — the numbers and the motion are
/// two readings of a single 45-second run, not two runs.
///
/// `rollout_json` asks the harness to also dump the trajectory there.
fn run(
    policy: &str,
    command_vx: f32,
    seconds: u32,
    rollout_json: Option<&Path>,
) -> Result<String, String> {
    let h = harness_path();
    if !h.is_file() {
        return Err(format!(
            "MuJoCo harness not found at {} — run scripts/setup-zealot.sh",
            h.display()
        ));
    }
    // Prefer the venv; fall back to `python3` so a machine that has MuJoCo
    // installed globally still works.
    let py = Path::new(VENV_PY);
    let py = if py.is_file() { py.as_os_str() } else { "python3".as_ref() };
    let mut cmd = Command::new(py);
    // An externally chosen scene wins — launching the console with
    // S2S_MODEL_XML (+ S2S_HFIELD_JSON / S2S_SPAWN, which inherit on their
    // own) points the whole sim-to-sim arm at a terrain variant, e.g. the
    // rough-slope scene scripts/make_mujoco_terrain_scene.py builds. The
    // playground flat scene stays the default.
    if std::env::var_os("S2S_MODEL_XML").is_none() && Path::new(SCENE).is_file() {
        cmd.env("S2S_MODEL_XML", SCENE);
    }
    // Nothing here wants the clip: the harness opens ffmpeg before its loop, so
    // without this a machine with MuJoCo but no ffmpeg measures nothing.
    // See patches/zealot-sim2sim-no-video.patch.
    cmd.env("S2S_NO_VIDEO", "1");
    // The harness defaults to EGL, which exists on Linux and not on macOS —
    // MuJoCo rejects it outright there. `setdefault` on its side means setting
    // it here wins. Left alone if the caller already chose one.
    if std::env::var_os("MUJOCO_GL").is_none() {
        cmd.env("MUJOCO_GL", if cfg!(target_os = "macos") { "glfw" } else { "egl" });
    }
    if let Some(p) = rollout_json {
        // Removed first, so a failed run reads as "no rollout" rather than
        // silently replaying the previous one.
        let _ = std::fs::remove_file(p);
        cmd.env("S2S_ROLLOUT_JSON", p);
    }
    let out = cmd
        .arg(&h)
        .arg(policy)
        // The harness writes an mp4 as its main artefact and takes the path
        // positionally even with video off, so it goes somewhere disposable.
        .arg(std::env::temp_dir().join("nexus_sim2sim.mp4"))
        .arg(seconds.to_string())
        .env("BIPED_CMD", format!("{command_vx},0,0"))
        .output()
        .map_err(|e| format!("could not start the MuJoCo harness: {e}"))?;

    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        // An unimportable module is the common failure and the message is the
        // whole diagnosis, so it is passed through rather than summarised.
        let tail: Vec<&str> = err.lines().rev().take(3).collect();
        return Err(format!(
            "MuJoCo harness failed ({}): {}",
            out.status,
            tail.into_iter().rev().collect::<Vec<_>>().join(" / ")
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Run the harness against a checkpoint.
///
/// Blocking; the caller runs it off the UI thread as `crosssim::spawn` already
/// does. `seconds` is wall-clock budget for the whole clip, which the harness
/// divides into as many attempts as survive.
pub fn evaluate(policy: &str, command_vx: f32, seconds: u32) -> Result<Report, String> {
    let stdout = run(policy, command_vx, seconds, None)?;
    let r = parse(&stdout);
    if r.attempts.is_empty() {
        return Err("MuJoCo harness reported no attempts; the run ended early".into());
    }
    Ok(r)
}

/// Both products of a single run: what MuJoCo measured, and what it simulated.
///
/// The numbers say whether the policy transferred; the motion says *how* it
/// failed, and a fall at one second looks nothing like a policy that stands
/// still. Asking for them separately meant running the harness twice and
/// waiting through 45 seconds of physics for data the first run had already
/// produced.
///
/// The rollout is `Option` because it is the weaker of the two: a run whose
/// numbers parsed but whose dump did not is still a valid verdict, and the
/// verdict is what the caller asked for.
pub fn evaluate_with_rollout(
    policy: &str,
    command_vx: f32,
    seconds: u32,
) -> Result<(Report, Option<crate::zealot::Rollout>), String> {
    let out = rollout_json_path();
    let stdout = run(policy, command_vx, seconds, Some(&out))?;
    let r = parse(&stdout);
    if r.attempts.is_empty() {
        return Err("MuJoCo harness reported no attempts; the run ended early".into());
    }
    let traj = std::fs::read_to_string(&out)
        .ok()
        .and_then(|t| crate::zealot::parse_rollout(&t));
    Ok((r, traj))
}

/// Run the harness and return the rollout MuJoCo actually simulated.
///
/// Mirrors [`crate::zealot::drive`] so the viewer path is identical: a worker
/// thread calls this, drops the result into the same `RolloutSlot`, and the
/// URDF view replays it. The difference is only which engine produced the
/// motion — which is the entire point of showing it.
///
/// The harness writes the rollout in this project's own schema (see
/// `S2S_ROLLOUT_JSON` in the patch), so it is read back with
/// [`crate::zealot::parse_rollout`] rather than a second parser. It reorders
/// the quaternion on the way out: MuJoCo stores `(w,x,y,z)` and the schema is
/// `(x,y,z,w)`, and getting that wrong renders as a robot lying on its side
/// rather than as an error.
///
/// When the caller also wants the verdict, [`evaluate_with_rollout`] returns
/// both from one run instead.
pub fn drive(policy: &str, command_vx: f32, seconds: u32) -> Option<crate::zealot::Rollout> {
    let out = rollout_json_path();
    run(policy, command_vx, seconds, Some(&out)).ok()?;
    crate::zealot::parse_rollout(&std::fs::read_to_string(&out).ok()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Verbatim** stdout from a real run on this machine:
    ///
    /// ```text
    /// S2S_NO_VIDEO=1 MUJOCO_GL=glfw S2S_MODEL_XML=<playground g1 flat scene> \
    ///   venv/bin/python3 sim2sim_g1_mujoco.py <ckpt> /tmp/x.mp4 45
    /// ```
    ///
    /// MuJoCo 3.11.0, playground's `scene_mjx_feetonly_flat_terrain.xml`,
    /// menagerie G1 meshes. The standard `zealot.rs` sets — a parser for
    /// someone else's stdout is worth exactly its fidelity to the real thing.
    const OUT: &str = "\
obs frame 53 (with gyro, step cue)
policy joints: 12, held joints: 17
attempt 1: timeout after 20.0s, traveled 0.00 m
attempt 2: timeout after 20.0s, traveled 0.00 m

2 completed attempts; mean survival 20.0s, best 20.0s
video → /tmp/x.mp4";

    /// A second shape the parser must handle, which the capture above does not
    /// exercise: episodes that end in a fall, and the diagnostic lines the
    /// harness prints when it has enough data for them.
    const OUT_FALLS: &str = "\
obs frame 45 (no gyro)
attempt 1: fell after 3.2s, traveled 0.94 m
attempt 2: timeout after 20.0s, traveled 7.31 m
attempt 3: fell after 5.8s, traveled 1.90 m
STANCE feet lateral sep: mean=0.212 p5=0.180
SPECTRUM left_hip_pitch: 4.2% of action power above 5 Hz";

    #[test]
    fn attempts_and_frame_width_are_read() {
        // The real capture: a 53-wide policy that held twice without moving.
        let r = parse(OUT);
        assert_eq!(r.obs_frame, 53);
        assert_eq!(r.attempts.len(), 2);
        assert!(!r.attempts[0].fell, "timeout is survival, not a fall");
        assert_eq!(r.attempts[0].metres, 0.0);

        let r = parse(OUT_FALLS);
        assert_eq!(r.obs_frame, 45);
        assert_eq!(r.attempts.len(), 3);
        assert!(r.attempts[0].fell);
        assert!(!r.attempts[1].fell, "timeout is survival, not a fall");
        assert!((r.attempts[1].metres - 7.31).abs() < 1e-4);
    }

    #[test]
    fn diagnostics_on_the_stream_are_ignored() {
        // STANCE / SPECTRUM / the video line are not attempts.
        assert_eq!(parse(OUT_FALLS).attempts.len(), 3);
        assert_eq!(parse(OUT).attempts.len(), 2);
    }

    #[test]
    fn fall_rate_counts_falls_not_timeouts() {
        assert!((parse(OUT_FALLS).fall_rate() - 2.0 / 3.0).abs() < 1e-6);
        // Two timeouts are not falls.
        assert_eq!(parse(OUT).fall_rate(), 0.0);
    }

    #[test]
    fn achieved_speed_is_total_distance_over_total_time() {
        // (0.94 + 7.31 + 1.90) / (3.2 + 20.0 + 5.8)
        let r = parse(OUT_FALLS);
        assert!((r.achieved_vx() - 10.15 / 29.0).abs() < 1e-4);
    }

    #[test]
    fn tracking_is_relative_to_the_command_it_was_given() {
        let r = parse(OUT_FALLS);
        // Commanded exactly what it achieved: perfect tracking.
        let a = r.achieved_vx();
        assert!((r.score(a).tracking - 1.0).abs() < 1e-5);
        // Commanded twice that: half the speed, so half the tracking.
        assert!((r.score(a * 2.0).tracking - 0.5).abs() < 1e-4);
    }

    #[test]
    fn a_zero_command_does_not_divide_by_it() {
        assert_eq!(parse(OUT_FALLS).score(0.0).tracking, 0.0);
    }

    #[test]
    fn standing_still_scores_zero_tracking_despite_surviving() {
        // The captured run: never fell, never moved. Survival alone must not
        // read as success, or a policy that stands still passes sim2sim.
        let s = parse(OUT).score(0.3);
        assert_eq!(s.survival, 1.0);
        assert_eq!(s.fall_rate, 0.0);
        assert_eq!(s.tracking, 0.0);
    }

    #[test]
    fn a_run_with_no_attempts_is_empty_not_zero_scored() {
        // A harness that died before its first attempt has measured nothing;
        // that is a different finding from a policy that scored zero.
        let r = parse("Traceback (most recent call last):\n  ModuleNotFoundError: mujoco\n");
        assert!(r.attempts.is_empty());
        assert_eq!(r.fall_rate(), 0.0);
        assert_eq!(r.achieved_vx(), 0.0);
    }
}
