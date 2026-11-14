//! Checkpoints on disk, with the provenance that makes them shippable.
//!
//! Law 05: a checkpoint carries the scene, weights and seed that produced it.
//! A file of floats with no lineage is a file you cannot ship, and after a week
//! of runs it is a file nobody dares delete either.
//!
//! The format is deliberately dull — a little-endian `f32` blob beside a small
//! text manifest — so a checkpoint can be read by anything, including a future
//! GPU trainer that does not share this crate's types.

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

/// Everything needed to identify what produced a set of weights.
#[derive(Clone, Debug, Default)]
pub struct Manifest {
    pub run: String,
    pub scene: String,
    pub seed: u64,
    pub step: u64,
    pub score: f64,
    pub n_obs: usize,
    pub n_act: usize,
    pub hidden: usize,
    /// Reward term names and weights, so a checkpoint records what it was
    /// optimising — not merely that it was optimised.
    pub terms: Vec<(String, f64)>,
}

impl Manifest {
    fn to_text(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!("run {}\n", self.run));
        s.push_str(&format!("scene {}\n", self.scene));
        s.push_str(&format!("seed {}\n", self.seed));
        s.push_str(&format!("step {}\n", self.step));
        s.push_str(&format!("score {:.6}\n", self.score));
        s.push_str(&format!(
            "shape {} {} {}\n",
            self.n_obs, self.n_act, self.hidden
        ));
        for (name, w) in &self.terms {
            s.push_str(&format!("term {name} {w}\n"));
        }
        s
    }

    fn from_text(text: &str) -> Self {
        let mut m = Manifest::default();
        for line in text.lines() {
            let mut it = line.split_whitespace();
            match it.next() {
                Some("run") => m.run = it.collect::<Vec<_>>().join(" "),
                Some("scene") => m.scene = it.collect::<Vec<_>>().join(" "),
                Some("seed") => m.seed = it.next().and_then(|v| v.parse().ok()).unwrap_or(0),
                Some("step") => m.step = it.next().and_then(|v| v.parse().ok()).unwrap_or(0),
                Some("score") => m.score = it.next().and_then(|v| v.parse().ok()).unwrap_or(0.0),
                Some("shape") => {
                    m.n_obs = it.next().and_then(|v| v.parse().ok()).unwrap_or(0);
                    m.n_act = it.next().and_then(|v| v.parse().ok()).unwrap_or(0);
                    m.hidden = it.next().and_then(|v| v.parse().ok()).unwrap_or(0);
                }
                Some("term") => {
                    let name = it.next().unwrap_or("").to_string();
                    let w = it.next().and_then(|v| v.parse().ok()).unwrap_or(0.0);
                    m.terms.push((name, w));
                }
                _ => {}
            }
        }
        m
    }
}

/// Where a run's checkpoints live. Runs own their artifacts (Law 01).
pub fn run_dir(run: &str) -> PathBuf {
    let root = std::env::var_os("NEXUS_RUNS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("runs"));
    root.join(if valid_id(run) { run } else { "invalid-run-id" })
}

pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id != "."
        && id != ".."
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
}

pub fn write(run: &str, name: &str, weights: &[f32], manifest: &Manifest) -> io::Result<PathBuf> {
    if !valid_id(run)
        || !valid_id(name)
        || manifest.run != run
        || weights.is_empty()
        || weights.iter().any(|v| !v.is_finite())
        || !manifest.score.is_finite()
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid checkpoint identity or non-finite weights/score",
        ));
    }
    let dir = run_dir(run);
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{name}.bin"));
    let temp = dir.join(format!(".{name}.bin.tmp"));
    let mut f = fs::File::create(&temp)?;
    for v in weights {
        f.write_all(&v.to_le_bytes())?;
    }
    f.sync_all()?;
    let meta_temp = dir.join(format!(".{name}.txt.tmp"));
    fs::write(&meta_temp, manifest.to_text())?;
    fs::rename(temp, &path)?;
    fs::rename(meta_temp, dir.join(format!("{name}.txt")))?;
    Ok(path)
}

pub fn read(path: &Path) -> io::Result<(Vec<f32>, Manifest)> {
    let mut buf = Vec::new();
    fs::File::open(path)?.read_to_end(&mut buf)?;
    if buf.is_empty() || buf.len() % 4 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "truncated or empty checkpoint",
        ));
    }
    let weights: Vec<f32> = buf
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    let manifest = Manifest::from_text(&fs::read_to_string(path.with_extension("txt"))?);
    if manifest.n_obs == 0
        || manifest.n_act == 0
        || manifest.hidden == 0
        || weights.iter().any(|v| !v.is_finite())
        || !manifest.score.is_finite()
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid checkpoint weights or manifest",
        ));
    }
    Ok((weights, manifest))
}

/// Checkpoints a run has written, newest first.
pub fn list(run: &str) -> Vec<(PathBuf, Manifest)> {
    let dir = run_dir(run);
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out: Vec<(PathBuf, Manifest)> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().map(|e| e == "bin").unwrap_or(false))
        .filter_map(|p| {
            let text = fs::read_to_string(p.with_extension("txt")).ok()?;
            Some((p, Manifest::from_text(&text)))
        })
        .collect();
    out.sort_by(|a, b| b.1.step.cmp(&a.1.step));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_checkpoint_round_trips_with_its_provenance() {
        let run = format!("test-{}", std::process::id());
        let weights = vec![0.5_f32, -1.25, 3.0];
        let m = Manifest {
            run: run.clone(),
            scene: "balance + velocity tracking".into(),
            seed: 7,
            step: 1_000_000,
            score: 0.812_345,
            n_obs: 5,
            n_act: 1,
            hidden: 64,
            terms: vec![("track_lin_vel".into(), 1.0), ("upright".into(), 0.5)],
        };
        let path = write(&run, "ckpt-001M", &weights, &m).unwrap();

        let (w2, m2) = read(&path).unwrap();
        assert_eq!(w2, weights);
        assert_eq!(m2.seed, 7);
        assert_eq!(m2.step, 1_000_000);
        assert_eq!(m2.n_obs, 5);
        assert_eq!(m2.terms.len(), 2);
        assert_eq!(m2.terms[0].0, "track_lin_vel");
        assert!((m2.score - 0.812_345).abs() < 1e-6);

        assert_eq!(list(&run).len(), 1);
        let _ = fs::remove_dir_all(run_dir(&run));
    }
}

#[cfg(test)]
mod malformed_tests {
    use super::*;
    #[test]
    fn traversal_and_non_finite_artifacts_are_refused() {
        assert!(!valid_id("../outside"));
        assert!(!valid_id("/tmp/outside"));
        let m = Manifest {
            run: "../outside".into(),
            ..Default::default()
        };
        assert!(write("../outside", "policy", &[1.0], &m).is_err());
        let m = Manifest {
            run: "review-nan".into(),
            ..Default::default()
        };
        assert!(write("review-nan", "policy", &[f32::NAN], &m).is_err());
    }
    #[test]
    fn truncated_weights_and_missing_manifest_are_refused() {
        let p = std::env::temp_dir().join(format!("nexus-corrupt-{}.bin", std::process::id()));
        fs::write(&p, [0u8; 5]).unwrap();
        assert!(read(&p).is_err());
        fs::write(&p, [0u8; 4]).unwrap();
        assert!(read(&p).is_err());
        fs::remove_file(p).unwrap();
    }
}
