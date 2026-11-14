//! Process adapter for robot-specific RL trainers (Isaac Lab, MuJoCo, etc.).
//! Arguments are passed directly, never through a shell. The caller supplies
//! the JSON metric parser, keeping the engine independent of a JSON library.
use crate::trainer::{Handle, Sample, Shared};
use std::{
    collections::BTreeMap,
    fs,
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
};

pub struct Spec {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub env: BTreeMap<String, String>,
    pub log: PathBuf,
    pub checkpoint: PathBuf,
}
pub fn spawn(
    spec: Spec,
    parse: fn(&str) -> Result<Option<Sample>, String>,
) -> Result<Handle, String> {
    let mut log = fs::File::create(&spec.log).map_err(|e| e.to_string())?;
    let stderr =
        fs::File::create(spec.log.with_extension("stderr.log")).map_err(|e| e.to_string())?;
    let mut command = Command::new(&spec.program);
    command
        .args(&spec.args)
        .current_dir(&spec.cwd)
        .envs(&spec.env)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(stderr);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        if std::env::var_os("NEXUS_SUPERVISED").is_none() {
            command.process_group(0);
        }
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("{}: {e}", spec.program))?;
    let stdout = child.stdout.take().ok_or("trainer has no stdout")?;
    let child = Arc::new(Mutex::new(child));
    let shared = Arc::new(Mutex::new(Shared {
        running: true,
        ..Shared::default()
    }));
    let stopped = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel();
    let (watched, flag) = (child.clone(), stopped.clone());
    thread::spawn(move || {
        let _ = rx.recv();
        flag.store(true, Ordering::Relaxed);
        if let Ok(mut child) = watched.lock() {
            if child.try_wait().ok().flatten().is_some() {
                return;
            }
            #[cfg(unix)]
            {
                let _ = Command::new("kill")
                    .args(["-TERM", "--", &format!("-{}", child.id())])
                    .status();
            }
            let _ = child.kill();
        }
    });
    let s = shared.clone();
    thread::spawn(move || {
        let mut last = 0;
        let mut error = None;
        for line in BufReader::new(stdout).lines() {
            let line = match line {
                Ok(line) => line,
                Err(e) => {
                    error = Some(e.to_string());
                    break;
                }
            };
            if let Err(e) = writeln!(log, "{line}") {
                error = Some(format!("log write failed: {e}"));
                break;
            }
            match parse(&line) {
                Ok(Some(sample)) => {
                    if sample.step <= last
                        || !sample.reward.is_finite()
                        || !sample.fall_rate.is_finite()
                        || !(0.0..=1.0).contains(&sample.fall_rate)
                        || !sample.steps_per_sec.is_finite()
                        || sample.steps_per_sec < 0.0
                        || !sample.episode_len.is_finite()
                        || sample.episode_len < 0.0
                    {
                        error = Some(
                            "trainer emitted non-finite metrics or non-increasing steps".into(),
                        );
                        break;
                    }
                    last = sample.step;
                    let mut g = s.lock().unwrap();
                    g.samples.push(sample);
                    if g.samples.len() > 600 {
                        g.samples.drain(0..100);
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    error = Some(e);
                    break;
                }
            }
        }
        loop {
            let status = if let Ok(mut c) = child.lock() {
                if error.is_some() {
                    let _ = c.kill();
                }
                c.try_wait()
            } else {
                Err(std::io::Error::other("trainer process lock failed"))
            };
            match status {
                Ok(Some(status)) => {
                    if !status.success() && !stopped.load(Ordering::Relaxed) {
                        error.get_or_insert(format!(
                            "trainer exited with {status}; see {}",
                            spec.log.with_extension("stderr.log").display()
                        ));
                    }
                    break;
                }
                Ok(None) => thread::sleep(std::time::Duration::from_millis(20)),
                Err(e) => {
                    error.get_or_insert(e.to_string());
                    break;
                }
            }
        }
        let was_stopped = stopped.load(Ordering::Relaxed);
        if !was_stopped && error.is_none() && (last == 0 || !spec.checkpoint.is_file()) {
            error = Some("trainer finished without metrics or the declared checkpoint".into());
        }
        let _ = log.sync_all();
        let mut g = s.lock().unwrap();
        g.error = error;
        g.running = false;
    });
    Ok(Handle::external(shared, tx))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parse(s: &str) -> Result<Option<Sample>, String> {
        if let Some(n) = s.strip_prefix("sample ") {
            return Ok(Some(Sample {
                step: n.parse().unwrap(),
                reward: 0.5,
                fall_rate: 0.0,
                ..Sample::default()
            }));
        }
        Ok(None)
    }
    #[test]
    fn successful_and_failed_processes_report_their_real_status() {
        let root = std::env::temp_dir().join(format!("nexus-external-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        for (name, script, fails) in [
            ("success", "from pathlib import Path; Path('policy.bin').write_bytes(b'policy'); print('sample 1'); print('sample 2')", false),
            ("failure", "import sys; print('sample 1'); sys.exit(7)", true),
            ("bad-order", "print('sample 2'); print('sample 1')", true),
        ] {
            let h = spawn(Spec { program: "python3".into(), args: vec!["-u".into(), "-c".into(), script.into()], cwd: root.clone(), env: BTreeMap::new(), log: root.join(format!("{name}.log")), checkpoint: root.join("policy.bin") }, parse).unwrap();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            loop {
                if !h.shared.lock().unwrap().running { break; }
                assert!(std::time::Instant::now() < deadline);
                thread::sleep(std::time::Duration::from_millis(10));
            }
            assert_eq!(h.shared.lock().unwrap().error.is_some(), fails, "{name}");
        }
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn stop_still_works_after_trainer_closes_stdout() {
        let root = std::env::temp_dir().join(format!("nexus-stop-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let h = spawn(
            Spec {
                program: "python3".into(),
                args: vec![
                    "-u".into(),
                    "-c".into(),
                    "import os,time; os.close(1); time.sleep(30)".into(),
                ],
                cwd: root.clone(),
                env: BTreeMap::new(),
                log: root.join("metrics.log"),
                checkpoint: root.join("policy.bin"),
            },
            parse,
        )
        .unwrap();
        thread::sleep(std::time::Duration::from_millis(100));
        h.stop();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while h.shared.lock().unwrap().running {
            assert!(std::time::Instant::now() < deadline);
            thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(h.shared.lock().unwrap().error.is_none());
        fs::remove_dir_all(root).unwrap();
    }
}
