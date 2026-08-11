//! The run is the document.
//!
//! Every artifact hangs off a [`Run`]. There is no loose state in this app,
//! because the checkpoint graveyard is the natural end state of every training
//! tool that lacks this.
//!
//! No trainer is wired up yet, so the metrics here are generated. That is
//! marked on screen rather than hidden: a plausible-looking curve that nothing
//! produced is the most expensive kind of lie a tool like this can tell.

use std::path::PathBuf;

use crate::env::{TERM_NAMES, TERM_WEIGHTS};
use crate::trainer::Sample;

/// A reward term, named so it can be blamed individually.
#[derive(Clone, Debug)]
pub struct Term {
    pub name: String,
    pub weight: f64,
    /// Contribution over the run, one sample per logged interval.
    pub series: Vec<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunState {
    Training,
    Stopped,
    Trained,
    Validated,
    Failed,
}

impl RunState {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Training => "training",
            Self::Stopped => "stopped",
            Self::Trained => "trained",
            Self::Validated => "validated",
            Self::Failed => "failed",
        }
    }
    /// Chip tone: 0 neutral, 1 ok, 2 warn, 3 stop, 4 accent.
    pub fn tone(&self) -> f64 {
        match self {
            Self::Training => 4.0,
            Self::Stopped => 0.0,
            Self::Trained => 1.0,
            Self::Validated => 1.0,
            Self::Failed => 3.0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Checkpoint {
    pub step: u64,
    pub score: f64,
}

impl Checkpoint {
    pub fn name(&self) -> String {
        format!("ckpt-{:03}M", self.step / 1_000_000)
    }
}

/// A named failure signature, with what to do about it.
#[derive(Clone, Debug)]
pub struct Finding {
    pub headline: String,
    pub detail: String,
    /// 2 warn, 3 stop.
    pub tone: f64,
}

#[derive(Clone, Debug)]
pub struct Run {
    pub id: String,
    pub robot: String,
    pub scene: String,
    pub seed: u64,
    pub state: RunState,
    pub envs: u32,
    pub steps: u64,
    pub total_steps: u64,
    pub steps_per_sec: f64,
    pub elapsed_s: f64,
    pub terms: Vec<Term>,
    /// Fraction of environments that terminated by falling, per interval.
    pub fall_rate: Vec<f64>,
    /// Throughput over the run, for the collapse detector.
    pub throughput: Vec<f64>,
    pub checkpoints: Vec<Checkpoint>,
    /// Lean of a sample of environments, for the contact sheet.
    pub leans: Vec<f32>,
}

impl Run {
    pub fn progress(&self) -> f64 {
        if self.total_steps == 0 {
            return 0.0;
        }
        (self.steps as f64 / self.total_steps as f64).clamp(0.0, 1.0)
    }

    pub fn elapsed_label(&self) -> String {
        let t = self.elapsed_s as u64;
        if t < 3600 {
            format!("{:02}:{:02}", t / 60, t % 60)
        } else {
            format!("{}:{:02}:{:02}", t / 3600, (t % 3600) / 60, t % 60)
        }
    }

    pub fn steps_label(&self) -> String {
        format!("{}M / {}M", self.steps / 1_000_000, self.total_steps / 1_000_000)
    }

    /// Total reward: the weighted sum of every term, per interval.
    pub fn total_reward(&self) -> Vec<f64> {
        let n = self.terms.iter().map(|t| t.series.len()).max().unwrap_or(0);
        (0..n)
            .map(|i| {
                self.terms
                    .iter()
                    .map(|t| t.weight * t.series.get(i).copied().unwrap_or(0.0))
                    .sum()
            })
            .collect()
    }

    /// The diagnosis catalogue, run over this run's metrics.
    ///
    /// These are not clever. They are the patterns an experienced engineer
    /// checks by eye, written down so they are available to everyone else at
    /// 2am — which is the entire point.
    pub fn findings(&self) -> Vec<Finding> {
        let mut out = Vec::new();
        let reward = self.total_reward();

        if let (Some(rt), Some(ft)) = (trend(&reward), trend(&self.fall_rate)) {
            if rt > 0.02 && ft > 0.02 {
                // The signature of a term being exploited: the policy is scoring
                // better while falling over more often.
                let worst = self
                    .terms
                    .iter()
                    .max_by(|a, b| {
                        trend(&a.series)
                            .unwrap_or(0.0)
                            .partial_cmp(&trend(&b.series).unwrap_or(0.0))
                            .unwrap()
                    })
                    .map(|t| t.name.clone())
                    .unwrap_or_default();
                out.push(Finding {
                    headline: "Reward hacking".into(),
                    detail: format!(
                        "Reward is rising while falls rise with it. '{worst}' is climbing fastest — \
                         open it in Task and consider a cap or a penalty."
                    ),
                    tone: 2.0,
                });
            }
        }

        if let Some(rt) = trend(&reward) {
            if reward.len() > 8 && rt.abs() < 0.002 {
                out.push(Finding {
                    headline: "Dead signal".into(),
                    detail: "Reward has not moved since the run began. Check the observation \
                             contract in Scene for a channel that is silently zero."
                        .into(),
                    tone: 3.0,
                });
            }
        }

        if self.throughput.len() > 4 {
            let head = mean(&self.throughput[..self.throughput.len() / 3]);
            let tail = mean(&self.throughput[self.throughput.len() * 2 / 3..]);
            if head > 0.0 && tail < head * 0.7 {
                out.push(Finding {
                    headline: "Throughput collapse".into(),
                    detail: format!(
                        "Steps per second fell from {head:.0}k to {tail:.0}k. Something is pulling \
                         the loop off the GPU — readback frequency is the usual culprit."
                    ),
                    tone: 2.0,
                });
            }
        }

        out
    }
}

/// Least-squares slope, normalised by the series' own scale so the threshold
/// means the same thing for a reward and for a fall rate.
fn trend(xs: &[f64]) -> Option<f64> {
    if xs.len() < 4 {
        return None;
    }
    let n = xs.len() as f64;
    let mx = (n - 1.0) / 2.0;
    let my = mean(xs);
    let mut num = 0.0;
    let mut den = 0.0;
    for (i, y) in xs.iter().enumerate() {
        let dx = i as f64 - mx;
        num += dx * (y - my);
        den += dx * dx;
    }
    if den == 0.0 {
        return None;
    }
    let scale = xs.iter().cloned().fold(f64::MIN, f64::max).abs().max(1e-6);
    Some((num / den) * n / scale)
}

fn mean(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    xs.iter().sum::<f64>() / xs.len() as f64
}

/// Build a run from what the trainer has published so far.
///
/// This is the path that replaces the generated fixtures: same `Run` shape, so
/// every screen and the diagnosis catalogue work unchanged on live data.
pub fn live_run(samples: &[Sample], envs: u32, total_steps: u64, seed: u64) -> Run {
    let last = samples.last();
    let terms = TERM_NAMES
        .iter()
        .enumerate()
        .map(|(i, name)| Term {
            name: (*name).to_string(),
            weight: TERM_WEIGHTS[i] as f64,
            series: samples
                .iter()
                .map(|s| s.terms.get(i).copied().unwrap_or(0.0) as f64)
                .collect(),
        })
        .collect();

    let steps = last.map(|s| s.step).unwrap_or(0);
    let best = samples
        .iter()
        .map(|s| s.reward as f64)
        .fold(f64::MIN, f64::max);

    Run {
        id: "balance-track-01".into(),
        robot: "cart-pole".into(),
        scene: "balance + velocity tracking".into(),
        seed,
        state: if steps >= total_steps { RunState::Trained } else { RunState::Training },
        envs,
        steps,
        total_steps,
        steps_per_sec: last.map(|s| s.steps_per_sec).unwrap_or(0.0),
        elapsed_s: last
            .map(|s| s.step as f64 / s.steps_per_sec.max(1.0))
            .unwrap_or(0.0),
        terms,
        fall_rate: samples.iter().map(|s| s.fall_rate as f64).collect(),
        throughput: samples.iter().map(|s| s.steps_per_sec / 1000.0).collect(),
        // Checkpoints are not written yet; the best interval stands in for the
        // score so the rail is not empty and does not invent a filename.
        checkpoints: if samples.is_empty() {
            Vec::new()
        } else {
            vec![Checkpoint { step: steps, score: best }]
        },
        leans: last.map(|s| s.leans.clone()).unwrap_or_default(),
    }
}

/// A robot the app can simulate.
#[derive(Clone, Debug)]
pub struct Robot {
    pub name: String,
    pub urdf: PathBuf,
    pub assets: PathBuf,
    pub dof: u32,
}

pub struct Studio {
    pub screen: crate::ux::Screen,
    pub robots: Vec<Robot>,
    pub runs: Vec<Run>,
    pub selected: usize,
}

impl Studio {
    pub fn new() -> Self {
        Self {
            screen: crate::ux::Screen::Train,
            robots: vec![Robot {
                name: "Unitree G1".into(),
                urdf: PathBuf::from("data/g1/g1.urdf"),
                assets: PathBuf::from("data/g1"),
                dof: 29,
            }],
            runs: sample_runs(),
            selected: 0,
        }
    }

    pub fn run(&self) -> &Run {
        &self.runs[self.selected.min(self.runs.len() - 1)]
    }

    pub fn robot(&self) -> &Robot {
        &self.robots[0]
    }
}

/// Stand-in metrics until a trainer is attached.
///
/// Shaped to exercise the diagnosis catalogue rather than to look pretty: the
/// second run deliberately carries the reward-hacking signature so the detector
/// has something to find.
fn sample_runs() -> Vec<Run> {
    const N: usize = 120;
    let curve = |a: f64, b: f64, k: f64| -> Vec<f64> {
        (0..N)
            .map(|i| {
                let t = i as f64 / (N - 1) as f64;
                a + (b - a) * (1.0 - (-k * t).exp())
            })
            .collect()
    };

    let terms = |lift: f64| {
        vec![
            Term { name: "track_lin_vel".into(), weight: 1.0, series: curve(0.05, 0.92, 3.4) },
            Term { name: "track_ang_vel".into(), weight: 0.5, series: curve(0.04, 0.78, 3.0) },
            Term { name: "upright".into(), weight: 0.3, series: curve(0.30, 0.88, 4.2) },
            Term { name: "feet_air_time".into(), weight: 0.15, series: curve(0.02, lift, 2.2) },
            Term { name: "action_rate".into(), weight: -0.01, series: curve(0.60, 0.22, 2.6) },
            Term { name: "torque".into(), weight: -0.00002, series: curve(0.55, 0.30, 2.0) },
        ]
    };

    let falls_down: Vec<f64> = curve(0.42, 0.06, 3.0);
    let falls_up: Vec<f64> = (0..N)
        .map(|i| 0.10 + 0.35 * (i as f64 / (N - 1) as f64))
        .collect();
    let flat_tp: Vec<f64> = (0..N).map(|_| 68.0).collect();

    vec![
        Run {
            id: "g1-walk-slope-07".into(),
            robot: "Unitree G1".into(),
            scene: "slope 12° · rough".into(),
            seed: 7,
            state: RunState::Training,
            envs: 4096,
            steps: 168_000_000,
            total_steps: 400_000_000,
            steps_per_sec: 68_200.0,
            elapsed_s: 41.0 * 60.0,
            terms: terms(0.51),
            fall_rate: falls_down,
            throughput: flat_tp.clone(),
            leans: Vec::new(),
            checkpoints: vec![
                Checkpoint { step: 160_000_000, score: 0.81 },
                Checkpoint { step: 150_000_000, score: 0.79 },
                Checkpoint { step: 140_000_000, score: 0.74 },
                Checkpoint { step: 130_000_000, score: 0.71 },
            ],
        },
        Run {
            id: "g1-walk-slope-06".into(),
            robot: "Unitree G1".into(),
            scene: "slope 12° · rough".into(),
            seed: 6,
            state: RunState::Failed,
            envs: 4096,
            steps: 400_000_000,
            total_steps: 400_000_000,
            steps_per_sec: 0.0,
            elapsed_s: 96.0 * 60.0,
            // Reward climbs while the robot falls more: the catalogue's first entry.
            terms: terms(0.95),
            fall_rate: falls_up,
            throughput: flat_tp,
            leans: Vec::new(),
            checkpoints: vec![Checkpoint { step: 400_000_000, score: 0.44 }],
        },
        Run {
            id: "g1-flat-baseline".into(),
            robot: "Unitree G1".into(),
            scene: "flat".into(),
            seed: 1,
            state: RunState::Validated,
            envs: 2048,
            steps: 200_000_000,
            total_steps: 200_000_000,
            steps_per_sec: 0.0,
            elapsed_s: 52.0 * 60.0,
            terms: terms(0.40),
            fall_rate: curve(0.30, 0.02, 3.6),
            throughput: (0..N).map(|_| 71.0).collect(),
            leans: Vec::new(),
            checkpoints: vec![Checkpoint { step: 200_000_000, score: 0.93 }],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reward_hacking_is_detected_when_falls_rise_with_reward() {
        let runs = sample_runs();
        let bad = runs.iter().find(|r| r.id == "g1-walk-slope-06").unwrap();
        let found = bad.findings();
        assert!(
            found.iter().any(|f| f.headline == "Reward hacking"),
            "expected the hacking signature, got {found:?}"
        );
    }

    #[test]
    fn a_healthy_run_reports_nothing() {
        let runs = sample_runs();
        let good = runs.iter().find(|r| r.id == "g1-flat-baseline").unwrap();
        assert!(good.findings().is_empty(), "healthy run flagged: {:?}", good.findings());
    }

    #[test]
    fn total_reward_is_the_weighted_sum() {
        let r = &sample_runs()[0];
        let total = r.total_reward();
        assert_eq!(total.len(), r.terms[0].series.len());
        let expect: f64 = r.terms.iter().map(|t| t.weight * t.series[0]).sum();
        assert!((total[0] - expect).abs() < 1e-9);
    }

    #[test]
    fn a_flat_series_has_no_trend() {
        assert!(trend(&[1.0; 20]).unwrap().abs() < 1e-9);
    }
}
