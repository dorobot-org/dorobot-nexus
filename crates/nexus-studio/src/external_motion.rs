//! Named physical poses from external trainers and measured checkpoint replays.
use crate::{
    nexus::{self, Replay},
    state::Mode,
    App,
};
use makepad_widgets::*;
use serde::Deserialize;
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Deserialize)]
pub struct LivePacket {
    version: u32,
    run_id: String,
    robot: String,
    source: String,
    pub sequence: u64,
    pub wall_time: f64,
    pub sim_time: f64,
    pub env_index: usize,
    pub reset_count: u64,
    pub finished: bool,
    pub command: [f64; 3],
    #[serde(flatten)]
    pub replay: Replay,
}

#[derive(Clone, Deserialize)]
pub struct Case {
    pub case: String,
    pub seed: u64,
    pub command: [f64; 3],
    pub survived: bool,
    pub tracking_pass: bool,
    pub max_tilt_deg: f64,
    pub upright_mean_velocity: [f64; 3],
    pub rollout: PathBuf,
}

#[derive(Clone, Deserialize)]
pub struct Validation {
    version: u32,
    run_id: String,
    robot: String,
    source: String,
    pub iteration: u64,
    pub report: PathBuf,
    pub survived: usize,
    pub total: usize,
    pub tracking_passed: usize,
    pub cases: Vec<Case>,
}

#[derive(Default)]
pub struct State {
    pub validations: Vec<Validation>,
    pub checkpoint: usize,
    pub case: usize,
    pub label: String,
    run: String,
    loaded: String,
    last_scan: Option<Instant>,
    expected_joints: Vec<String>,
}

fn bounded_read(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    if fs::metadata(path).map_err(|e| e.to_string())?.len() > limit {
        return Err("Motion file exceeds size limit".into());
    }
    fs::read(path).map_err(|e| e.to_string())
}

pub fn valid_pose(replay: &Replay) -> bool {
    replay.dt.is_finite()
        && replay.dt > 0.0
        && !replay.joint_names.is_empty()
        && replay.joint_names.len() <= 256
        && replay
            .joint_names
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len()
            == replay.joint_names.len()
        && !replay.joints.is_empty()
        && replay.base.len() == replay.joints.len()
        && replay
            .joints
            .iter()
            .all(|row| row.len() == replay.joint_names.len() && row.iter().all(|v| v.is_finite()))
        && replay.base.iter().all(|b| {
            b.iter().all(|v| v.is_finite())
                && (b[3..].iter().map(|v| v * v).sum::<f32>() - 1.0).abs() < 0.02
        })
}

fn same_joints(expected: &[String], actual: &[String]) -> bool {
    !expected.is_empty()
        && expected.len() == actual.len()
        && expected.iter().all(|n| actual.contains(n))
}

pub fn read_live(path: &Path, run: &str, robot: &str) -> Result<LivePacket, String> {
    let p: LivePacket =
        serde_json::from_slice(&bounded_read(path, 128 * 1024)?).map_err(|e| e.to_string())?;
    if p.version != 1
        || p.run_id != run
        || p.robot != robot
        || p.source != "training"
        || p.replay.joints.len() != 1
        || !valid_pose(&p.replay)
        || !p.wall_time.is_finite()
        || !p.sim_time.is_finite()
        || p.sim_time < 0.0
        || p.command.iter().any(|v| !v.is_finite())
    {
        return Err("Training motion identity or pose is invalid".into());
    }
    Ok(p)
}

fn inside(root: &Path, path: &Path) -> bool {
    root.canonicalize()
        .is_ok_and(|root| path.canonicalize().is_ok_and(|p| p.starts_with(root)))
}

pub fn read_validation(
    path: &Path,
    root: &Path,
    run: &str,
    robot: &str,
) -> Result<Validation, String> {
    let p: Validation =
        serde_json::from_slice(&bounded_read(path, 256 * 1024)?).map_err(|e| e.to_string())?;
    if p.version != 1
        || p.run_id != run
        || p.robot != robot
        || p.source != "validation"
        || p.cases.is_empty()
        || p.cases.len() > 100
        || p.total != p.cases.len()
        || p.survived != p.cases.iter().filter(|c| c.survived).count()
        || p.tracking_passed != p.cases.iter().filter(|c| c.tracking_pass).count()
        || !inside(root, &p.report)
        || p.cases.iter().any(|c| {
            !inside(root, &c.rollout)
                || !c.max_tilt_deg.is_finite()
                || c.command
                    .iter()
                    .chain(c.upright_mean_velocity.iter())
                    .any(|v| !v.is_finite())
        })
    {
        return Err("Validation identity, paths or measurements are invalid".into());
    }
    Ok(p)
}

impl App {
    pub fn external_robot_run(&self) -> Option<(String, String)> {
        let run = self.st.sel.run.as_ref().and_then(|id| self.st.run(id))?;
        let robot = self
            .st
            .sel
            .robot
            .as_ref()
            .and_then(|id| self.st.robots.iter().find(|r| &r.id == id))?;
        (run.set == "robot training task"
            && run
                .snapshot
                .robot_id
                .as_ref()
                .is_none_or(|id| id == &robot.id)
            && run.snapshot.robot.as_deref() == Some(robot.model_name()))
        .then(|| (run.id.clone(), robot.model_name().to_string()))
    }

