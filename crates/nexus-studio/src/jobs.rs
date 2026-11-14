//! Durable queue entries and monitoring of independent local/SSH supervisors.
use crate::{state::*, App};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::{BufRead, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn source_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}
fn python() -> PathBuf {
    source_root().join("training-stack/microduck_rl/.venv/bin/python")
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub transport: String,
    pub host: String,
    pub port: u16,
    pub identity: String,
    pub known_hosts: String,
    pub remote_repo: String,
    pub remote_root: String,
    pub python: String,
    pub studio_binary: String,
    pub delay_minutes: u64,
    pub processes: u32,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            transport: "local".into(),
            host: String::new(),
            port: 22,
            identity: String::new(),
            known_hosts: String::new(),
            remote_repo: String::new(),
            remote_root: String::new(),
            python: "python3".into(),
            studio_binary: String::new(),
            delay_minutes: 0,
            processes: 1,
        }
    }
}
impl Settings {
    pub fn validate(&self) -> Result<(), String> {
        if !(1..=64).contains(&self.processes) {
            return Err("Choose 1–64 learner processes on one training host".into());
        }
        if !["local", "ssh"].contains(&self.transport.as_str()) || self.delay_minutes > 525600 {
            return Err("Choose local or ssh and a delay up to one year".into());
        }
        if self.transport == "ssh" {
            if self.host.is_empty()
                || self.host.starts_with('-')
                || self
                    .host
                    .chars()
                    .any(|c| c.is_whitespace() || c.is_control())
                || self.port == 0
            {
                return Err("Enter a valid SSH host and port".into());
            }
            for (name, path) in [
                ("Remote repository", &self.remote_repo),
                ("Remote run storage", &self.remote_root),
                ("Remote Nexus executable", &self.studio_binary),
            ] {
                if !Path::new(path).is_absolute() {
                    return Err(format!("{name} must be absolute"));
                }
            }
            if self.python.is_empty() || self.python.contains('\0') {
                return Err("Enter the remote Python executable".into());
            }
            for path in [&self.identity, &self.known_hosts] {
                if !path.is_empty() && !Path::new(path).is_file() {
                    return Err(format!("SSH configuration file is missing: {path}"));
                }
            }
        }
        Ok(())
    }
}

