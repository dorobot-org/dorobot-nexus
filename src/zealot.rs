//! The zealot backend: a real GPU trainer behind the same metric stream.
//!
//! `trainer.rs` promised that "the trainer is a job behind a metric stream, so
//! it can be swapped without touching a screen". This is that swap. Every
//! screen keeps reading `Shared`; only the producer changes.
//!
//! zealot is driven as a **subprocess**, not linked. That is forced, not
//! stylistic: its `lib.rs` exposes nothing but `mod guides` — the training code
//! lives in `src/bin/` behind `#[path]` includes — and it path-depends on five
//! sibling checkouts outside its own repo, so it cannot be a git dependency and
//! cannot be an optional path dependency either (cargo loads every dependency
//! manifest during resolution, feature-gated or not, so a missing sibling
//! breaks a clean clone). Talking to its stdout keeps zealot's Rust untouched
//! and keeps `cargo build` here working with nothing but a toolchain.
//!
//! Run `scripts/setup-zealot.sh` to produce the binary this looks for.

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

use crate::trainer::{Handle, Sample, Shared};

/// Where the trainer binary is expected, unless `DOROBOT_ZEALOT_BIN` says
/// otherwise. `scripts/setup-zealot.sh` builds exactly this path.
const DEFAULT_BIN: &str = "zealot-stack/zealot/target/release/biped_train_gpu";

/// zealot's reward terms, in the order this app's four slots expect them.
/// The names are zealot's own, read off its `[rb]` line; the app's
/// `env::TERM_NAMES` were modelled on them, so the mapping is 1:1 except that
/// zealot splits torque into leg and ankle and only the leg term is shown.
const TERMS: [&str; 4] = ["track_lin_vel", "upright", "action_rate", "torque_leg"];