    pub fn external_validation(&self) -> Option<&Validation> {
        self.external_motion
            .validations
            .get(self.external_motion.checkpoint)
    }

    pub fn tick_external_motion(&mut self, cx: &mut Cx) -> bool {
        if self.evaluation_visible() {
            return false;
        }
        if !matches!(self.st.mode, Mode::Train | Mode::Validate) {
            return false;
        }
        let Some((run, robot)) = self.external_robot_run() else {
            return false;
        };
        let root = nexus_engine::ckpt::run_dir(&run);
        if self.external_motion.run != run {
            let expected_joints = self
                .st
                .sel
                .robot
                .as_ref()
                .and_then(|id| self.st.robots.iter().find(|r| &r.id == id))
                .and_then(|r| r.urdf.as_ref())
                .and_then(|p| urdf_rs::read_file(p).ok())
                .map(|r| {
                    r.joints
                        .into_iter()
                        .filter(|j| j.joint_type != urdf_rs::JointType::Fixed)
                        .map(|j| j.name)
                        .collect()
                })
                .unwrap_or_default();
            self.external_motion = State {
                run: run.clone(),
                case: 6,
                expected_joints,
                ..Default::default()
            };
            self.replay = None;
            self.replay_map = None;
            self.st.replay_driven = false;
        }
        let mut changed = false;
        if self
            .external_motion
            .last_scan
            .is_none_or(|t| t.elapsed() >= Duration::from_secs(2))
        {
            self.external_motion.last_scan = Some(Instant::now());
            let pinned = self
                .external_validation()
                .map(|v| v.iteration)
                .filter(|_| self.external_motion.checkpoint > 0);
            let mut manifests = Vec::new();
            let latest = root.join("validation-preview.json");
            if latest.is_file() {
                manifests.push(latest);
            }
            if let Ok(dirs) = fs::read_dir(root.join("evaluations")) {
                manifests.extend(
                    dirs.flatten()
                        .map(|d| d.path().join("nexus-validation.json"))
                        .filter(|p| p.is_file()),
                );
            }
            let mut validations: Vec<_> = manifests
                .iter()
                .filter_map(|p| read_validation(p, &root, &run, &robot).ok())
                .collect();
            validations.sort_by_key(|v| std::cmp::Reverse(v.iteration));
            validations.dedup_by_key(|v| v.iteration);
            changed = validations.iter().map(|v| v.iteration).collect::<Vec<_>>()
                != self
                    .external_motion
                    .validations
                    .iter()
                    .map(|v| v.iteration)
                    .collect::<Vec<_>>();
            self.external_motion.checkpoint = pinned
                .and_then(|i| validations.iter().position(|v| v.iteration == i))
                .unwrap_or(0);
            self.external_motion.validations = validations;
        }
        let label;
        if self.st.mode == Mode::Train {
            match read_live(&root.join("live-motion.json"), &run, &robot).and_then(|p| {
                if same_joints(&self.external_motion.expected_joints, &p.replay.joint_names) {
                    Ok(p)
                } else {
                    Err("Live joint names do not match the selected URDF".into())
                }
            }) {
                Ok(mut packet) => {
                    let age = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs_f64()
                        - packet.wall_time;
                    let state = if packet.finished {
                        "Training motion ended"
                    } else if age > 3.0 {
                        "Waiting for training poses"
                    } else {
                        "Live training"
                    };
                    label = format!(
                        "{state} · env {} · sim {:.2}s · {} resets",
                        packet.env_index, packet.sim_time, packet.reset_count
                    );
                    let key = format!("live:{run}:{}", packet.sequence);
                    if self.external_motion.loaded != key {
                        packet.replay.name = label.clone();
                        self.replay = Some(packet.replay);
                        self.replay_map = None;
                        self.st.replay_driven = true;
                        self.st.live.frames = 1;
                        self.st.live.frame = 0;
                        self.st.live.playing = false; // live frames are driven by the publisher
                        self.external_motion.loaded = key;
                        self.drive_replay(cx);
                        changed = true;
                    }
                }
                Err(e) => {
                    label = if root.join("live-motion.json").exists() {
                        format!("Live motion unavailable: {e}")
                    } else {
                        "Waiting for live training poses · use Validate for saved checkpoints"
                            .into()
                    };
                    if self.external_motion.loaded.starts_with("validation:") {
                        self.replay = None;
                        self.replay_map = None;
                        self.st.replay_driven = false;
                        self.st.live.frames = 0;
                        self.external_motion.loaded.clear();
                    }
                }
            }
        } else if let Some(validation) = self.external_validation().cloned() {
            self.external_motion.case = self.external_motion.case.min(validation.cases.len() - 1);
            let case = &validation.cases[self.external_motion.case];
            label = format!(
                "Validation replay · checkpoint {} · {} / seed {} · {}",
                validation.iteration,
                case.case,
                case.seed,
                if case.tracking_pass { "PASS" } else { "FAIL" }
            );
            let key = format!(
                "validation:{run}:{}:{}",
                validation.iteration, self.external_motion.case
            );
            if self.external_motion.loaded != key {
                match nexus::load_rollout(&case.rollout, &label).filter(|p| {
                    valid_pose(p)
                        && same_joints(&self.external_motion.expected_joints, &p.joint_names)
                }) {
                    Some(replay) => {
                        self.st.live.frames = replay.joints.len() as u32;
                        self.st.live.frame = 0;
                        self.st.live.playing = true;
                        self.st.replay_driven = true;
                        self.replay = Some(replay);
                        self.replay_map = None;
                        self.external_motion.loaded = key;
                        self.drive_replay(cx);
                        changed = true;
                    }
                    None => {
                        self.external_motion.loaded = key;
                        self.replay = None;
                        self.replay_map = None;
                        self.st.live.frames = 0;
                        self.st
                            .toast("Validation rollout failed pose checks".into());
                    }
                }
            }
        } else {
            label = "No completed checkpoint validation for this run".into();
            if self.external_motion.loaded.starts_with("live:") {
                self.replay = None;
                self.replay_map = None;
                self.st.replay_driven = false;
                self.st.live.frames = 0;
                self.external_motion.loaded.clear();
            }
        }
        changed |= self.external_motion.label != label;
        self.external_motion.label = label;
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[test]
    fn validation_paths_accept_symlinked_roots_but_reject_escape() {
        let dir = std::env::temp_dir().join(format!("nexus-path-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("actual")).unwrap();
        std::fs::write(dir.join("actual/report.md"), "report").unwrap();
        std::fs::write(dir.join("outside.md"), "outside").unwrap();
        std::os::unix::fs::symlink(dir.join("actual"), dir.join("alias")).unwrap();
        assert!(inside(&dir.join("alias"), &dir.join("actual/report.md")));
        assert!(!inside(&dir.join("alias"), &dir.join("outside.md")));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn named_pose_contract_checks_orientation_dimensions_and_duplicates() {
        let mut p = Replay {
            name: "test".into(),
            joint_names: vec!["hip".into()],
            joints: vec![vec![0.1]],
            base: vec![[0., 0., 0.12, 0., 0., 0., 1.]],
            dt: 0.02,
        };
        assert!(valid_pose(&p));
        assert!(same_joints(&["hip".into()], &p.joint_names));
        assert!(!same_joints(&["ankle".into()], &p.joint_names));
        assert!(same_joints(
            &["a".into(), "b".into()],
            &["b".into(), "a".into()]
        ));
        p.base[0][6] = 0.;
        assert!(!valid_pose(&p));
        p.base[0][6] = 1.;
        p.joints[0][0] = f32::NAN;
        assert!(!valid_pose(&p));
        p.joints[0] = vec![0.1, 0.2];
        p.joint_names.push("hip".into());
        assert!(!valid_pose(&p));
    }
    #[test]
    fn live_packet_rejects_wrong_robot_run_and_preserves_xyzw() {
        let root = std::env::temp_dir().join(format!("nexus-live-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("live.json");
        fs::write(&path, r#"{"version":1,"run_id":"r1","robot":"Microduck","source":"training","sequence":1,"wall_time":1.0,"sim_time":0.02,"env_index":0,"reset_count":0,"finished":false,"command":[0.2,0,0],"dt":0.02,"joint_names":["hip"],"joints":[[0.1]],"base":[[0,0,0.12,0,0,0,1]]}"#).unwrap();
        let p = read_live(&path, "r1", "Microduck").unwrap();
        assert_eq!(p.replay.base[0][6], 1.0);
        assert!(read_live(&path, "r2", "Microduck").is_err());
        assert!(read_live(&path, "r1", "G1").is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn validation_requires_matching_measurements_and_files_inside_its_run() {
        let root =
            std::env::temp_dir().join(format!("nexus-validation-contract-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let report = root.join("report.md");
        let rollout = root.join("rollout.json");
        fs::write(&report, "measured report").unwrap();
        fs::write(&rollout, "{}").unwrap();
        let path = root.join("manifest.json");
        let mut value = serde_json::json!({"version":1,"run_id":"r1","robot":"Microduck","source":"validation",
            "iteration":1000,"report":report,"survived":1,"total":1,"tracking_passed":0,
            "cases":[{"case":"forward","seed":0,"command":[0.2,0,0],"survived":true,"tracking_pass":false,
                "max_tilt_deg":2.0,"upright_mean_velocity":[0.001,0,0],"rollout":rollout}]});
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(read_validation(&path, &root, "r1", "Microduck").is_ok());
        value["tracking_passed"] = 1.into();
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(read_validation(&path, &root, "r1", "Microduck").is_err());
        value["tracking_passed"] = 0.into();
        value["cases"][0]["rollout"] = "/etc/hosts".into();
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(read_validation(&path, &root, "r1", "Microduck").is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