fn parallel_profile(run: &mut Run, settings: &mut Settings) -> Result<(), String> {
    if let Some(n) = run
        .snapshot
        .training_profile
        .as_ref()
        .and_then(|p| p.pointer("/parallel/processes"))
        .and_then(|n| n.as_u64())
    {
        settings.processes = u32::try_from(n).map_err(|_| "Invalid captured process count")?;
        settings.validate()?;
        // Continuation and reproduction preserve the numerical training recipe.
        return Ok(());
    }
    if settings.processes == 1 {
        return Ok(());
    }
    let profile = run.snapshot.training_profile.as_mut()
        .ok_or("Parallel PPO requires an authored robot task; the native G1 and CPU reference trainers use one learner")?;
    let kind = profile
        .pointer("/task_config/kind")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if !["generic_mujoco", "open_duck", "microduck"].contains(&kind) {
        return Err("Select an authored MuJoCo or duck task for parallel PPO".into());
    }
    let device = profile
        .pointer("/task_config/ppo/device")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if !["cpu", "cuda"].contains(&device) {
        return Err("Parallel PPO requires CPU or one CUDA device per learner. Keep one learner for Apple MPS.".into());
    }
    let original = profile["args"]
        .as_array()
        .ok_or("Trainer arguments missing")?
        .clone();
    let script = original
        .first()
        .and_then(|v| v.as_str())
        .ok_or("Trainer script missing")?;
    let mut args = vec![
        serde_json::json!(source_root().join("tools/training/parallel_worker.py")),
        "--processes".into(),
        settings.processes.to_string().into(),
        "--worker".into(),
        script.into(),
        "--".into(),
    ];
    args.extend(original.into_iter().skip(1));
    let local_envs = profile
        .pointer("/task_config/ppo/num_envs")
        .and_then(|v| v.as_u64())
        .ok_or("Environment count missing")?;
    profile["args"] = args.into();
    profile["recipe"]["num_envs"] = (local_envs * settings.processes as u64).into();
    profile["parallel"] = serde_json::json!({"processes":settings.processes,"num_envs_per_process":local_envs,"scope":"one host, shared PPO policy"});
    Ok(())
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Job {
    pub token: String,
    pub request: String,
    pub request_sha256: String,
    pub settings: Settings,
    pub queued_at: u64,
    pub not_before: u64,
    pub order: u64,
    #[serde(default)]
    pub last_contact: Option<u64>,
    #[serde(default)]
    pub connection_error: Option<String>,
}
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Status {
    pub state: String,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub worker_pid: Option<u32>,
    #[serde(default)]
    pub supervisor_pid: Option<u32>,
    #[serde(default)]
    pub ack: Option<String>,
}
#[derive(Clone, Debug, Default)]
pub struct Update {
    pub status: Option<Status>,
    pub error: Option<String>,
    pub at: u64,
}

fn write(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let temp = path.with_extension("job-tmp");
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    let mut file = fs::File::create(&temp).map_err(|e| e.to_string())?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())?;
    fs::rename(temp, path).map_err(|e| e.to_string())
}
fn files(root: &Path, out: &mut BTreeMap<String, String>, base: &Path) -> Result<(), String> {
    for entry in fs::read_dir(root).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let p = entry.path();
        let ty = entry.file_type().map_err(|e| e.to_string())?;
        if ty.is_symlink() {
            return Err("Captured inputs cannot contain links".into());
        }
        if ty.is_dir() {
            files(&p, out, base)?;
        } else if ty.is_file() {
            out.insert(
                p.strip_prefix(base).unwrap().to_string_lossy().into(),
                hash(&fs::read(p).map_err(|e| e.to_string())?),
            );
        }
    }
    Ok(())
}
/// Capture model assets and warm-start inputs before the workspace commits a queue entry.
pub fn prepare(run: &mut Run, settings: &Settings, root: &Path) -> Result<(), String> {
    prepare_with_python(run, settings, root, &python())
}