pub fn binary_path() -> PathBuf {
    std::env::var("DOROBOT_ZEALOT_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(DEFAULT_BIN))
}

/// True when the zealot stack has been built on this machine.
pub fn available() -> bool {
    binary_path().is_file()
}

/// `biped_drive` sits next to the trainer: same build, same stack. It rolls a
/// checkpoint out deterministically in a single environment with randomisation
/// off, which is exactly what a probe screen wants.
pub fn drive_path() -> PathBuf {
    binary_path().with_file_name("biped_drive")
}

/// A deterministic rollout of one checkpoint, as `biped_drive` records it.
///
/// This is the surface that makes the 3D view real: `joints` is per-frame joint
/// angles in radians, in `joint_names` order, and `base` is the floating base
/// as `[x, y, z, qx, qy, qz, qw]`.
#[derive(Default, Clone)]
pub struct Rollout {
    pub dt: f32,
    pub joint_names: Vec<String>,
    pub base: Vec<[f32; 7]>,
    pub joints: Vec<Vec<f32>>,
}

impl Rollout {
    pub fn len(&self) -> usize {
        self.joints.len().min(self.base.len())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Forward progress over the whole rollout, in metres per second — the
    /// number a velocity-tracking policy is actually judged on.
    pub fn achieved_vx(&self) -> f32 {
        let n = self.len();
        if n < 2 || self.dt <= 0.0 {
            return 0.0;
        }
        (self.base[n - 1][0] - self.base[0][0]) / ((n - 1) as f32 * self.dt)
    }

    /// The angle vector for one frame, laid out for the URDF viewer.
    ///
    /// `RobotView::set_joint_angles` zips against every joint the loader read,
    /// in URDF document order, so this returns a full-length vector with
    /// zealot's twelve leg angles placed by name and everything else left at
    /// zero. Matching by name rather than position is what stops a URDF with
    /// extra fixed joints — the G1 has several — from posing the wrong limb.
    pub fn pose(&self, frame: usize, urdf_joints: &[String]) -> Vec<f32> {
        let mut angles = vec![0.0; urdf_joints.len()];
        let Some(row) = self.joints.get(frame) else {
            return angles;
        };
        for (name, angle) in self.joint_names.iter().zip(row.iter()) {
            if let Some(i) = urdf_joints.iter().position(|u| u == name) {
                // A non-finite angle would propagate into the transform chain
                // and make the whole robot vanish; hold the previous pose.
                if angle.is_finite() {
                    angles[i] = *angle;
                }
            }
        }
        angles
    }

    /// A rollout counts as fallen if the base ever drops below `floor`. zealot
    /// terminates on its own thresholds; this is the coarse check a sweep cell
    /// needs, and it is deliberately independent of zealot's own reward.
    pub fn fell(&self, floor: f32) -> bool {
        self.base.iter().any(|b| b[2] < floor || !b[2].is_finite())
    }
}

/// A rollout slot the UI can hold while a worker thread fills it.
///
/// A named type rather than a bare `Arc<Mutex<Option<…>>>` field: makepad's
/// `Script` derive parses a restricted type grammar and rejects the nested
/// form outright ("Unexpected field form").
#[derive(Default, Clone)]
pub struct RolloutSlot(Arc<Mutex<Option<Rollout>>>);

impl RolloutSlot {
    pub fn set(&self, r: Option<Rollout>) {
        if let Ok(mut g) = self.0.lock() {
            *g = r;
        }
    }

    /// The pose at `frame`, if a rollout has arrived and has frames.
    pub fn pose(&self, frame: usize, urdf_joints: &[String]) -> Option<Vec<f32>> {
        let g = self.0.lock().ok()?;
        let r = g.as_ref()?;
        if r.is_empty() {
            return None;
        }
        Some(r.pose(frame % r.len(), urdf_joints))
    }

    pub fn len(&self) -> usize {
        self.0.lock().ok().and_then(|g| g.as_ref().map(|r| r.len())).unwrap_or(0)
    }

    /// The rollout's control period. Playback uses it to run at the rate the
    /// policy actually ran, rather than at whatever rate the UI ticks.
    pub fn dt(&self) -> f32 {
        self.0
            .lock()
            .ok()
            .and_then(|g| g.as_ref().map(|r| r.dt))
            .filter(|dt| *dt > 0.0)
            .unwrap_or(0.02)
    }
}

/// How to drive a checkpoint: a constant velocity command, held for `seconds`.
#[derive(Clone, Copy)]
pub struct Drive {
    pub vx: f32,
    pub vy: f32,
    pub yaw_rate: f32,
    pub seconds: f32,
}

impl Default for Drive {
    fn default() -> Self {
        Self { vx: 0.3, vy: 0.0, yaw_rate: 0.0, seconds: 3.0 }
    }
}

/// Roll a checkpoint out and read back the trajectory.
///
/// `knobs` are zealot's own `BIPED_*` environment variables, which is how the
/// robustness sweep changes the physics without touching zealot's code.
/// Blocking: it runs a GPU simulation and takes seconds, so callers put it on
/// a thread.
pub fn drive(ckpt: &str, cmd: Drive, knobs: &[(&str, String)]) -> Option<Rollout> {
    let bin = drive_path();
    if !bin.is_file() {
        return None;
    }
    // A distinct file per call: sweep cells run concurrently and must not read
    // one another's trajectory.
    let stamp = format!(
        "{}-{}",
        std::process::id(),
        NEXT_ROLLOUT.fetch_add(1, Ordering::Relaxed)
    );
    let json = std::env::temp_dir().join(format!("dorobot-rollout-{stamp}.json"));
    let csv = std::env::temp_dir().join(format!("dorobot-rollout-{stamp}.csv"));

    let mut c = Command::new(&bin);
    c.arg(cmd.vx.to_string())
        .arg(cmd.vy.to_string())
        .arg(cmd.yaw_rate.to_string())
        .arg(cmd.seconds.to_string())
        .arg(ckpt)
        .arg(&csv)
        .arg(&json)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    for (k, v) in knobs {
        c.env(k, v);
    }
    let ok = c.status().ok()?.success();
    let text = std::fs::read_to_string(&json).ok();
    let _ = std::fs::remove_file(&json);
    let _ = std::fs::remove_file(&csv);
    if !ok {
        return None;
    }
    parse_rollout(&text?)
}

static NEXT_ROLLOUT: AtomicU64 = AtomicU64::new(0);

/// Joint names in URDF document order — the order the viewer's loader pushes
/// them, and therefore the index space `set_joint_angles` expects.
///
/// A deliberately small reader: it takes `<joint name="…"` in file order and
/// ignores everything else, because the only thing needed here is the order.
///
/// Commented-out joints must be skipped, and that is not a nicety: the G1 URDF
/// carries `floating_base_joint` inside an XML comment, so counting it inserts
/// a phantom entry at the front and shifts every index by one — the viewer then
/// applies each angle to the neighbouring joint, which looks like a plausible
/// but wrong pose rather than an error.
pub fn urdf_joint_names(urdf: &std::path::Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(urdf) else {
        return Vec::new();
    };
    let mut names = Vec::new();
    for chunk in strip_comments(&text).split("<joint").skip(1) {
        let Some(i) = chunk.find("name=\"") else { continue };
        let rest = &chunk[i + 6..];
        let Some(end) = rest.find('"') else { continue };
        names.push(rest[..end].to_string());
    }
    names
}

/// Drop `<!-- … -->` regions, so a text scan sees what an XML parser sees.
fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find("<!--") {
        out.push_str(&rest[..open]);
        match rest[open..].find("-->") {
            Some(close) => rest = &rest[open + close + 3..],
            // An unterminated comment swallows the remainder, as XML says.
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// Read the fields this app needs out of zealot's rollout JSON.
///
/// Hand-rolled rather than pulling in serde: the `zealot` feature deliberately
/// adds no dependencies, and the document is machine-generated with a fixed
/// shape — flat arrays of numbers under known keys.
pub fn parse_rollout(text: &str) -> Option<Rollout> {
    let base2d = array2d(text, "base")?;
    let joints2d = array2d(text, "joints")?;
    Some(Rollout {
        dt: number(text, "dt").unwrap_or(0.02),
        joint_names: strings(text, "joint_names").unwrap_or_default(),
        base: base2d
            .iter()
            .filter(|r| r.len() >= 7)
            .map(|r| [r[0], r[1], r[2], r[3], r[4], r[5], r[6]])
            .collect(),
        joints: joints2d,
    })
}

/// Byte offset just past `"key":`, so a later key never matches an earlier
/// value that happens to contain the same text.
fn after_key(text: &str, key: &str) -> Option<usize> {
    let pat = format!("\"{key}\"");
    let k = text.find(&pat)? + pat.len();
    let colon = text[k..].find(':')? + k + 1;
    Some(colon)
}

fn number(text: &str, key: &str) -> Option<f32> {
    let start = after_key(text, key)?;
    let rest = text[start..].trim_start();
    let end = rest
        .find(|c: char| !(c.is_ascii_digit() || c == '-' || c == '+' || c == '.' || c == 'e' || c == 'E'))
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

fn strings(text: &str, key: &str) -> Option<Vec<String>> {
    let start = after_key(text, key)?;
    let rest = &text[start..];
    let open = rest.find('[')?;
    let close = rest[open..].find(']')? + open;
    Some(
        rest[open + 1..close]
            .split(',')
            .filter_map(|s| {
                let s = s.trim();
                s.strip_prefix('"')?.strip_suffix('"').map(str::to_string)
            })
            .collect(),
    )
}

/// The `[[…],[…]]` under `key`, as rows of numbers.
fn array2d(text: &str, key: &str) -> Option<Vec<Vec<f32>>> {
    let start = after_key(text, key)?;
    let rest = &text[start..];
    let open = rest.find('[')?;
    // Walk to the matching bracket rather than the first one, so nested rows
    // do not truncate the value.
    let mut depth = 0i32;
    let mut end = None;
    for (i, ch) in rest[open..].char_indices() {
        match ch {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    end = Some(open + i);
                    break;
                }
            }
            _ => {}
        }
    }
    let body = &rest[open + 1..end?];
    let mut rows = Vec::new();
    let mut cur = String::new();
    let mut inside = false;
    for ch in body.chars() {
        match ch {
            '[' => {
                inside = true;
                cur.clear();
            }
            ']' if inside => {
                inside = false;
                rows.push(
                    cur.split(',')
                        .filter_map(|v| v.trim().parse::<f32>().ok())
                        .collect::<Vec<f32>>(),
                );
            }
            c if inside => cur.push(c),
            _ => {}
        }
    }
    Some(rows)
}

/// Start a zealot run. `None` when the binary is absent, so the caller can fall
/// back to the built-in trainer rather than presenting a dead screen.
pub fn spawn(envs: usize, iters: u64, ckpt: &str) -> Option<Handle> {
    let bin = binary_path();
    if !bin.is_file() {
        return None;
    }

    let child = Command::new(&bin)
        .arg(iters.to_string())
        .arg(envs.to_string())
        .arg(ckpt)
        // zealot prints its metric table to stdout and its banners to stderr.
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    let shared = Arc::new(Mutex::new(Shared {
        samples: Vec::new(),
        running: true,
        envs,
        // zealot counts iterations; the step budget is only known once it
        // reports how many samples an iteration produced.
        total_steps: 0,
    }));
    let (tx, rx) = mpsc::channel();
    let s = Arc::clone(&shared);

    let mut child = child;
    let stdout = child.stdout.take()?;
    // The child outlives a naive stop: the reader blocks in `lines()`, and
    // zealot prints roughly every four seconds, so a stop checked between
    // lines arrives late — or never, if the app is closing. A watcher blocked
    // on the channel kills it immediately instead, which is what keeps a
    // window close from leaking a GPU trainer.
    let child = Arc::new(Mutex::new(child));
    let watched = Arc::clone(&child);
    thread::spawn(move || {
        // Both a Stop and a hung-up sender (the Handle dropped) mean the same
        // thing here: nobody is left who wants this run.
        let _ = rx.recv();
        if let Ok(mut c) = watched.lock() {
            let _ = c.kill();
        }
    });

    thread::spawn(move || {
        let mut pending = Row::default();
        let mut cumulative: u64 = 0;

        // No stop check here: the watcher kills the child, which closes this
        // pipe and ends the loop. One owner of that decision, not two.
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };

            // A run emits the table row first, then the `[rb]` breakdown for
            // the same iteration. Hold the row until its terms arrive so the
            // UI never sees a sample with half its fields.
            if let Some(row) = parse_row(&line) {
                pending = row;
            } else if let Some(rb) = parse_rb(&line) {
                cumulative += rb.samples;
                let sample = Sample {
                    step: cumulative,
                    reward: pending.reward,
                    terms: TERMS.iter().map(|t| rb.term(t)).collect(),
                    fall_rate: if rb.samples > 0 {
                        rb.fell as f32 / rb.samples as f32
                    } else {
                        0.0
                    },
                    steps_per_sec: if pending.secs > 0.0 {
                        rb.samples as f64 / pending.secs
                    } else {
                        0.0
                    },
                    // zealot reports terminations rather than episode length;
                    // leaving this zero is honest, where a guess would not be.
                    episode_len: 0.0,
                    leans: Vec::new(),
                };
                if let Ok(mut g) = s.lock() {
                    g.samples.push(sample);
                    g.total_steps = cumulative;
                    if g.samples.len() > 600 {
                        g.samples.drain(0..100);
                    }
                }
            }
        }

        // Reap it whether it ended on its own or the watcher killed it, so a
        // finished run leaves no zombie behind.
        if let Ok(mut c) = child.lock() {
            let _ = c.kill();
            let _ = c.wait();
        }
        if let Ok(mut g) = s.lock() {
            g.running = false;
        }
    });

    Some(Handle::external(shared, tx))
}

/// One row of zealot's metric table:
/// `iter curr step_rew falls torso_z lr kl sec`
#[derive(Default, Clone, Copy)]
struct Row {
    reward: f32,
    secs: f64,
}

fn parse_row(line: &str) -> Option<Row> {
    let f: Vec<&str> = line.split_whitespace().collect();
    if f.len() != 8 {
        return None;
    }
    // The header line has the same field count; the first field being an
    // integer is what distinguishes a data row from it.
    f[0].parse::<u64>().ok()?;
    Some(Row {
        // `NaN` parses to NaN, which is exactly what should reach the plot when
        // the sim is producing it — dropping it would hide a broken run.
        reward: f[2].parse().unwrap_or(f32::NAN),
        secs: f[7].parse().unwrap_or(0.0),
    })
}

/// zealot's per-iteration breakdown: `[rb] iter N key=value key=value …`
struct Rb {
    pairs: Vec<(String, f32)>,
    samples: u64,
    fell: u64,
}

impl Rb {
    fn term(&self, name: &str) -> f32 {
        self.pairs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| *v)
            .unwrap_or(0.0)
    }
}

