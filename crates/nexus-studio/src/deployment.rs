//! Native controls for an authenticated robot agent and retained deployment evidence.
use crate::{state::*, App};
use makepad_widgets::*;
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
};

#[derive(Default)]
pub struct State {
    pub session: Option<Session>,
}
pub struct Session {
    pub folder: PathBuf,
    pub target: String,
    pub robot: String,
    pub status: Value,
    sequence: u64,
    client: Option<Child>,
    agent: Option<Child>,
    config: Value,
}
fn write(path: &Path, value: &Value) -> Result<(), String> {
    let tmp = path.with_extension("tmp");
    fs::write(
        &tmp,
        serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    fs::rename(tmp, path).map_err(|e| e.to_string())
}
impl Session {
    pub fn send(&mut self, op: &str, fields: Value) -> Result<(), String> {
        self.sequence += 1;
        let mut request = fields;
        request["id"] = self.sequence.into();
        request["op"] = op.into();
        write(
            &self
                .folder
                .join(format!("command-{:08}.json", self.sequence)),
            &request,
        )
    }
    pub fn agent_state(&self) -> &str {
        self.status["agent"]["state"]
            .as_str()
            .unwrap_or("disconnected")
    }
    pub fn busy(&self) -> bool {
        self.status["state"] == "exporting"
            || self.status["ack"].as_u64().unwrap_or(0) < self.sequence
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        // Stop refreshing the GUI lease. The independent agent watchdog expires even
        // if the desktop is killed before this disconnect request can be delivered.
        let _ = self.send("disconnect", json!({}));
        let _ = fs::remove_file(self.folder.join("heartbeat"));
        // Client and local agent shut down away from the UI event thread.
        let client = self.client.take();
        let agent = self.agent.take();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(1200));
            for mut child in [client, agent].into_iter().flatten() {
                if child.try_wait().ok().flatten().is_none() {
                    let _ = Command::new("kill")
                        .arg("-TERM")
                        .arg(child.id().to_string())
                        .status();
                    for _ in 0..20 {
                        if child.try_wait().ok().flatten().is_some() {
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(50));
                    }
                    if child.try_wait().ok().flatten().is_none() {
                        let _ = child.kill();
                    }
                }
                let _ = child.wait();
            }
        });
    }
}
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}
fn process(script: &str, folder: &Path, args: &[String]) -> Result<Child, String> {
    let root = root();
    let log = fs::File::create(folder.join(format!("{script}.log"))).map_err(|e| e.to_string())?;
    let mut cmd = Command::new(root.join("training-stack/microduck_rl/.venv/bin/python"));
    cmd.arg(root.join(format!("tools/deploy/{script}.py")))
        .args(args)
        .stdin(Stdio::null())
        .stdout(log.try_clone().map_err(|e| e.to_string())?)
        .stderr(log);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    cmd.spawn().map_err(|e| e.to_string())
}
impl App {
    pub fn save_agent_target(
        &mut self,
        edit: Option<String>,
        name: &str,
        endpoint: &str,
        token: &str,
    ) -> Result<(), String> {
        if self.deployment.session.is_some() {
            return Err("Disconnect the agent before editing targets".into());
        }
        let robot = self
            .st
            .sel
            .robot
            .as_ref()
            .and_then(|id| self.st.robots.iter().find(|r| &r.id == id))
            .ok_or("Select a robot before adding its target")?;
        let endpoint = endpoint.trim();
        if endpoint != "sim://local"
            && !endpoint.starts_with("http://127.0.0.1:")
            && !endpoint.starts_with("http://localhost:")
            && !endpoint.starts_with("https://")
        {
            return Err("Use sim://local, a localhost HTTP agent through SSH, or HTTPS".into());
        }
        if endpoint != "sim://local" && !Path::new(token.trim()).is_file() {
            return Err("The agent token file does not exist".into());
        }
        if name.trim().is_empty() {
            return Err("Enter a target name".into());
        }
        let model = robot.model_name().to_string();
        let id = edit.unwrap_or_else(|| self.st.nid("tg"));
        let rev = self.st.target(&id).map(|t| t.rev + 1).unwrap_or(0);
        let target = Target {
            id: id.clone(),
            name: name.trim().into(),
            model,
            ip: endpoint.into(),
            iface: token.trim().into(),
            sdk: "nexus-agent/1".into(),
            cap: 100,
            rev,
            ping: Ping::Unprobed,
        };
        if let Some(old) = self.st.targets.iter_mut().find(|t| t.id == id) {
            *old = target;
        } else {
            self.st.targets.push(target);
        }
        self.st.dg.target = Some(id);
        self.st.modal = AppModal::None;
        Ok(())
    }
    pub fn connect_agent(&mut self) -> Result<(), String> {
        if self.deployment.session.is_some() {
            return Err("Disconnect the current agent first".into());
        }
        let target = self
            .st
            .dg
            .target
            .as_ref()
            .and_then(|id| self.st.target(id))
            .ok_or("Add or select a deployment target")?
            .clone();
        if target.sdk != "nexus-agent/1" {
            return Err(
                "Edit this legacy target with a Nexus agent endpoint and token file".into(),
            );
        }
        let robot = self
            .st
            .sel
            .robot
            .as_ref()
            .and_then(|id| self.st.robots.iter().find(|r| &r.id == id))
            .ok_or("Select a robot policy")?;
        if robot.model_name() != target.model {
            return Err("The target model differs from the selected policy robot".into());
        }
        let folder = PathBuf::from(crate::nexus::repo_dir())
            .join("deployments")
            .join(format!(
                "session{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            ));
        fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
        fs::write(folder.join("heartbeat"), b"live").map_err(|e| e.to_string())?;
        let mut config = json!({"url":target.ip,"token_file":target.iface,"robot":target.model,"target":target.id});
        let agent = if target.ip == "sim://local" {
            let token = folder.join("token");
            // Read OS randomness; token is never placed in a process argument or UI dump.
            use std::io::Read;
            let mut bytes = [0_u8; 32];
            fs::File::open("/dev/urandom")
                .and_then(|mut f| f.read_exact(&mut bytes))
                .map_err(|e| e.to_string())?;
            fs::write(
                &token,
                bytes.iter().map(|b| format!("{b:02x}")).collect::<String>(),
            )
            .map_err(|e| e.to_string())?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&token, fs::Permissions::from_mode(0o600))
                    .map_err(|e| e.to_string())?;
            }
            config["token_file"] = token.to_string_lossy().as_ref().into();
            Some(process(
                "agent",
                &folder,
                &[
                    "--adapter".into(),
                    "simulator".into(),
                    "--robot".into(),
                    target.model.clone(),
                    "--state".into(),
                    folder.join("agent").to_string_lossy().into(),
                    "--token-file".into(),
                    token.to_string_lossy().into(),
                    "--port".into(),
                    "0".into(),
                ],
            )?)
        } else {
            None
        };
        let mut session = Session {
            folder,
            target: target.id,
            robot: robot.id.clone(),
            status: json!({"state":"connecting"}),
            sequence: 0,
            client: None,
            agent,
            config,
        };
        if session.agent.is_none() {
            start_client(&mut session)?;
        }
        self.deployment.session = Some(session);
        self.st.mode = Mode::Deploy;
        Ok(())
    }
    pub fn agent_action(&mut self, act: &str, arg: &str) -> Result<(), String> {
        match act {
            "agent-connect" => self.connect_agent(),
            "agent-disconnect" => {
                self.deployment.session = None;
                Ok(())
            }
            "agent-arm" => {
                self.st.modal = AppModal::AgentArm;
                Ok(())
            }
            "agent-command" => {
                self.st.modal = AppModal::AgentCommand;
                Ok(())
            }
            "agent-prepare" => {
                let checkpoint = crate::nexus::selected_artifact(&self.st)
                    .ok_or("Select a checkpoint to prepare")?;
                let s = self
                    .deployment
                    .session
                    .as_mut()
                    .ok_or("Connect a target first")?;
                if self.st.sel.robot.as_deref() != Some(s.robot.as_str()) {
                    return Err("Selected policy robot differs from connected target".into());
                }
                s.send("prepare", json!({"checkpoint":checkpoint}))
            }
            "agent-evidence" => {
                let root = PathBuf::from(crate::nexus::repo_dir()).join("deployments");
                Command::new("open")
                    .arg(root)
                    .spawn()
                    .map_err(|e| e.to_string())?;
                Ok(())
            }
            "agent-set-command" => {
                let values = arg
                    .split(',')
                    .map(|s| s.trim().parse::<f64>())
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|_| "Enter forward, lateral and yaw velocity")?;
                if values.len() != 3 || values.iter().any(|v| !v.is_finite()) {
                    return Err("Enter three finite velocity values".into());
                }
                self.deployment
                    .session
                    .as_mut()
                    .ok_or("Connect a target first")?
                    .send("command", json!({"velocity":values}))
            }
            "agent-arm-confirm" | "agent-dry-run" | "agent-advance" | "agent-stop"
            | "agent-rollback" => {
                let op = if act == "agent-arm-confirm" {
                    "arm"
                } else {
                    act.strip_prefix("agent-").unwrap()
                };
                self.deployment
                    .session
                    .as_mut()
                    .ok_or("Connect a target first")?
                    .send(op, json!({}))
            }
            _ => Err("Unknown robot agent action".into()),
        }
    }
    pub fn tick_agent(&mut self, cx: &mut Cx) -> bool {
        let Some(s) = self.deployment.session.as_mut() else {
            return false;
        };
        let _ = fs::write(s.folder.join("heartbeat"), b"live");
        if s.client.is_none() {
            if let Ok(bytes) = fs::read(s.folder.join("agent/endpoint.json")) {
                if let Ok(value) = serde_json::from_slice::<Value>(&bytes) {
                    s.config["url"] = value["url"].clone();
                    if let Err(e) = start_client(s) {
                        s.status = json!({"state":"failed","error":e});
                    }
                }
            }
            if let Some(agent) = s.agent.as_mut() {
                if agent.try_wait().ok().flatten().is_some() {
                    s.status =
                        json!({"state":"failed","error":"Local agent exited; inspect agent.log"});
                }
            }
        }
        let old = s.status.clone();
        if let Ok(bytes) = fs::read(s.folder.join("status.json")) {
            if bytes.len() < 4 * 1024 * 1024 {
                if let Ok(status) = serde_json::from_slice::<Value>(&bytes) {
                    s.status = status;
                }
            }
        }
        if let Some(child) = s.client.as_mut() {
            if child.try_wait().ok().flatten().is_some() && s.status["state"] != "failed" {
                s.status["state"] = "disconnected".into();
            }
        }
        let changed = old != s.status;
        if self.st.mode == Mode::Deploy && self.st.sel.robot.as_deref() == Some(s.robot.as_str()) {
            let telemetry = &s.status["agent"]["telemetry"];
            if let (Some(joints), Some(names)) = (
                telemetry["joints"].as_array(),
                telemetry["joint_names"].as_array(),
            ) {
                if let Some(values) = joints.first().and_then(|v| v.as_array()) {
                    use makepad_urdf_player::robot_view::RobotViewWidgetRefExt;
                    let rv = self.ui.robot_view(cx, ids!(stage.urdf_wrap.rview));
                    if let Some(mut inner) = rv.borrow_mut() {
                        let angles = inner
                            .movable_joint_names()
                            .iter()
                            .map(|name| {
                                names
                                    .iter()
                                    .position(|n| n.as_str() == Some(name.as_str()))
                                    .and_then(|i| values.get(i))
                                    .and_then(Value::as_f64)
                                    .unwrap_or(0.) as f32
                            })
                            .collect::<Vec<_>>();
                        inner.set_joint_angles(cx, &angles);
                        if let Some(b) = telemetry["base"]
                            .get(0)
                            .and_then(|v| v.as_array())
                            .filter(|b| b.len() == 7)
                        {
                            let b = b
                                .iter()
                                .map(|v| v.as_f64().unwrap_or(0.) as f32)
                                .collect::<Vec<_>>();
                            inner.set_base_pose(cx, [b[0], b[1], b[2]], [b[3], b[4], b[5], b[6]]);
                            inner.set_camera_target(cx, [b[0], b[1], (b[2] + 0.1).max(0.15)]);
                        }
                    };
                }
            }
        }
        if changed && self.st.mode == Mode::Deploy {
            crate::screens::sync_right_rail(self, cx);
        }
        changed
    }
}
fn start_client(session: &mut Session) -> Result<(), String> {
    write(&session.folder.join("config.json"), &session.config)?;
    session.client = Some(process(
        "client",
        &session.folder,
        &["--session".into(), session.folder.to_string_lossy().into()],
    )?);
    Ok(())
}