fn prepare_with_python(
    run: &mut Run,
    settings: &Settings,
    root: &Path,
    python: &Path,
) -> Result<(), String> {
    settings.validate()?;
    let mut settings = settings.clone();
    let mut worker = run.clone();
    parallel_profile(&mut worker, &mut settings)?;
    if !python.is_file() {
        return Err("Install the training Python environment first".into());
    }
    fs::create_dir(root).map_err(|e| format!("Choose a fresh run directory: {e}"))?;
    let canonical = root.canonicalize().map_err(|e| e.to_string())?;
    let root = canonical.as_path();
    let inputs = root.join("inputs");
    fs::create_dir(&inputs).map_err(|e| e.to_string())?;
    worker.job = None;
    let mut source_checks = BTreeMap::<String, String>::new();
    if let Some(profile) = worker.snapshot.training_profile.as_mut() {
        if let Some(path) = profile
            .pointer("/task_config/model")
            .and_then(|v| v.as_str())
            .map(str::to_owned)
        {
            let model = crate::robot_import::import(Path::new(&path), "model", &inputs)?;
            profile["task_config"]["model"] = model.urdf.unwrap().into();
        }
        if let Some(args) = profile["args"].as_array() {
            for path in args
                .iter()
                .filter_map(|v| v.as_str())
                .map(Path::new)
                .filter(|p| p.extension().is_some_and(|e| e == "py") && p.is_file())
            {
                let parent = path.parent().unwrap();
                for e in fs::read_dir(parent).map_err(|e| e.to_string())? {
                    let p = e.map_err(|e| e.to_string())?.path();
                    if p.extension().is_some_and(|e| e == "py")
                        && !p
                            .file_name()
                            .unwrap()
                            .to_string_lossy()
                            .starts_with("test_")
                    {
                        source_checks.insert(
                            p.to_string_lossy().into(),
                            hash(&fs::read(p).map_err(|e| e.to_string())?),
                        );
                    }
                }
            }
        }
    }
    if let Some(warm) = worker.snapshot.warm.clone().filter(|s| s != "Fresh") {
        let source = Path::new(&warm);
        let target = inputs.join(source.file_name().ok_or("Invalid warm-start path")?);
        fs::copy(source, &target).map_err(|e| e.to_string())?;
        for filename in ["model.mjz", "model.mjb", "model-source.mjz", "task.json"] {
            if let Some(p) = [
                source.parent().unwrap().join(filename),
                source
                    .parent()
                    .unwrap()
                    .parent()
                    .unwrap_or(Path::new("."))
                    .join(filename),
            ]
            .into_iter()
            .find(|p| p.is_file())
            {
                fs::copy(p, inputs.join(filename)).map_err(|e| e.to_string())?;
            }
        }
        if let Some(profile) = worker.snapshot.training_profile.as_mut() {
            if let Some(args) = profile["args"].as_array_mut() {
                for arg in args {
                    if arg.as_str() == Some(&warm) {
                        *arg = target.to_string_lossy().into_owned().into();
                    }
                }
            }
            profile["resume_checkpoint"] = target.to_string_lossy().into_owned().into();
        }
        worker.snapshot.warm = Some(target.to_string_lossy().into());
    }
    // The captured worker recipe, distinct from the UI ledger entry, never changes.
    run.snapshot = worker.snapshot.clone();
    worker.state = RunState::Running;
    write(&root.join("job-worker.json"), &worker)?;
    let mut digests = BTreeMap::new();
    files(&inputs, &mut digests, root)?;
    digests.insert(
        "job-worker.json".into(),
        hash(&fs::read(root.join("job-worker.json")).map_err(|e| e.to_string())?),
    );
    fs::create_dir(root.join(".job")).map_err(|e| e.to_string())?;
    let token = format!(
        "{}-{}",
        run.id,
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let request = serde_json::json!({"id":run.id,"settings":settings,"local_root":root,"source_root":source_root(),"profile_python":worker.snapshot.training_profile.as_ref().and_then(|p|p["program"].as_str()),
        "spec":{"version":1,"token":token,"command":[std::env::current_exe().map_err(|e|e.to_string())?,"--job-worker",root.join("job-worker.json")],"cwd":source_root(),"env":{"NEXUS_RUNS_DIR":root.parent(),"DOROBOT_NEXUS_DIR":source_root()},"inputs":digests,"source_checks":source_checks}});
    let request_path = root.join("job-request.json");
    write(&request_path, &request)?;
    let bytes = fs::read(&request_path).map_err(|e| e.to_string())?;
    run.job = Some(Job {
        token,
        request: request_path.to_string_lossy().into(),
        request_sha256: hash(&bytes),
        settings: settings.clone(),
        queued_at: now(),
        not_before: now() + settings.delay_minutes * 60,
        order: now() * 1_000_000
            + SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .subsec_micros() as u64,
        last_contact: None,
        connection_error: None,
    });
    run.state = RunState::Queued;
    run.diagnosis = None;
    write(&root.join("run.json"), run)?;
    Ok(())
}
fn request(job: &Job) -> Result<serde_json::Value, String> {
    let bytes = fs::read(&job.request).map_err(|e| e.to_string())?;
    if hash(&bytes) != job.request_sha256 {
        return Err("Queued job specification changed; launch refused".into());
    }
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}
fn rpc(doc: &serde_json::Value, op: &str, command: Option<&str>) -> Result<Status, String> {
    let mut req = serde_json::json!({"job":doc,"op":op});
    if let Some(action) = command {
        req["action"] = action.into();
        req["request_id"] = format!(
            "{}-{action}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )
        .into();
    }
    let mut child = Command::new(python())
        .arg(source_root().join("tools/jobs/client.py"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&req).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).map_err(|e| {
        format!(
            "Job transport: {e}; {}",
            String::from_utf8_lossy(&output.stderr)
        )
    })?;
    if value["ok"] != true {
        return Err(value["error"]
            .as_str()
            .unwrap_or("Job transport failed")
            .into());
    }
    let status: Status =
        serde_json::from_value(value["result"].clone()).map_err(|e| e.to_string())?;
    Ok(status)
}

pub fn monitor(run: &Run, submit: bool) -> Result<crate::nexus::RealProc, String> {
    let job = run.job.clone().ok_or("Run has no job supervisor")?;
    let doc = request(&job)?;
    let root = PathBuf::from(
        doc["local_root"]
            .as_str()
            .ok_or("Job has no local directory")?,
    );
    let update = Arc::new(Mutex::new(Update::default()));
    let shared = Arc::new(Mutex::new(nexus_engine::trainer::Shared {
        running: true,
        ..Default::default()
    }));
    {
        let mut g = shared.lock().unwrap();
        g.envs = run
            .snapshot
            .training_profile
            .as_ref()
            .and_then(|p| p["recipe"]["num_envs"].as_u64())
            .unwrap_or(if run.snapshot.backend.as_deref() == Some("cpu") {
                256
            } else {
                256
            }) as usize;
        g.total_steps = run
            .snapshot
            .training_profile
            .as_ref()
            .and_then(|p| p["recipe"]["steps_per_env"].as_u64())
            .unwrap_or(32)
            * g.envs as u64
            * run.iters_per as u64;
    }
    let done = Arc::new(AtomicBool::new(false));
    let (logtx, logrx) = mpsc::channel();
    let (cmdtx, cmdrx) = mpsc::channel();
    let (state, measurements, fin) = (update.clone(), shared.clone(), done.clone());
    std::thread::spawn(move || {
        let mut first = submit;
        let mut last_step = 0;
        let mut last_error = String::new();
        let mut log_offset = 0;
        let mut action = None;
        loop {
            match cmdrx.try_recv() {
                Ok(nexus_engine::trainer::Cmd::Stop) => action = Some("stop"),
                Ok(nexus_engine::trainer::Cmd::Pause) => action = Some("pause"),
                Ok(nexus_engine::trainer::Cmd::Resume) => action = Some("resume"),
                Err(mpsc::TryRecvError::Disconnected) => return,
                _ => {}
            }
            let op = if first && action == Some("stop") {
                "cancel"
            } else if action.is_some() {
                "control"
            } else if first {
                "submit"
            } else {
                "status"
            };
            match rpc(&doc, op, action) {
                Ok(status) => {
                    first = false;
                    action = None;
                    last_error.clear();
                    let terminal =
                        matches!(status.state.as_str(), "Completed" | "Failed" | "Stopped");
                    if let Ok(mut file) = fs::File::open(root.join("job-events.jsonl")) {
                        let _ = file.seek(SeekFrom::Start(log_offset));
                        let mut reader = std::io::BufReader::new(file);
                        loop {
                            let mut line = String::new();
                            let Ok(n) = reader.read_line(&mut line) else {
                                break;
                            };
                            if n == 0 || !line.ends_with('\n') {
                                break;
                            }
                            log_offset += n as u64;
                            match crate::nexus::external_sample(&line) {
                                Ok(Some(s)) if s.step > last_step => {
                                    last_step = s.step;
                                    let _ = logtx
                                        .send(format!("step {} · reward {:.4}", s.step, s.reward));
                                    let mut g = measurements.lock().unwrap();
                                    g.samples.push(s);
                                    let excess = g.samples.len().saturating_sub(600);
                                    g.samples.drain(..excess);
                                }
                                _ => {}
                            }
                        }
                    }
                    {
                        let mut g = measurements.lock().unwrap();
                        g.paused = status.state == "Paused";
                        if terminal {
                            g.running = false;
                            g.error = if status.state == "Failed" {
                                Some(status.error.clone().unwrap_or("Worker failed".into()))
                            } else {
                                None
                            };
                        }
                    }
                    *state.lock().unwrap() = Update {
                        status: Some(status),
                        error: None,
                        at: now(),
                    };
                    if terminal {
                        fin.store(true, Ordering::Relaxed);
                        return;
                    }
                }
                Err(error) => {
                    if last_error != error {
                        let _ = logtx.send(format!("Job connection: {error}"));
                        last_error = error.clone();
                    }
                    state.lock().unwrap().error = Some(error);
                    // A failed observation never proves the worker stopped. Retry the same identity.
                }
            }
            std::thread::sleep(Duration::from_millis(750));
        }
    });
    let mut handle = nexus_engine::trainer::Handle::external(shared, cmdtx);
    handle.can_pause = true;
    let artifact = run
        .snapshot
        .training_profile
        .as_ref()
        .and_then(|p| p["checkpoint"].as_str())
        .map(|s| {
            s.replace(
                "{run_dir}",
                &crate::artifacts::runs_root()
                    .join(&run.id)
                    .to_string_lossy(),
            )
        })
        .or_else(|| {
            Some(
                crate::artifacts::runs_root()
                    .join(&run.id)
                    .join(if run.snapshot.backend.as_deref() == Some("cpu") {
                        "policy.bin"
                    } else {
                        "policy.safetensors"
                    })
                    .to_string_lossy()
                    .into(),
            )
        });
    Ok(crate::nexus::RealProc {
        kind: crate::nexus::ProcKind::Train,
        run_id: Some(run.id.clone()),
        artifact,
        backend: format!(
            "{} · {} supervised job",
            if run.snapshot.training_profile.is_some() {
                "External"
            } else if run.snapshot.backend.as_deref() == Some("cpu") {
                "CPU"
            } else {
                "Zealot"
            },
            job.settings.transport
        ),
        rx: logrx,
        done,
        surface: None,
        train: Some(handle),
        job_update: Some(update),
    })
}

/// Worker entry point runs the same engine outside the GUI process.
pub fn worker(path: &Path) -> Result<(), String> {
    let run: Run = serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let job = crate::nexus::run_train(2_000_000, &run)?;
    let handle = job.train.as_ref().ok_or("Training handle missing")?;
    let mut last = 0;
    loop {
        let state = handle.shared.lock().map_err(|e| e.to_string())?;
        for s in state
            .samples
            .iter()
            .filter(|s| s.step > last)
            .cloned()
            .collect::<Vec<_>>()
        {
            println!(
                "{}",
                serde_json::json!({"event":"sample","step":s.step,"reward":s.reward,"fall_rate":s.fall_rate,"fall_rate_measured":!s.fall_rate_unmeasured,"steps_per_sec":s.steps_per_sec,"episode_len":s.episode_len})
            );
            last = s.step;
        }
        let _ = std::io::stdout().flush();
        if !state.running {
            if let Some(error) = &state.error {
                return Err(error.clone());
            }
            break;
        }
        drop(state);
        std::thread::sleep(Duration::from_millis(100));
    }
    if last == 0 {
        return Err("Worker finished without measured training steps".into());
    }
    println!("{}", serde_json::json!({"event":"complete"}));
    Ok(())
}

impl App {
    pub fn enqueue_selected(&mut self) -> Result<(), String> {
        let id = self.st.sel.run.clone().ok_or("Select a run")?;
        let mut run = self.st.run(&id).ok_or("Run missing")?.clone();
        prepare(
            &mut run,
            &self.st.job_settings,
            &crate::artifacts::runs_root().join(&id),
        )?;
        let previous = self.st.run(&id).unwrap().clone();
        *self.st.run_mut(&id).unwrap() = run;
        if let Err(e) = self
            .workspace_session
            .as_ref()
            .ok_or("Workspace is read-only")?
            .save(&self.st)
        {
            *self.st.run_mut(&id).unwrap() = previous;
            return Err(e.to_string());
        }
        self.st
            .toast("Training queued with captured model, recipe and launch settings".into());
        Ok(())
    }
    pub fn tick_queue(&mut self) {
        if self.st.demo || self.workspace_session.is_none() || self.real_proc.is_some() {
            return;
        }
        let active = self.st.runs.iter().find(|r| {
            r.job.is_some()
                && matches!(
                    r.state,
                    RunState::Running
                        | RunState::Paused
                        | RunState::Starting
                        | RunState::Stopping
                        | RunState::Unknown
                )
        });
        let next = active
            .or_else(|| {
                if self.st.queue_enabled {
                    self.st
                        .runs
                        .iter()
                        .filter(|r| {
                            r.state == RunState::Queued
                                && r.job.as_ref().is_some_and(|j| j.not_before <= now())
                        })
                        .min_by_key(|r| r.job.as_ref().unwrap().order)
                } else {
                    None
                }
            })
            .cloned();
        if let Some(run) = next {
            let submit = run.state == RunState::Queued;
            match monitor(&run, submit) {
                Ok(job) => {
                    if submit {
                        self.st.run_mut(&run.id).unwrap().state = RunState::Starting;
                    }
                    self.real_proc = Some(job);
                    self.train_seen = run.metrics.last().map(|s| s.step as usize).unwrap_or(0);
                    self.real_lines.clear();
                }
                Err(error) => {
                    let r = self.st.run_mut(&run.id).unwrap();
                    // A damaged recovery record does not prove an existing worker exited.
                    r.state = if submit {
                        RunState::Failed
                    } else {
                        RunState::Unknown
                    };
                    let changed = r.diagnosis.as_ref() != Some(&error);
                    r.diagnosis = Some(error.clone());
                    if let Some(job) = r.job.as_mut() {
                        job.connection_error = Some(error.clone());
                    }
                    if changed {
                        self.st.toast(error);
                    }
                }
            }
        }
    }
    pub fn queue_action(&mut self, act: &str, arg: &str) -> Result<(), String> {
        match act {
            "queue-settings" => {
                self.st.modal = AppModal::QueueSettings {
                    page: 0,
                    draft: self.st.job_settings.clone(),
                }
            }
            "queue-toggle" => self.st.queue_enabled = !self.st.queue_enabled,
            "queue-cancel" => {
                let r = self.st.run_mut(arg).ok_or("Run missing")?;
                if r.state != RunState::Queued {
                    return Err(
                        "Only queued jobs can be cancelled here; stop an active worker".into(),
                    );
                }
                r.state = RunState::Stopped;
                r.diagnosis = Some("Cancelled before launch".into());
            }
            "queue-first" => {
                let first = self
                    .st
                    .runs
                    .iter()
                    .filter_map(|r| r.job.as_ref().map(|j| j.order))
                    .min()
                    .unwrap_or(1);
                let r = self.st.run_mut(arg).ok_or("Run missing")?;
                if r.state != RunState::Queued {
                    return Err("Only queued jobs can be reordered".into());
                }
                r.job.as_mut().unwrap().order = first.saturating_sub(1);
            }
            "queue-now" => {
                let r = self.st.run_mut(arg).ok_or("Run missing")?;
                if r.state != RunState::Queued {
                    return Err("Only queued jobs can be scheduled".into());
                }
                r.job.as_mut().unwrap().not_before = now();
            }
            _ => return Err("Unknown queue action".into()),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn temp() -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "nexus-job-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&p).unwrap();
        p
    }
    #[test]
    fn queue_captures_model_and_recipe_and_rejects_request_tampering() {
        let root = temp();
        let model = root.join("robot.urdf");
        fs::write(&model,"<robot name=\"fixture\"><link name=\"base\"/><link name=\"arm\"/><joint name=\"hinge\" type=\"revolute\"><parent link=\"base\"/><child link=\"arm\"/><axis xyz=\"0 1 0\"/><limit lower=\"-1\" upper=\"1\" effort=\"1\" velocity=\"1\"/></joint></robot>").unwrap();
        let mut run = Store::seed().runs.remove(0);
        run.id = "captured".into();
        run.snapshot.training_profile = Some(
            serde_json::json!({"program":"python3","args":[],"robot":"fixture","task_config":{"model":model},"recipe":{"num_envs":4,"iterations":3,"steps_per_env":8}}),
        );
        // Capture does not execute Python; use a temporary dependency marker.
        let python = root.join("python-fixture");
        assert!(prepare_with_python(&mut run, &Settings::default(), &root.join("captured"), &python).is_err());
        assert!(!root.join("captured").exists());
        fs::write(&python, b"fixture only").unwrap();
        prepare_with_python(&mut run, &Settings::default(), &root.join("captured"), &python).unwrap();
        fs::remove_file(model).unwrap();
        assert_eq!(run.state, RunState::Queued);
        let captured = run.snapshot.training_profile.as_ref().unwrap()["task_config"]["model"]
            .as_str()
            .unwrap();
        assert!(Path::new(captured).is_file());
        let job = run.job.unwrap();
        let doc = request(&job).unwrap();
        assert_eq!(doc["spec"]["command"][1], "--job-worker");
        assert_eq!(doc["settings"]["transport"], "local");
        fs::write(&job.request, b"changed").unwrap();
        assert!(request(&job).is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn managed_running_and_queued_jobs_survive_workspace_restart_without_false_stops() {
        let root = temp();
        let mut st = Store::seed();
        st.demo = false;
        st.queue_enabled = false;
        let mut job = Job {
            token: "token".into(),
            request: "job-request.json".into(),
            request_sha256: "digest".into(),
            settings: Settings::default(),
            queued_at: 1,
            not_before: 4,
            order: 1,
            last_contact: None,
            connection_error: None,
        };
        st.runs[0].job = Some(job.clone());
        st.runs[0].state = RunState::Running;
        job.order = 2;
        st.runs[1].job = Some(job);
        st.runs[1].state = RunState::Queued;
        let session = crate::workspace::Session::open(&root).unwrap();
        session.save(&st).unwrap();
        drop(session);
        let mut restored = Store::seed();
        restored.demo = false;
        crate::workspace::restore_at(&mut restored, &root.join("studio-workspace.json"), false)
            .unwrap();
        assert_eq!(restored.runs[0].state, RunState::Running);
        assert_eq!(restored.runs[1].state, RunState::Queued);
        assert!(!restored.queue_enabled);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn launch_settings_reject_invalid_ssh_inputs_before_creating_a_job() {
        let mut c = Settings::default();
        c.transport = "ssh".into();
        assert!(c.validate().is_err());
        c.host = "-oProxyCommand=unexpected".into();
        assert!(c.validate().is_err());
        c.transport = "local".into();
        c.delay_minutes = 525601;
        assert!(c.validate().is_err());
    }

    #[test]
    fn parallel_recipe_counts_all_environments_and_survives_reproduction() {
        let mut run = Store::seed().runs.remove(0);
        run.snapshot.training_profile = Some(serde_json::json!({
            "args":["/tmp/generic_worker.py", "--run-dir", "{run_dir}"],
            "task_config":{"kind":"generic_mujoco","ppo":{"device":"cpu","num_envs":4}},
            "recipe":{"num_envs":4}
        }));
        let mut settings = Settings {
            processes: 2,
            ..Default::default()
        };
        parallel_profile(&mut run, &mut settings).unwrap();
        let captured = run.snapshot.training_profile.clone().unwrap();
        assert_eq!(captured["recipe"]["num_envs"], 8);
        assert_eq!(captured["parallel"]["processes"], 2);
        assert!(captured["args"][0]
            .as_str()
            .unwrap()
            .ends_with("parallel_worker.py"));
        let mut changed_settings = Settings::default();
        parallel_profile(&mut run, &mut changed_settings).unwrap();
        assert_eq!(changed_settings.processes, 2);
        assert_eq!(run.snapshot.training_profile.as_ref().unwrap(), &captured);
    }

    #[test]
    fn parallel_recipe_rejects_mps_without_changing_the_task_or_device() {
        let mut run = Store::seed().runs.remove(0);
        run.snapshot.training_profile = Some(
            serde_json::json!({"task_config":{"kind":"microduck","ppo":{"device":"mps","num_envs":4}}}),
        );
        let original = run.snapshot.training_profile.clone();
        let mut settings = Settings {
            processes: 2,
            ..Default::default()
        };
        assert!(parallel_profile(&mut run, &mut settings)
            .unwrap_err()
            .contains("MPS"));
        assert_eq!(run.snapshot.training_profile, original);
    }
}
