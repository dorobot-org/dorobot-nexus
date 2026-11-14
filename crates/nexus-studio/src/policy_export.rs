//! Supervise a real policy exporter without blocking the Makepad event loop.
use crate::state::*;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::Instant,
};

pub struct Job {
    child: Child,
    pub run: Option<String>,
    pub output: PathBuf,
    pub log: PathBuf,
    started: Instant,
}
impl Drop for Job {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_some() {
            return;
        }
        #[cfg(unix)]
        {
            let _ = Command::new("kill")
                .args(["-TERM", "--", &format!("-{}", self.child.id())])
                .status();
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl crate::App {
    pub fn start_policy_export(
        &mut self,
        checkpoint: &str,
        destination: &str,
    ) -> Result<(), String> {
        if self.export_job.is_some() {
            return Err("A policy export is already running".into());
        }
        let source = Path::new(checkpoint)
            .canonicalize()
            .map_err(|e| e.to_string())?;
        let mut run = self
            .st
            .sel
            .run
            .as_ref()
            .and_then(|id| self.st.run(id))
            .cloned()
            .ok_or("Select the checkpoint's run")?;
        crate::nexus::restore_run_recipe(&mut run)?;
        if !run
            .ckpts
            .iter()
            .any(|c| Path::new(&c.id).canonicalize().ok().as_ref() == Some(&source))
        {
            return Err("Checkpoint no longer belongs to the selected run".into());
        }
        let robot = if run.snapshot.backend.as_deref() == Some("cpu")
            || source.extension().is_some_and(|s| s == "bin")
        {
            "CPU cart-pole".to_string()
        } else {
            run.snapshot
                .robot
                .clone()
                .ok_or("Run has no robot contract")?
        };
        let output = PathBuf::from(destination.trim());
        if !output.is_absolute() || output.exists() {
            return Err("Choose a new absolute export directory".into());
        }
        let workspace = PathBuf::from(crate::nexus::repo_dir());
        let installed = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .map_err(|e| e.to_string())?;
        let root = if workspace
            .join("tools/training/export_checkpoint.py")
            .is_file()
        {
            workspace
        } else {
            installed
        };
        let python = root.join("training-stack/microduck_rl/.venv/bin/python");
        if !python.is_file() {
            return Err(
                "Policy export needs the configured PyTorch/ONNX Python environment".into(),
            );
        }
        fs::create_dir_all(
            output
                .parent()
                .ok_or("Export path needs a parent directory")?,
        )
        .map_err(|e| e.to_string())?;
        let serial = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let log = output.with_extension(format!("export-{serial}.log"));
        let file = fs::File::options()
            .create_new(true)
            .write(true)
            .open(&log)
            .map_err(|e| e.to_string())?;
        let recipe = log.with_extension("recipe.json");
        fs::write(
            &recipe,
            serde_json::to_vec_pretty(&run.snapshot).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let mut command = Command::new(python);
        command
            .arg(root.join("tools/training/export_checkpoint.py"))
            .args(["--robot", &robot, "--checkpoint"])
            .arg(&source)
            .arg("--output")
            .arg(&output)
            .arg("--studio-root")
            .arg(&root)
            .arg("--recipe-file")
            .arg(&recipe)
            .stdin(Stdio::null())
            .stdout(file.try_clone().map_err(|e| e.to_string())?)
            .stderr(file);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let child = command.spawn().map_err(|e| e.to_string())?;
        self.export_job = Some(Job {
            child,
            run: Some(run.id.clone()),
            output,
            log,
            started: Instant::now(),
        });
        self.st.modal = AppModal::None;
        self.st.toast(format!(
            "Exporting {robot}: observation normalization and action parity will be checked"
        ));
        Ok(())
    }
    pub fn poll_policy_export(&mut self) -> bool {
        let Some(job) = self.export_job.as_mut() else {
            return false;
        };
        let result = job.child.try_wait();
        if matches!(result, Ok(None)) && job.started.elapsed().as_secs() < 180 {
            return false;
        }
        let job = self.export_job.take().unwrap();
        let message = match result {
            Ok(Some(status))
                if status.success()
                    && job.output.join("provenance.json").is_file()
                    && job.output.join("policy.onnx").is_file() =>
            {
                format!("Verified ONNX bundle exported to {}", job.output.display())
            }
            Ok(None) => format!("Export timed out. Details: {}", job.log.display()),
            _ => {
                let detail = fs::read_to_string(&job.log)
                    .unwrap_or_default()
                    .lines()
                    .rev()
                    .take(3)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect::<Vec<_>>()
                    .join(" ");
                format!("Export failed: {detail}. Log: {}", job.log.display())
            }
        };
        self.st.toast(message);
        true
    }
}

#[cfg(test)]
mod integration {
    use super::*;
    #[test]
    #[ignore = "requires the configured Python ONNX environment and a learned CPU checkpoint"]
    fn cpu_onnx_matches_native_actor() {
        let source = PathBuf::from(
            std::env::var("NEXUS_VERIFY_CPU_SOURCE").expect("NEXUS_VERIFY_CPU_SOURCE"),
        );
        let original = fs::read(&source).unwrap();
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap();
        let output = std::env::temp_dir().join(format!(
            "nexus-cpu-onnx-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let result = Command::new(root.join("training-stack/microduck_rl/.venv/bin/python"))
            .arg(root.join("tools/training/export_checkpoint.py"))
            .args(["--robot", "CPU cart-pole", "--checkpoint"])
            .arg(&source)
            .arg("--output")
            .arg(&output)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let (weights, meta) = nexus_engine::ckpt::read(&source).unwrap();
        let mut rng = nexus_engine::rng::Rng::new(11);
        let mut native = nexus_engine::rl::Ppo::new(
            meta.n_obs,
            meta.n_act,
            meta.hidden,
            nexus_engine::rl::Config::default(),
            &mut rng,
        );
        assert!(native.load_weights(&weights));
        let parity: serde_json::Value =
            serde_json::from_slice(&fs::read(output.join("parity.json")).unwrap()).unwrap();
        let mut maximum = 0.0f32;
        for case in parity["cases"].as_array().unwrap() {
            let obs = case["observation"]
                .as_array()
                .unwrap()
                .iter()
                .map(|n| n.as_f64().unwrap() as f32)
                .collect::<Vec<_>>();
            let mut action = vec![0.0; meta.n_act];
            native.act_mean(&obs, &mut action);
            for (a, b) in action.iter().zip(case["onnx_action"].as_array().unwrap()) {
                maximum = maximum.max((a - b.as_f64().unwrap() as f32).abs());
            }
        }
        assert!(
            maximum.is_finite() && maximum < 1e-5,
            "native actor mismatch: {maximum}"
        );
        assert_eq!(original, fs::read(source).unwrap());
        println!(
            "96 ONNX cases match the native Rust actor; maximum error {maximum}; bundle {}",
            output.display()
        );
    }
}