fn parse_rb(line: &str) -> Option<Rb> {
    let rest = line.strip_prefix("[rb] ")?;
    let mut pairs = Vec::new();
    let (mut samples, mut fell) = (0u64, 0u64);
    for tok in rest.split_whitespace() {
        let Some((k, v)) = tok.split_once('=') else {
            continue;
        };
        match k {
            "samples" => samples = v.parse().unwrap_or(0),
            "term_fell" => fell = v.parse().unwrap_or(0),
            _ => pairs.push((k.to_string(), v.parse().unwrap_or(f32::NAN))),
        }
    }
    Some(Rb { pairs, samples, fell })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROW: &str = "   0   1.00      -0.362     6144     0.842    1.00e-3   0.0071     4.2";
    const HEAD: &str = "iter   curr   step_rew    falls   torso_z         lr       kl     sec";

    #[test]
    fn a_data_row_parses_and_the_header_does_not() {
        let r = parse_row(ROW).expect("data row should parse");
        assert!((r.reward - -0.362).abs() < 1e-6);
        assert!((r.secs - 4.2).abs() < 1e-6);
        assert!(parse_row(HEAD).is_none(), "header must not parse as data");
    }

    #[test]
    fn a_nan_reward_survives_rather_than_being_swallowed() {
        let row = ROW.replace("-0.362", "NaN");
        assert!(parse_row(&row).expect("row").reward.is_nan());
    }

    #[test]
    fn the_breakdown_yields_terms_and_counts() {
        let rb = parse_rb("[rb] iter 3 track_lin_vel=0.51 upright=0.90 \
                           action_rate=-0.01 torque_leg=-0.04 term_fell=12 samples=6144")
            .expect("rb line should parse");
        assert_eq!(rb.samples, 6144);
        assert_eq!(rb.fell, 12);
        assert!((rb.term("upright") - 0.90).abs() < 1e-6);
        // An absent term reads as zero rather than poisoning the row.
        assert_eq!(rb.term("not_a_term"), 0.0);
    }

    #[test]
    fn non_metric_output_is_ignored() {
        assert!(parse_rb("[khal] backend = WebGPU").is_none());
        assert!(parse_row("[khal] backend = WebGPU").is_none());
        assert!(parse_row("contact reduction ENABLED (per-pair merged)").is_none());
    }

    /// Shape-accurate excerpt of a real `biped_drive` rollout: same keys, same
    /// ordering, same 12 leg joints, three frames instead of 151.
    const ROLLOUT: &str = r#"{
  "dt": 0.0200,
  "names": ["pelvis", "left_hip"],
  "edges": [[0,1]],
  "feet": [5, 11],
  "resets": [],
  "joint_names": ["left_hip_pitch_joint", "left_hip_roll_joint", "left_hip_yaw_joint", "left_knee_joint", "left_ankle_pitch_joint", "left_ankle_roll_joint", "right_hip_pitch_joint", "right_hip_roll_joint", "right_hip_yaw_joint", "right_knee_joint", "right_ankle_pitch_joint", "right_ankle_roll_joint"],
  "base": [[11.34413,-0.98156,0.82200,0.00000,0.00000,0.00000,1.00000],[11.35013,-0.98150,0.82010,0.00100,0.00200,0.00000,0.99999],[11.35613,-0.98140,0.81800,0.00200,0.00400,0.00000,0.99998]],
  "joints": [[0.00000,0.00000,0.00000,0.00000,0.00000,0.00000,0.00000,0.00000,0.00000,0.00000,0.00000,0.00000],[-0.01000,0.00200,0.00000,0.02000,-0.01000,0.00100,-0.01100,-0.00200,0.00000,0.02100,-0.01000,-0.00100],[-0.02000,0.00400,0.00000,0.04000,-0.02000,0.00200,-0.02200,-0.00400,0.00000,0.04200,-0.02000,-0.00200]],
  "frames": [
    [[0.0000,0.0000,0.8220]],
    [[0.0060,0.0001,0.8201]]
  ]
}"#;

    #[test]
    fn a_rollout_parses_into_frames_the_viewer_can_use() {
        let r = parse_rollout(ROLLOUT).expect("rollout should parse");
        assert!((r.dt - 0.02).abs() < 1e-6);
        assert_eq!(r.joint_names.len(), 12);
        assert_eq!(r.joint_names[3], "left_knee_joint");
        assert_eq!(r.len(), 3);
        // Every frame carries one angle per named joint, or the viewer would
        // silently pose the robot from a short row.
        assert!(r.joints.iter().all(|f| f.len() == r.joint_names.len()));
        assert!((r.base[0][2] - 0.822).abs() < 1e-5);
        // Left knee at the last frame, and the right knee that follows it —
        // pinning the index mapping, not just the row length.
        assert!((r.joints[2][3] - 0.040).abs() < 1e-5, "left knee");
        assert!((r.joints[2][9] - 0.042).abs() < 1e-5, "right knee");
    }

    #[test]
    fn a_pose_places_angles_by_name_not_by_position() {
        let r = parse_rollout(ROLLOUT).expect("rollout");
        // A URDF whose movable joints are preceded and separated by fixed
        // ones, like the real G1.
        let urdf: Vec<String> = [
            "floating_base_joint",
            "pelvis_contour_joint",
            "left_hip_pitch_joint",
            "left_hip_roll_joint",
            "left_hip_yaw_joint",
            "left_knee_joint",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();

        let pose = r.pose(2, &urdf);
        assert_eq!(pose.len(), urdf.len(), "must cover every URDF joint");
        // The two fixed joints stay at zero rather than being handed the
        // first two leg angles.
        assert_eq!(pose[0], 0.0);
        assert_eq!(pose[1], 0.0);
        assert!((pose[2] - -0.020).abs() < 1e-5, "left_hip_pitch");
        assert!((pose[5] - 0.040).abs() < 1e-5, "left_knee");
    }

    #[test]
    fn a_non_finite_angle_does_not_reach_the_viewer() {
        let r = parse_rollout(&ROLLOUT.replace("-0.02000,0.00400", "NaN,0.00400"))
            .expect("rollout");
        let urdf = vec!["left_hip_pitch_joint".to_string()];
        assert!(r.pose(2, &urdf).iter().all(|a| a.is_finite()));
    }

    #[test]
    fn urdf_joint_names_come_back_in_document_order() {
        let dir = std::env::temp_dir().join(format!("dorobot-urdf-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("tmp dir");
        let path = dir.join("t.urdf");
        std::fs::write(
            &path,
            r#"<robot name="t">
                 <link name="a"/>
                 <joint name="j_first" type="fixed"><parent link="a"/></joint>
                 <joint name="j_second" type="revolute"><parent link="a"/></joint>
               </robot>"#,
        )
        .expect("write urdf");
        assert_eq!(urdf_joint_names(&path), vec!["j_first", "j_second"]);
        let _ = std::fs::remove_file(&path);
    }

    /// The real G1 URDF comments out `floating_base_joint`. Counting it shifts
    /// every later index by one, so each angle lands on the neighbouring joint
    /// — a wrong pose that still looks like a robot, which is why this is a
    /// test and not a comment.
    #[test]
    fn a_commented_out_joint_is_not_counted() {
        let dir = std::env::temp_dir().join(format!("dorobot-urdf-c-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("tmp dir");
        let path = dir.join("c.urdf");
        std::fs::write(
            &path,
            r#"<robot name="t">
                 <!-- <joint name="floating_base_joint" type="floating"/> -->
                 <joint name="left_hip_pitch_joint" type="revolute"/>
                 <joint name="left_hip_roll_joint" type="revolute"/>
               </robot>"#,
        )
        .expect("write urdf");

        let names = urdf_joint_names(&path);
        assert_eq!(names, vec!["left_hip_pitch_joint", "left_hip_roll_joint"]);

        // And the pose lands on the joint it names, not the one beside it.
        let r = parse_rollout(ROLLOUT).expect("rollout");
        let pose = r.pose(2, &names);
        assert!((pose[0] - -0.020).abs() < 1e-5, "hip pitch");
        assert!((pose[1] - 0.004).abs() < 1e-5, "hip roll");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn achieved_velocity_comes_from_the_base_track() {
        let r = parse_rollout(ROLLOUT).expect("rollout");
        // 11.35613 - 11.34413 = 0.012 m over two 0.02 s steps = 0.3 m/s.
        assert!((r.achieved_vx() - 0.3).abs() < 1e-3, "got {}", r.achieved_vx());
    }

    #[test]
    fn a_fall_is_detected_from_base_height_and_from_nan() {
        let r = parse_rollout(ROLLOUT).expect("rollout");
        assert!(!r.fell(0.4), "an upright rollout must not read as a fall");
        assert!(r.fell(0.9), "a floor above the base must read as a fall");

        let nan = parse_rollout(&ROLLOUT.replace("0.81800", "NaN")).expect("rollout");
        assert!(nan.fell(0.4), "a non-finite base must read as a fall");
    }

    /// The nested `frames` array follows `joints` in the document. A parser
    /// that stopped at the first `]` would truncate, so this pins the
    /// bracket-matching that prevents it.
    #[test]
    fn nested_arrays_do_not_truncate_earlier_keys() {
        let r = parse_rollout(ROLLOUT).expect("rollout");
        assert_eq!(r.joints.len(), 3, "joints must not be cut short by frames");
        assert_eq!(r.base.len(), 3);
    }

    /// Verbatim output from a real `biped_train_gpu 15 256` run on Metal,
    /// truncated only in the middle of the term list. A parser for someone
    /// else's stdout is worth exactly as much as its fidelity to the real
    /// thing, so this is copied rather than reconstructed — including the NaNs
    /// that run produced.
    #[test]
    fn real_zealot_output_parses() {
        let row = parse_row("   0   1.00        NaN     6144     0.842    1.00e-3      NaN     4.2")
            .expect("real table row should parse");
        assert!(row.reward.is_nan());
        assert!((row.secs - 4.2).abs() < 1e-6);

        let rb = parse_rb(
            "[rb] iter 0 track_lin_vel=NaN track_ang_vel=NaN upright=NaN base_height=NaN \
             pose=NaN bilateral_symmetry=NaN action_rate=0.00000 action_rate_hipz_hipx=0.00000 \
             body_ang_vel=NaN lin_vel_z=NaN dof_pos_limits=0.00000 torque_leg=NaN \
             torque_ankle=NaN self_coll=0.00000 termination=-2.00000 term_illegal=0 \
             term_fell=6144 term_timeout=0 samples=6144 terrain_level=0.500",
        )
        .expect("real breakdown line should parse");
        assert_eq!(rb.samples, 6144);
        assert_eq!(rb.fell, 6144);
        assert_eq!(rb.term("action_rate"), 0.0);
        assert!(rb.term("track_lin_vel").is_nan());
        // Every env terminating is what a fall rate of 1.0 means, and the UI
        // should be told that rather than shown a tidy zero.
        assert!((rb.fell as f32 / rb.samples as f32 - 1.0).abs() < 1e-6);
    }
}
