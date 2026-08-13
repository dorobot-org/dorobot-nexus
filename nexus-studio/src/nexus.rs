//! The real dorobot-nexus wiring: this module includes dorobot-nexus's own
//! dependency-free `scene.rs` verbatim (same structs, same on-disk JSON), so
//! scenes and recordings edited here are the same files the real app trains
//! from. It also lists real checkpoints, loads real recorded rollouts, and
//! spawns the real `dorobot-nexus` binary for headless training / sweeps.

use std::io::BufRead;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};

/// dorobot-nexus's artifact-schema library (scenes, recordings) — the
/// dependency-free lib target it exposes for exactly this purpose.
pub use dorobot_nexus::scene;

/// The dorobot-nexus checkout this studio operates on. Override with
/// DOROBOT_NEXUS_DIR; defaults to the sibling checkout layout.
pub fn repo_dir() -> String {
    std::env::var("DOROBOT_NEXUS_DIR").unwrap_or_else(|_| {
        let home = std::env::var("HOME").unwrap_or_default();
        format!("{home}/home/dorobot-nexus")
    })
}

/// The real trainer binary. Override with DOROBOT_NEXUS_BIN.
pub fn bin_path() -> String {
    std::env::var("DOROBOT_NEXUS_BIN").unwrap_or_else(|_| format!("{}/target/release/dorobot-nexus", repo_dir()))
}

/// Point the schema library at the real scene library before anything reads it.
pub fn init_env() {
    std::env::set_var("DOROBOT_SCENES_DIR", format!("{}/scenes", repo_dir()))
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
    Some(Replay { name: name.into(), joint_names, joints, dt })
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

// ------------------------------------------------------- real processes --

pub struct RealProc {
    pub child: Child,
    pub rx: Receiver<String>,
    pub kind: ProcKind,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ProcKind {
    Train,
    Sweep,
}

pub fn bin_exists() -> bool {
    std::path::Path::new(&bin_path()).exists()
}

/// Spawn the real dorobot-nexus binary and stream its stdout line by line.
pub fn spawn(kind: ProcKind, args: &[&str]) -> std::io::Result<RealProc> {
    let mut child = Command::new(bin_path())
        .args(args)
        .current_dir(repo_dir())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let (tx, rx) = channel();
    if let Some(out) = child.stdout.take() {
        let tx2 = tx.clone();
        std::thread::spawn(move || {
            for line in std::io::BufReader::new(out).lines().map_while(Result::ok) {
                if tx2.send(line).is_err() {
                    break;
                }
            }
        });
    }
    if let Some(err) = child.stderr.take() {
        std::thread::spawn(move || {
            for line in std::io::BufReader::new(err).lines().map_while(Result::ok) {
                if tx.send(format!("[err] {line}")).is_err() {
                    break;
                }
            }
        });
    }
    Ok(RealProc { child, rx, kind })
}

/// Parse one printed sweep row ("     1.60   .81 .77 …") into cell values.
pub fn parse_sweep_row(line: &str) -> Option<Vec<Option<f64>>> {
    let t = line.trim();
    if t.is_empty() || t.starts_with("mass") || t.contains('%') {
        return None;
    }
    let mut it = t.split_whitespace();
    let _axis: f64 = it.next()?.parse().ok()?;
    let cells: Vec<Option<f64>> = it
        .map(|c| {
            if c == "—" || c == "-" {
                None
            } else {
                c.trim_start_matches('.').parse::<f64>().ok().map(|v| if c.starts_with('.') { v / 10f64.powi(c.len() as i32 - 1) } else { v })
            }
        })
        .collect();
    if cells.is_empty() {
        None
    } else {
        Some(cells)
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

    #[test]
    fn loads_real_scene_files() {
        if !have_repo() {
            return;
        }
        init_env();
        let scenes = scene::list();
        // The repo ships flat-easy and rough-slippery.
        assert!(scenes.iter().any(|s| s.name == "flat-easy"), "found: {:?}", scenes.iter().map(|s| &s.name).collect::<Vec<_>>());
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
