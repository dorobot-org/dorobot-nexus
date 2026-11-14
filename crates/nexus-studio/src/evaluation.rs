//! Durable measured evaluations and synchronized checkpoint comparisons.
use crate::{nexus::Replay, state::*, App};
use makepad_widgets::*;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
};

#[derive(Default)]
pub struct State {
    pub job: Option<Job>,
    pub report: Option<Value>,
    pub folder: Option<PathBuf>,
    pub case: usize,
    pub second: Option<Replay>,
    pub second_model: String,
}
pub struct Job {
    child: Child,
    pub output: PathBuf,
    pub status: Value,
    run: String,
    robot: String,
}
impl Drop for Job {
    fn drop(&mut self) {
        let _ = fs::write(self.output.join("cancel"), b"cancel");
        if self.child.try_wait().ok().flatten().is_none() {
            #[cfg(unix)]
            let _ = Command::new("kill")
                .args(["-TERM", "--", &format!("-{}", self.child.id())])
                .status();
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}
fn read(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    if fs::metadata(path).map_err(|e| e.to_string())?.len() > limit {
        return Err("Evaluation file exceeds its size limit".into());
    }
    fs::read(path).map_err(|e| e.to_string())
}
fn rollout(folder: &Path, case: &Value) -> Result<Replay, String> {
    let name = case["rollout"].as_str().ok_or("Missing measured rollout")?;
    if Path::new(name).file_name().and_then(|s| s.to_str()) != Some(name) {
        return Err("Invalid rollout path".into());
    }
    let path = folder
        .join(name)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if !path.starts_with(folder.canonicalize().map_err(|e| e.to_string())?) {
        return Err("Rollout escaped report folder".into());
    }
    let bytes = read(&path, 32 * 1024 * 1024)?;
    if case["rollout_sha256"].as_str() != Some(format!("{:x}", Sha256::digest(&bytes)).as_str()) {
        return Err("Measured rollout digest changed".into());
    }
    let replay: Replay = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if !crate::external_motion::valid_pose(&replay) || replay.joints.len() > 100_000 {
        return Err("Invalid measured rollout".into());
    }
    Ok(replay)
}
fn validate_report(report: &Value) -> Result<(), String> {
    let vector = |v: &Value, n| {
        v.as_array().is_some_and(|a| {
            a.len() == n && a.iter().all(|v| v.as_f64().is_some_and(f64::is_finite))
        })
    };
    let groups = report["results"]
        .as_array()
        .ok_or("Missing evaluation cases")?;
    let summaries = report["summaries"]
        .as_array()
        .ok_or("Missing evaluation summaries")?;
    let checkpoints = report["checkpoints"]
        .as_array()
        .ok_or("Missing policy identities")?;
    if !(1..=2).contains(&groups.len())
        || summaries.len() != groups.len()
        || checkpoints.len() != groups.len()
    {
        return Err("Evaluation needs matching policies, cases and summaries".into());
    }
    let count = groups[0].as_array().ok_or("Invalid case list")?.len();
    if !(1..=40).contains(&count) {
        return Err("Invalid number of evaluation trials".into());
    }
    for (i, group) in groups.iter().enumerate() {
        let cases = group
            .as_array()
            .filter(|a| a.len() == count)
            .ok_or("Unpaired evaluation trials")?;
        let summary = &summaries[i];
        if !vector(&summary["mean_velocity_rmse"], 3)
            || !vector(&summary["survival_ci95"], 2)
            || summary["total"].as_u64() != Some(count as u64)
            || !checkpoints[i]["checkpoint_sha256"]
                .as_str()
                .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            return Err("Invalid policy identity or statistics".into());
        }
        let mut survived = 0;
        let mut tracked = 0;
        for (j, case) in cases.iter().enumerate() {
            if case["case"].as_str().is_none()
                || case["seed"].as_u64().is_none()
                || !vector(&case["command"], 3)
                || !vector(&case["velocity_rmse"], 3)
                || case["survived"].as_bool().is_none()
                || case["tracking_passed"].as_bool().is_none()
                || (i > 0
                    && ["case", "seed", "command"]
                        .iter()
                        .any(|k| case[k] != groups[0][j][k]))
            {
                return Err("Invalid or unmatched measured trial".into());
            }
            survived += usize::from(case["survived"] == true);
            tracked += usize::from(case["tracking_passed"] == true);
        }
        if summary["survived"].as_u64() != Some(survived as u64)
            || summary["tracking_passed"].as_u64() != Some(tracked as u64)
        {
            return Err("Summary disagrees with measured trials".into());
        }
    }
    if groups.len() == 2 && !vector(&report["comparison"]["mean_rmse_delta"], 3) {
        return Err("Missing paired comparison statistics".into());
    }
    Ok(())
}
pub fn metric_vector(v: &Value) -> String {
    v.as_array()
        .map(|a| {
            a.iter()
                .map(|v| {
                    let n = v.as_f64().unwrap_or_default();
                    if n != 0.0 && n.abs() < 0.001 {
                        format!("{n:.2e}")
                    } else {
                        format!("{n:.3}")
                    }
                })
                .collect::<Vec<_>>()
                .join(" / ")
        })
        .unwrap_or_else(|| "—".into())
}
fn export_report(source: &Path, dest: &Path) -> Result<(), String> {
    if !dest.is_absolute() || dest.exists() || dest.starts_with(source) {
        return Err("Choose a new absolute output directory outside the report".into());
    }
    let stage = dest.with_file_name(format!(
        ".nexus-report-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    fs::create_dir(&stage).map_err(|e| e.to_string())?;
    let result = (|| {
        for entry in fs::read_dir(source).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            if path.is_symlink() {
                return Err("Report contains a linked file".into());
            }
            if path.is_file() && entry.file_name() != "cancel" {
                fs::copy(&path, stage.join(entry.file_name())).map_err(|e| e.to_string())?;
            }
        }
        if dest.exists() {
            return Err("Destination was created during export".into());
        }
        fs::rename(&stage, dest).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(stage);
    }
    result
}
impl App {
    pub fn evaluation_visible(&self) -> bool {
        self.st.mode == Mode::Validate
            && self.evaluation.report.as_ref().is_some_and(|r| {
                r["run_id"].as_str() == self.st.sel.run.as_deref()
                    && r["robot_id"].as_str() == self.st.sel.robot.as_deref()
            })
    }
    pub fn start_evaluation(
        &mut self,
        other: &str,
        seconds: &str,
        seeds: &str,
    ) -> Result<(), String> {
        if self.evaluation.job.is_some() {
            return Err("An evaluation is already running".into());
        }
        let checkpoint =
            crate::nexus::selected_artifact(&self.st).ok_or("Select a robot policy checkpoint")?;
        let run = self
            .st
            .sel
            .run
            .as_ref()
            .and_then(|id| self.st.run(id))
            .ok_or("Select the policy's run")?;
        let robot_id = self
            .st
            .snapshot_robot(&run.snapshot)
            .ok_or("Policy robot is missing or ambiguous")?;
        let robot = self
            .st
            .robots
            .iter()
            .find(|r| r.id == robot_id)
            .ok_or("Robot missing")?;
        let duration = seconds
            .trim()
            .parse::<f64>()
            .map_err(|_| "Enter evaluation duration in seconds")?;
        if !duration.is_finite() || !(0.2..=30.).contains(&duration) {
            return Err("Duration must be 0.2–30 seconds".into());
        }
        let parsed = seeds
            .split(',')
            .map(|v| v.trim().parse::<u32>())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| "Enter comma-separated integer seeds")?;
        if parsed.is_empty()
            || parsed.len() > 8
            || parsed
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len()
                != parsed.len()
        {
            return Err("Use 1–8 distinct seeds".into());
        }
        if !other.trim().is_empty() && !Path::new(other.trim()).is_file() {
            return Err("Comparison checkpoint does not exist".into());
        }
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .map_err(|e| e.to_string())?;
        let output = PathBuf::from(crate::nexus::repo_dir())
            .join("evaluations")
            .join(format!(
                "eval{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            ));
        fs::create_dir_all(output.parent().unwrap()).map_err(|e| e.to_string())?;
        let log = fs::File::options()
            .create_new(true)
            .write(true)
            .open(output.with_extension("log"))
            .map_err(|e| e.to_string())?;
        let mut cmd = Command::new(root.join("training-stack/microduck_rl/.venv/bin/python"));
        cmd.arg(root.join("tools/training/evaluate_policy.py"))
            .arg("--checkpoint")
            .arg(checkpoint)
            .arg("--output")
            .arg(&output)
            .args([
                "--run-id",
                &run.id,
                "--robot-id",
                &robot_id,
                "--robot",
                robot.model_name(),
                "--seconds",
                seconds.trim(),
                "--seeds",
                seeds.trim(),
            ])
            .stdin(Stdio::null())
            .stdout(log.try_clone().map_err(|e| e.to_string())?)
            .stderr(log);
        if !other.trim().is_empty() {
            cmd.arg("--compare").arg(other.trim());
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        let child = cmd.spawn().map_err(|e| e.to_string())?;
        self.evaluation.job = Some(Job {
            child,
            output,
            run: run.id.clone(),
            robot: robot_id.clone(),
            status: json!({"state":"starting"}),
        });
        self.evaluation.report = None;
        self.evaluation.second = None;
        self.st.sel.robot = Some(robot_id);
        self.st.mode = Mode::Validate;
        self.st.modal = AppModal::None;
        self.st
            .toast("Evaluating real policy physics on matched commands and seeds.".into());
        Ok(())
    }
    pub fn open_evaluation(&mut self, folder: PathBuf) -> Result<(), String> {
        let report: Value =
            serde_json::from_slice(&read(&folder.join("report.json"), 2 * 1024 * 1024)?)
                .map_err(|e| e.to_string())?;
        validate_report(&report)?;
        let robot_id = report["robot_id"]
            .as_str()
            .ok_or("Evaluation has no robot identity")?;
        if report["version"] != 1
            || report["status"] != "completed"
            || !self
                .st
                .robots
                .iter()
                .any(|r| r.id == robot_id && Some(r.model_name()) == report["robot"].as_str())
        {
            return Err("Evaluation robot or completion identity is invalid".into());
        }
        let run = report["run_id"]
            .as_str()
            .filter(|id| self.st.run(id).is_some())
            .ok_or("Evaluation run is not in this workspace")?;
        if self
            .st
            .run(run)
            .and_then(|r| self.st.snapshot_robot(&r.snapshot))
            .as_deref()
            != Some(robot_id)
        {
            return Err("Evaluation run belongs to another robot".into());
        }
        for group in report["results"].as_array().unwrap() {
            rollout(&folder, group.get(0).ok_or("No measured cases")?)?;
        }
        self.st.sel.robot = Some(robot_id.into());
        self.st.sel.run = Some(run.into());
        let checkpoint = report["checkpoints"][0]["checkpoint"]
            .as_str()
            .and_then(|p| Path::new(p).canonicalize().ok());
        self.st.sel.ck = self
            .st
            .run(run)
            .and_then(|r| {
                r.ckpts.iter().find(|c| {
                    checkpoint.is_some() && Path::new(&c.id).canonicalize().ok() == checkpoint
                })
            })
            .map(|c| c.id.clone());
        self.st.sel.ck_run = self.st.sel.ck.as_ref().map(|_| run.to_string());
        self.st.mode = Mode::Validate;
        self.evaluation.report = Some(report);
        self.evaluation.folder = Some(folder);
        self.evaluation.case = 0;
        self.evaluation.second_model.clear();
        self.terrain_shown.clear();
        self.select_evaluation_case(0)
    }
    pub fn select_evaluation_case(&mut self, index: usize) -> Result<(), String> {
        let report = self
            .evaluation
            .report
            .as_ref()
            .ok_or("Open an evaluation report")?;
        let folder = self
            .evaluation
            .folder
            .as_ref()
            .ok_or("Missing evaluation folder")?;
        let cases = report["results"][0]
            .as_array()
            .ok_or("Missing evaluation cases")?;
        let index = index.min(cases.len().saturating_sub(1));
        let first = rollout(folder, &cases[index])?;
        let second = if report["results"].as_array().unwrap().len() == 2 {
            Some(rollout(folder, &report["results"][1][index])?)
        } else {
            None
        };
        self.st.live.frames = first
            .joints
            .len()
            .max(second.as_ref().map(|r| r.joints.len()).unwrap_or(0))
            as u32;
        self.st.live.frame = 0;
        self.st.live.playing = false;
        self.st.replay_driven = true;
        self.replay = Some(first);
        self.replay_map = None;
        self.review = Default::default();
        self.mj = None;
        self.cross = None;
        self.evaluation.second = second;
        self.evaluation.case = index;
        Ok(())
    }
    pub fn evaluation_action(&mut self, act: &str, _arg: &str) -> Result<(), String> {
        match act {
            "eval-start" => {
                self.st.modal = AppModal::Evaluation;
                Ok(())
            }
            "eval-cancel" => {
                if let Some(job) = &self.evaluation.job {
                    fs::write(job.output.with_extension("cancel"), b"cancel")
                        .map_err(|e| e.to_string())?;
                }
                Ok(())
            }
            "eval-next" => self.select_evaluation_case(self.evaluation.case + 1),
            "eval-prev" => self.select_evaluation_case(self.evaluation.case.saturating_sub(1)),
            "eval-close" => {
                self.evaluation.report = None;
                self.evaluation.second = None;
                self.replay = None;
                self.st.live.playing = false;
                self.external_motion = Default::default();
                Ok(())
            }
            "eval-latest" => {
                let root = PathBuf::from(crate::nexus::repo_dir()).join("evaluations");
                let mut folders = fs::read_dir(root)
                    .map_err(|e| e.to_string())?
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| p.join("report.json").is_file())
                    .collect::<Vec<_>>();
                folders.sort();
                self.open_evaluation(folders.pop().ok_or("No completed evaluation reports")?)
            }
            "eval-export" => {
                self.st.modal = AppModal::EvaluationExport;
                Ok(())
            }
            _ => Err("Unknown evaluation action".into()),
        }
    }
    pub fn export_evaluation(&self, destination: &str) -> Result<(), String> {
        let source = self
            .evaluation
            .folder
            .as_ref()
            .ok_or("Open a completed evaluation")?;
        let dest = Path::new(destination.trim());
        // Export report, CSV, measured trajectories and checksums. Policy bundles
        // remain under the source report and have their own export workflow.
        export_report(source, dest)
    }
    pub fn tick_evaluation(&mut self, cx: &mut Cx) -> bool {
        let Some(job) = self.evaluation.job.as_mut() else {
            return false;
        };
        let old = job.status.clone();
        if let Ok(bytes) = read(&job.output.join("status.json"), 256 * 1024) {
            if let Ok(value) = serde_json::from_slice::<Value>(&bytes) {
                job.status = value;
            }
        }
        let exit = job.child.try_wait().ok().flatten();
        let changed = old != job.status;
        if let Some(exit) = exit {
            let job = self.evaluation.job.take().unwrap();
            if exit.success() && job.status["state"] == "completed" {
                if self.st.sel.run.as_ref() == Some(&job.run)
                    && self.st.sel.robot.as_ref() == Some(&job.robot)
                {
                    if let Err(e) = self.open_evaluation(job.output.clone()) {
                        self.st.toast(e);
                    }
                }
                self.st
                    .toast(format!("Evaluation report saved: {}", job.output.display()));
            } else {
                self.st.toast(format!(
                    "Evaluation {}: {}",
                    job.status["state"],
                    job.status["error"].as_str().unwrap_or("process stopped")
                ));
            }
            self.sync_all(cx);
            return true;
        }
        if changed {
            crate::screens::sync_right_rail(self, cx);
        }
        changed
    }
    pub fn drive_comparison(&mut self, cx: &mut Cx) {
        if !self.evaluation_visible() {
            return;
        }
        let Some(replay) = &self.evaluation.second else {
            return;
        };
        use makepad_urdf_player::robot_view::RobotViewWidgetRefExt;
        let rv = self.ui.robot_view(cx, ids!(stage.urdf_wrap.rview_b));
        let Some(view) = rv.borrow_mut() else {
            return;
        };
        let frame = (self.st.live.frame as usize).min(replay.joints.len().saturating_sub(1));
        let angles = view
            .movable_joint_names()
            .iter()
            .map(|n| {
                replay
                    .joint_names
                    .iter()
                    .position(|s| s == n)
                    .map(|i| replay.joints[frame][i])
                    .unwrap_or(0.)
            })
            .collect::<Vec<_>>();
        drop(view);
        rv.set_joint_angles(cx, &angles);
        if let Some(mut view) = rv.borrow_mut() {
            let b = replay.base[frame];
            view.set_base_pose(cx, [b[0], b[1], b[2]], [b[3], b[4], b[5], b[6]]);
            let offset = match self
                .evaluation
                .report
                .as_ref()
                .and_then(|r| r["robot"].as_str())
            {
                Some("Microduck") => 0.07,
                Some("Open Duck Mini v2") => 0.1,
                _ => 0.0,
            };
            let model = self
                .evaluation
                .report
                .as_ref()
                .and_then(|r| r["robot"].as_str())
                .unwrap_or("");
            let target = if matches!(
                model,
                "Microduck" | "Open Duck Mini v2" | "Unitree G1-29dof"
            ) {
                [
                    b[0],
                    b[1],
                    if model == "Open Duck Mini v2" {
                        (b[2] + offset).max(0.15)
                    } else {
                        b[2] + offset
                    },
                ]
            } else {
                view.robot()
                    .map(|r| {
                        let (lo, hi) = r.bounds_world();
                        let c = (lo + hi) * 0.5;
                        [c.x, c.y, c.z]
                    })
                    .unwrap_or([b[0], b[1], b[2]])
            };
            view.set_camera_target(cx, target);
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn folder(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "nexus-evaluation-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&p).unwrap();
        p
    }
    #[test]
    fn malformed_or_unpaired_reports_are_rejected_without_panicking() {
        // Synthetic schema fixture: tests must not depend on private run reports.
        let valid = json!({
            "checkpoints": [{"checkpoint_sha256": "0".repeat(64)}],
            "results": [[{
                "case": "standing", "seed": 1, "command": [0, 0, 0],
                "velocity_rmse": [0.1, 0.2, 0.3],
                "survived": true, "tracking_passed": false
            }]],
            "summaries": [{
                "mean_velocity_rmse": [0.1, 0.2, 0.3],
                "survival_ci95": [0.2, 1.0], "total": 1,
                "survived": 1, "tracking_passed": 0
            }]
        });
        validate_report(&valid).unwrap();
        for field in ["results", "summaries", "checkpoints"] {
            let mut report = valid.clone();
            report[field] = json!([]);
            assert!(validate_report(&report).is_err());
            report[field] = json!(false);
            assert!(validate_report(&report).is_err());
        }
        let mut report = valid.clone();
        report["summaries"][0]["survived"] = 99.into();
        assert!(validate_report(&report).is_err());
        let mut report = valid.clone();
        report["results"][0][0]["velocity_rmse"] = json!([0, 0]);
        assert!(validate_report(&report).is_err());
        let mut report = valid;
        let extra = report["results"][0].clone();
        report["results"].as_array_mut().unwrap().push(extra);
        assert!(validate_report(&report).is_err());
    }
    #[test]
    fn measured_rollout_digest_and_folder_containment_are_enforced() {
        let dir = folder("rollout");
        let bytes=br#"{"name":"physical","dt":0.02,"joint_names":["hip"],"joints":[[0.2]],"base":[[0,0,0.8,0,0,0,1]]}"#;
        fs::write(dir.join("case.json"), bytes).unwrap();
        let mut case =
            json!({"rollout":"case.json","rollout_sha256":format!("{:x}",Sha256::digest(bytes))});
        assert!(rollout(&dir, &case).is_ok());
        fs::write(dir.join("case.json"), b"{}").unwrap();
        assert!(rollout(&dir, &case).is_err());
        case["rollout"] = "../case.json".into();
        assert!(rollout(&dir, &case).is_err());
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn failed_report_export_preserves_existing_destination_and_cleans_staging() {
        let dir = folder("export");
        let source = dir.join("report");
        let dest = dir.join("out");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("report.json"), b"evidence").unwrap();
        fs::create_dir(&dest).unwrap();
        fs::write(dest.join("existing"), b"keep").unwrap();
        assert!(export_report(&source, &dest).is_err());
        assert_eq!(fs::read(dest.join("existing")).unwrap(), b"keep");
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("missing", source.join("linked")).unwrap();
            let failed = dir.join("new-output");
            assert!(export_report(&source, &failed).is_err());
            assert!(!failed.exists());
            assert_eq!(fs::read_dir(&dir).unwrap().count(), 2);
            fs::remove_file(source.join("linked")).unwrap();
        }
        export_report(&source, &dir.join("success")).unwrap();
        assert_eq!(
            fs::read(dir.join("success/report.json")).unwrap(),
            b"evidence"
        );
        fs::remove_dir_all(dir).unwrap();
    }
}
