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
