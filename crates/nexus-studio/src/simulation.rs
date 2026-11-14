//! A checkpoint-bound live physics session. The worker owns physics and records every step.
use crate::{nexus::Replay, state::*, App};
use makepad_widgets::*;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub struct Session {
    child: Option<Child>,
    input: Option<ChildStdin>,
    pub output: PathBuf,
    pub run: String,
    pub robot_id: String,
    pub robot: String,
    pub checkpoint: String,
    pub digest: String,
    pub status: Value,
    expected_joints: Vec<String>,
    serial: u64,
    save_after: Option<u64>,
}

impl Session {
    pub fn send(&mut self, mut value: Value) -> Result<u64, String> {
        self.serial += 1;
        value["id"] = self.serial.into();
        let input = self
            .input
            .as_mut()
            .ok_or("Simulation process has stopped")?;
        writeln!(input, "{value}").map_err(|e| format!("Simulation control: {e}"))?;
        Ok(self.serial)
    }
    pub fn state(&self) -> &str {
        self.status["state"].as_str().unwrap_or("loading")
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // EOF asks the worker to flush. Reap away from the UI thread, including
        // a bounded fallback for a worker stuck loading an export or physics.
        self.input.take();
        if let Some(mut child) = self.child.take() {
            std::thread::spawn(move || {
                let start = Instant::now();
                while start.elapsed() < Duration::from_secs(3) {
                    if child.try_wait().ok().flatten().is_some() {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
                let _ = child.kill();
                let _ = child.wait();
            });
        }
    }
}

fn read_status(session: &Session) -> Result<Value, String> {
    let path = session.output.join("status.json");
    if fs::metadata(&path).map_err(|e| e.to_string())?.len() > 256 * 1024 {
        return Err("Simulation status exceeds 256 KiB".into());
    }
    let value: Value = serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    if value["version"] != 1
        || value["session_id"].as_str() != session.output.file_name().and_then(|s| s.to_str())
        || value["run_id"] != session.run
        || value["robot_id"] != session.robot_id
        || value["robot"] != session.robot
        || value["checkpoint_sha256"] != session.digest
    {
        return Err("Simulation identity differs from the selected checkpoint".into());
    }
    if !value["pose"].is_null() {
        let replay: Replay =
            serde_json::from_value(value["pose"].clone()).map_err(|e| e.to_string())?;
        if !crate::external_motion::valid_pose(&replay)
            || replay.joints.len() != 1
            || replay.joint_names.len() != session.expected_joints.len()
            || !session
                .expected_joints
                .iter()
                .all(|n| replay.joint_names.contains(n))
            || value["pose"]["source"] != "simulation"
            || value["pose"]["checkpoint_sha256"] != session.digest
        {
            return Err("Simulation pose does not match the selected robot".into());
        }
    }
    Ok(value)
}

impl App {
    pub fn simulation_matches_selection(&self) -> bool {
        self.simulation.as_ref().is_some_and(|s| {
            self.st.mode == Mode::Inspect
                && self.st.sel.run.as_ref() == Some(&s.run)
                && self.st.sel.robot.as_ref() == Some(&s.robot_id)
                && crate::nexus::selected_artifact(&self.st).as_ref() == Some(&s.checkpoint)
        })
    }

    pub fn release_simulation_if_selection_changed(&mut self) {
        if self.simulation.is_some() && !self.simulation_matches_selection() {
            let path = self.simulation.take().unwrap().output.clone();
            self.st.live.playing = false;
            self.replay = None;
            self.replay_map = None;
            self.st.toast(format!(
                "Simulation closed; source recording retained at {}",
                path.display()
            ));
        }
    }

    fn start_simulation(&mut self) -> Result<(), String> {
        let checkpoint =
            crate::nexus::selected_artifact(&self.st).ok_or("Select a saved checkpoint first")?;
        let mut run = self
            .st
            .sel
            .run
            .as_ref()
            .and_then(|id| self.st.run(id))
            .cloned()
            .ok_or("Select the checkpoint's run")?;
        crate::nexus::restore_run_recipe(&mut run)?;
        let robot_id = self
            .st
            .snapshot_robot(&run.snapshot)
            .ok_or("Checkpoint robot is missing or ambiguous")?;
        let robot = self
            .st
            .robots
            .iter()
            .find(|r| r.id == robot_id)
            .ok_or("Robot is not in the library")?;
        let model_name = run
            .snapshot
            .robot
            .clone()
            .ok_or("Run has no robot contract")?;
        let source = Path::new(&checkpoint)
            .canonicalize()
            .map_err(|e| e.to_string())?;
        if source.extension().is_none_or(|s| s != "pt")
            && !source
                .file_name()
                .is_some_and(|s| s.to_string_lossy().contains(".safetensors"))
        {
            return Err("Select a supported robot policy checkpoint; CPU-reference tasks are outside this workspace's supported scope".into());
        }
        let expected_joints = urdf_rs::read_file(robot.urdf.as_ref().ok_or("Robot has no URDF")?)
            .map_err(|e| e.to_string())?
            .joints
            .into_iter()
            .filter(|j| j.joint_type != urdf_rs::JointType::Fixed)
            .map(|j| j.name)
            .collect();
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .map_err(|e| e.to_string())?;
        let directory = PathBuf::from(crate::nexus::repo_dir()).join("simulations");
        fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let output = directory.join(format!("sim{stamp}"));
        let recipe = directory.join(format!("sim{stamp}-recipe.json"));
        fs::write(
            &recipe,
            serde_json::to_vec_pretty(&run.snapshot).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let file = fs::File::options()
            .create_new(true)
            .write(true)
            .open(directory.join(format!("sim{stamp}.log")))
            .map_err(|e| e.to_string())?;
        let digest = format!(
            "{:x}",
            Sha256::digest(fs::read(&source).map_err(|e| e.to_string())?)
        );
        let mut command = Command::new(root.join("training-stack/microduck_rl/.venv/bin/python"));
        command
            .arg(root.join("tools/training/interactive_worker.py"))
            .arg("--checkpoint")
            .arg(&source)
            .arg("--output")
            .arg(&output)
            .args([
                "--run-id",
                &run.id,
                "--robot",
                &model_name,
                "--robot-id",
                &robot_id,
            ])
            .arg("--recipe-file")
            .arg(recipe)
            .arg("--studio-root")
            .arg(root)
            .stdin(Stdio::piped())
            .stdout(file.try_clone().map_err(|e| e.to_string())?)
            .stderr(file);
        let mut child = command.spawn().map_err(|e| e.to_string())?;
        let input = child.stdin.take();
        self.st.sel.ck = Some(checkpoint.clone());
        self.st.sel.ck_run = Some(run.id.clone());
        self.simulation = Some(Session {
            child: Some(child),
            input,
            output,
            run: run.id,
            robot_id: robot_id.clone(),
            robot: model_name,
            checkpoint,
            digest,
            expected_joints,
            status: json!({"state":"loading","sequence":0}),
            serial: 0,
            save_after: None,
        });
        self.st.sel.robot = Some(robot_id);
        self.st.mode = Mode::Inspect;
        self.st.live.playing = false;
        self.replay = None;
        self.replay_map = None;
        self.review = Default::default();
        self.st
            .toast("Preparing checkpoint and real physics; simulation opens paused.".into());
        Ok(())
    }

    pub fn simulation_control(&mut self, value: Value) -> Result<(), String> {
        self.simulation
            .as_mut()
            .ok_or("Start live simulation first")?
            .send(value)?;
        Ok(())
    }

    pub fn simulation_action(&mut self, act: &str, arg: &str) -> Result<(), String> {
        match act {
            "sim-import" => {
                self.st.modal = AppModal::SimulationImport;
                Ok(())
            }
            "sim-start" => self.start_simulation(),
            "sim-command" => {
                self.st.modal = AppModal::SimulationCommand;
                Ok(())
            }
            "sim-push" => {
                self.st.modal = AppModal::SimulationPush;
                Ok(())
            }
            "sim-set-command" => {
                let value: Value = serde_json::from_str(arg).map_err(|e| e.to_string())?;
                self.simulation_control(json!({"op":"command","value":value}))
            }
            "sim-save" => {
                let s = self
                    .simulation
                    .as_mut()
                    .ok_or("Start live simulation first")?;
                if s.status["sequence"].as_u64().unwrap_or(0) == 0 {
                    return Err("Advance physics before saving a recording".into());
                }
                if matches!(s.state(), "stopped" | "failed") {
                    s.save_after = Some(s.status["ack"].as_u64().unwrap_or(0));
                } else {
                    s.save_after = Some(s.send(json!({"op":"pause"}))?);
                }
                Ok(())
            }
            "sim-pause" | "sim-resume" | "sim-step" | "sim-restart" | "sim-stop" => {
                self.simulation_control(json!({"op":act.strip_prefix("sim-").unwrap()}))
            }
            _ => Err("Unknown simulation action".into()),
        }
    }

    pub fn recover_simulation(&mut self, path: &str) -> Result<(), String> {
        let folder = Path::new(path.trim());
        let status_path = folder.join("status.json");
        if fs::metadata(&status_path).map_err(|e| e.to_string())?.len() > 256 * 1024 {
            return Err("Simulation status exceeds 256 KiB".into());
        }
        let status: Value =
            serde_json::from_slice(&fs::read(status_path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        let robot_id = status["robot_id"]
            .as_str()
            .ok_or("Missing simulation robot identity")?;
        let robot = self
            .st
            .robots
            .iter()
            .find(|r| r.id == robot_id && Some(r.model_name()) == status["robot"].as_str())
            .ok_or("Simulation robot is missing from this library")?;
        let run = status["run_id"].as_str().ok_or("Missing simulation run")?;
        let digest = status["checkpoint_sha256"]
            .as_str()
            .filter(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
            .ok_or("Missing checkpoint digest")?;
        if status["version"] != 1 {
            return Err("Unsupported simulation status".into());
        }
        let doc = crate::review::simulation(
            &folder.join("simulation-motion.jsonl"),
            run,
            Some(robot.id.clone()),
            robot.model_name(),
            digest,
        )?;
        if doc.samples.first().and_then(|s| s.session_id.as_deref())
            != status["session_id"].as_str()
            || doc.samples.first().and_then(|s| s.policy_sha256.as_deref())
                != status["policy_sha256"].as_str()
        {
            return Err("Simulation source differs from its captured status".into());
        }
        self.register_review(doc)?;
        self.st.modal = AppModal::None;
        self.st
            .toast("Recovered measured simulation recording.".into());
        Ok(())
    }

    pub fn tick_simulation(&mut self, cx: &mut Cx) -> bool {
        self.release_simulation_if_selection_changed();
        let Some(s) = self.simulation.as_mut() else {
            return false;
        };
        let prior = s.status.clone();
        if s.output.join("status.json").exists() {
            match read_status(s) {
                Ok(value) => s.status = value,
                Err(error) => {
                    let _ = s.send(json!({"op":"stop"}));
                    s.status = json!({"state":"failed","error":error});
                }
            }
        }
        if let Some(exit) = s.child.as_mut().and_then(|c| c.try_wait().ok().flatten()) {
            s.input.take();
            if !matches!(s.state(), "failed" | "stopped") {
                s.status["state"] = "failed".into();
                s.status["error"] = format!(
                    "Simulation process exited: {exit}; inspect {}",
                    s.output.display()
                )
                .into();
            }
        }
        let changed = prior != s.status;
        let save = s
            .save_after
            .is_some_and(|id| s.status["ack"].as_u64().unwrap_or(0) >= id);
        if save {
            s.save_after = None;
            let result = crate::review::simulation(
                &s.output.join("simulation-motion.jsonl"),
                &s.run,
                Some(s.robot_id.clone()),
                &s.robot,
                &s.digest,
            );
            // Keep the live session and source files on a failed save for retry.
            match result.and_then(|doc| self.register_review(doc)) {
                Ok(()) => {
                    self.simulation.take();
                    self.st.toast(
                        "Live trajectory saved with commands, forces and checkpoint provenance."
                            .into(),
                    );
                }
                Err(e) => self.st.toast(format!("Simulation recording: {e}")),
            }
            self.sync_all(cx);
            return true;
        }
        if changed {
            if let Ok(mut replay) = serde_json::from_value::<Replay>(s.status["pose"].clone()) {
                replay.name = format!("{} · live physics", s.robot);
                self.replay = Some(replay);
                self.replay_map = None;
                self.st.live.frame = 0;
                self.st.live.frames = 1;
                self.st.live.playing = false;
                self.st.replay_driven = true;
                self.drive_replay(cx);
            }
            crate::screens::sync_right_rail(self, cx);
            crate::screens::sync_stage_tl(self, cx);
        }
        changed
    }
}
