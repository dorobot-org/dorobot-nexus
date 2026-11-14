//! Lossless measured recordings, source provenance and reversible episode edits.
use crate::{nexus::Replay, state::*, App};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

const MAX_BYTES: u64 = 256 * 1024 * 1024;
const MAX_FRAMES: usize = 100_000;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Sample {
    /// Original simulator precision. Replay's f32 poses are only the viewport representation.
    #[serde(default)]
    pub joints: Vec<Vec<f64>>,
    #[serde(default)]
    pub base: Vec<[f64; 7]>,
    pub sequence: u64,
    pub sim_time: f64,
    #[serde(default)]
    pub reset_count: u64,
    #[serde(default)]
    pub terminal: bool,
    #[serde(default)]
    pub observation: Option<Vec<f64>>,
    #[serde(default)]
    pub next_observation: Option<Vec<f64>>,
    #[serde(default)]
    pub action: Option<Vec<f64>>,
    #[serde(default)]
    pub targets: Option<Vec<f64>>,
    #[serde(default)]
    pub torques: Option<Vec<f64>>,
    #[serde(default)]
    pub actuator_joints: Vec<String>,
    #[serde(default)]
    pub observation_names: Vec<String>,
    #[serde(default)]
    pub command: Option<Vec<f64>>,
    #[serde(default)]
    pub force_world: Option<Vec<f64>>,
    #[serde(default)]
    pub checkpoint_sha256: Option<String>,
    #[serde(default)]
    pub checkpoint: Option<String>,
    #[serde(default)]
    pub seed: Option<u64>,
    #[serde(default)]
    pub policy_sha256: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub control_ack: Option<u64>,
    #[serde(default)]
    pub reward_terms: std::collections::BTreeMap<String, f64>,
    #[serde(default)]
    pub sample_timing: String,
    #[serde(default)]
    pub pose_after_reset: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Segment {
    /// Source frame range [start, end), including any trimmed frames.
    pub start: usize,
    pub end: usize,
    pub keep_start: usize,
    pub keep_end: usize,
    pub excluded: bool,
    pub annotation: String,
    pub boundary: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Document {
    pub review_version: u32,
    pub revision: u64,
    pub robot_id: Option<String>,
    pub robot: String,
    pub run: Option<String>,
    pub source: String,
    pub source_sha256: String,
    #[serde(flatten)]
    pub replay: Replay,
    pub samples: Vec<Sample>,
    pub segments: Vec<Segment>,
}
#[derive(Default)]
pub struct State {
    pub document: Option<Document>,
    pub recording: Option<String>,
    pub segment: usize,
    pub channel: usize,
    pub page: usize,
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn read_bytes(path: &Path) -> Result<Vec<u8>, String> {
    let f = fs::File::open(path).map_err(|e| e.to_string())?;
    let size = f.metadata().map_err(|e| e.to_string())?.len();
    if size > MAX_BYTES {
        return Err("Recording exceeds the 256 MiB review limit; split it before importing".into());
    }
    let mut bytes = Vec::new();
    f.take(size)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    Ok(bytes)
}
fn segment(start: usize, end: usize, boundary: &str) -> Segment {
    Segment {
        start,
        end,
        keep_start: start,
        keep_end: end,
        excluded: false,
        annotation: String::new(),
        boundary: boundary.into(),
    }
}
impl Document {
    pub fn from_replay(
        replay: Replay,
        robot_id: Option<String>,
        robot: String,
        run: Option<String>,
    ) -> Result<Self, String> {
        let bytes = serde_json::to_vec(&replay).map_err(|e| e.to_string())?;
        let n = replay.joints.len();
        let doc = Self {
            review_version: 1,
            revision: 1,
            robot_id,
            robot,
            run,
            source: "saved policy replay".into(),
            source_sha256: hash(&bytes),
            replay,
            samples: vec![],
            segments: vec![segment(0, n, "replay; reset boundaries unavailable")],
        };
        doc.validate()?;
        Ok(doc)
    }
    pub fn validate(&self) -> Result<(), String> {
        let n = self.replay.joints.len();
        if self.revision == 0
            || self.source_sha256.len() != 64
            || !self.source_sha256.bytes().all(|b| b.is_ascii_hexdigit())
            || self.replay.joint_names.len() > 256
            || self.replay.joint_names.iter().any(|n| n.is_empty())
            || self.review_version != 1
            || n == 0
            || n > MAX_FRAMES
            || self.robot.is_empty()
            || (!self.samples.is_empty() && self.samples.len() != n)
            || !self.replay.dt.is_finite()
            || self.replay.dt <= 0.
            || self.replay.joint_names.is_empty()
            || self
                .replay
                .joint_names
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len()
                != self.replay.joint_names.len()
            || self.replay.joints.iter().any(|r| {
                r.len() != self.replay.joint_names.len() || r.iter().any(|v| !v.is_finite())
            })
            || (!self.replay.base.is_empty() && self.replay.base.len() != n)
            || self.replay.base.iter().any(|b| {
                b.iter().any(|v| !v.is_finite())
                    || (b[3..].iter().map(|v| v * v).sum::<f32>() - 1.).abs() > 0.02
            })
        {
            return Err("Invalid recording version, identity, frame count or physical pose".into());
        }
        let mut end = 0;
        for s in &self.segments {
            if s.start != end
                || s.start >= s.end
                || s.end > n
                || s.keep_start < s.start
                || s.keep_end > s.end
                || s.keep_start >= s.keep_end
                || s.annotation.len() > 4096
            {
                return Err("Episode ranges must cover the original recording once; trims must retain at least one frame".into());
            }
            end = s.end;
        }
        if end != n {
            return Err("Episode ranges do not cover the source recording".into());
        }
        for (i, s) in self.samples.iter().enumerate() {
            if (!s.joints.is_empty()
                && (s.joints.len() != 1
                    || s.joints[0].len() != self.replay.joint_names.len()
                    || s.joints[0].iter().any(|v| !v.is_finite())))
                || (!s.base.is_empty()
                    && (s.base.len() != 1 || s.base[0].iter().any(|v| !v.is_finite())))
            {
                return Err("Invalid original simulator pose".into());
            }
            if !s.sim_time.is_finite()
                || s.sim_time < 0.
                || (i > 0
                    && (s.sequence <= self.samples[i - 1].sequence
                        || s.sim_time <= self.samples[i - 1].sim_time))
            {
                return Err("Telemetry sequence/time is not strictly increasing".into());
            }
            for a in [
                &s.observation,
                &s.next_observation,
                &s.action,
                &s.targets,
                &s.torques,
                &s.command,
                &s.force_world,
            ]
            .into_iter()
            .flatten()
            {
                if a.is_empty() || a.len() > 4096 || a.iter().any(|v| !v.is_finite()) {
                    return Err("Invalid measured telemetry vector".into());
                }
            }
            if s.reward_terms.values().any(|v| !v.is_finite())
                || [&s.targets, &s.torques]
                    .into_iter()
                    .flatten()
                    .any(|v| v.len() != s.actuator_joints.len())
                || (!s.observation_names.is_empty()
                    && s.observation
                        .as_ref()
                        .is_none_or(|v| v.len() != s.observation_names.len()))
            {
                return Err("Telemetry names/dimensions or reward terms are invalid".into());
            }
        }
        Ok(())
    }
    pub fn included(&self) -> Vec<usize> {
        self.segments
            .iter()
            .filter(|s| !s.excluded)
            .flat_map(|s| s.keep_start..s.keep_end)
            .collect()
    }
    pub fn edit(
        &mut self,
        index: usize,
        start: usize,
        end: usize,
        annotation: String,
    ) -> Result<(), String> {
        let s = self.segments.get(index).ok_or("Select an episode")?;
        if start < s.start || end > s.end || start >= end || annotation.len() > 4096 {
            return Err(format!(
                "Keep range must be within {}..{} (end excluded); annotation limit is 4096 bytes",
                s.start, s.end
            ));
        }
        let s = &mut self.segments[index];
        s.keep_start = start;
        s.keep_end = end;
        s.annotation = annotation;
        self.revision += 1;
        Ok(())
    }
    pub fn split(&mut self, index: usize, at: usize) -> Result<(), String> {
        let s = self.segments.get(index).ok_or("Select an episode")?.clone();
        if at <= s.keep_start || at >= s.keep_end {
            return Err("Move the playhead inside the kept range before splitting".into());
        }
        let mut a = s.clone();
        a.end = at;
        a.keep_end = at;
        let mut b = s;
        b.start = at;
        b.keep_start = at;
        b.boundary = "manual split".into();
        self.segments.splice(index..=index, [a, b]);
        self.revision += 1;
        Ok(())
    }
}

pub fn training(
    path: &Path,
    run: &str,
    robot_id: Option<String>,
    robot: &str,
) -> Result<Document, String> {
    measured(path, run, robot_id, robot, "training", None)
}

pub fn simulation(
    path: &Path,
    run: &str,
    robot_id: Option<String>,
    robot: &str,
    checkpoint: &str,
) -> Result<Document, String> {
    measured(path, run, robot_id, robot, "simulation", Some(checkpoint))
}

fn measured(
    path: &Path,
    run: &str,
    robot_id: Option<String>,
    robot: &str,
    source: &str,
    checkpoint: Option<&str>,
) -> Result<Document, String> {
    let bytes = read_bytes(path)?;
    // A live worker may be in the middle of its final write. Only complete lines are imported.
    let end = bytes
        .iter()
        .rposition(|b| *b == b'\n')
        .map(|i| i + 1)
        .ok_or("No complete measured frames yet")?;
    let mut replay = Replay {
        name: format!("{robot} · {source} recording"),
        joint_names: vec![],
        joints: vec![],
        base: vec![],
        dt: 0.,
    };
    let mut samples: Vec<Sample> = vec![];
    let mut segments: Vec<Segment> = vec![];
    let mut simulation_identity: Option<(String, String)> = None;
    for line in bytes[..end]
        .split(|b| *b == b'\n')
        .filter(|l| !l.is_empty())
    {
        let v: serde_json::Value =
            serde_json::from_slice(line).map_err(|e| format!("Recording JSON: {e}"))?;
        if v["version"] != 1
            || v["run_id"].as_str() != Some(run)
            || v["robot"].as_str() != Some(robot)
            || v["source"] != source
            || v["env_index"] != 0
            || checkpoint.is_some_and(|sha| {
                v["checkpoint_sha256"].as_str() != Some(sha)
                    || v["robot_id"].as_str() != robot_id.as_deref()
            })
        {
            return Err("Measured recording identity or environment changed".into());
        }
        if checkpoint.is_some() {
            let session = v["session_id"]
                .as_str()
                .filter(|s| !s.is_empty())
                .ok_or("Missing simulation session identity")?;
            let policy = v["policy_sha256"]
                .as_str()
                .filter(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
                .ok_or("Missing simulation policy digest")?;
            let identity = (session.to_string(), policy.to_string());
            if simulation_identity
                .as_ref()
                .is_some_and(|prior| prior != &identity)
            {
                return Err("Simulation recording changed session or policy".into());
            }
            simulation_identity = Some(identity);
        }
        let pose: Replay = serde_json::from_value(v.clone()).map_err(|e| e.to_string())?;
        if !crate::external_motion::valid_pose(&pose) || pose.joints.len() != 1 {
            return Err("Invalid training pose".into());
        }
        let sample: Sample = serde_json::from_value(v).map_err(|e| e.to_string())?;
        let i = samples.len();
        if i == 0 {
            replay.joint_names = pose.joint_names.clone();
            replay.dt = pose.dt;
        }
        if replay.joint_names != pose.joint_names || (replay.dt - pose.dt).abs() > 1e-9 {
            return Err("Recording joint order or timestep changed".into());
        }
        let boundary = if let Some(prev) = samples.last() {
            if sample.sequence != prev.sequence + 1 {
                Some("missing control steps")
            } else if prev.terminal || sample.reset_count != prev.reset_count {
                Some("episode reset")
            } else {
                None
            }
        } else {
            Some("recording start")
        };
        if let Some(reason) = boundary {
            segments.push(segment(i, i + 1, reason));
        }
        let s = segments.last_mut().unwrap();
        s.end = i + 1;
        s.keep_end = i + 1;
        replay.joints.push(pose.joints[0].clone());
        replay.base.push(pose.base[0]);
        samples.push(sample);
        if i >= MAX_FRAMES {
            return Err("Recording exceeds 100,000 frames; split it before importing".into());
        }
    }
    let doc = Document {
        review_version: 1,
        revision: 1,
        robot_id,
        robot: robot.into(),
        run: Some(run.into()),
        source: path.to_string_lossy().into(),
        source_sha256: hash(&bytes[..end]),
        replay,
        samples,
        segments,
    };
    doc.validate()?;
    Ok(doc)
}
pub fn load(path: &Path) -> Result<Document, String> {
    let doc: Document = serde_json::from_slice(&read_bytes(path)?).map_err(|e| e.to_string())?;
    doc.validate()?;
    Ok(doc)
}
fn atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if bytes.len() as u64 > MAX_BYTES {
        return Err("Recording exceeds the 256 MiB review limit".into());
    }
    let parent = path.parent().ok_or("No recording directory")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let tmp = parent.join(format!(
        ".review-{}.tmp",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&tmp, path)?;
        fs::File::open(parent)?.sync_all()
    })()
    .map_err(|e: std::io::Error| e.to_string());
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}
fn encoded(doc: &Document) -> Result<Vec<u8>, String> {
    let mut value = serde_json::to_value(doc).map_err(|e| e.to_string())?;
    if !doc.samples.is_empty()
        && doc
            .samples
            .iter()
            .all(|s| s.joints.len() == 1 && s.base.len() == 1)
    {
        value["joints"] =
            serde_json::json!(doc.samples.iter().map(|s| &s.joints[0]).collect::<Vec<_>>());
        value["base"] =
            serde_json::json!(doc.samples.iter().map(|s| &s.base[0]).collect::<Vec<_>>());
    }
    serde_json::to_vec(&value).map_err(|e| e.to_string())
}
pub fn save(path: &Path, doc: &Document) -> Result<(), String> {
    doc.validate()?;
    atomic(path, &encoded(doc)?)
}

pub fn export(doc: &Document, destination: &Path) -> Result<(), String> {
    doc.validate()?;
    let indices = doc.included();
    if indices.is_empty() {
        return Err("All episodes are excluded".into());
    }
    if !destination.is_absolute() {
        return Err("Choose a new absolute destination folder".into());
    }
    fs::create_dir(destination).map_err(|e| format!("Destination must be a new folder: {e}"))?;
    let mut created = Vec::new();
    let result = (|| -> Result<(), String> {
        let mut edited = doc.clone();
        edited.replay.joints = indices
            .iter()
            .map(|&i| doc.replay.joints[i].clone())
            .collect();
        edited.replay.base = indices
            .iter()
            .filter_map(|&i| doc.replay.base.get(i).copied())
            .collect();
        edited.samples = indices
            .iter()
            .filter_map(|&i| doc.samples.get(i).cloned())
            .collect();
        let mut offset = 0;
        edited.segments = doc
            .segments
            .iter()
            .filter(|s| !s.excluded)
            .map(|s| {
                let n = s.keep_end - s.keep_start;
                let mut out = segment(offset, offset + n, &s.boundary);
                out.annotation = s.annotation.clone();
                offset += n;
                out
            })
            .collect();
        let manifest = serde_json::json!({"version":1,"source_sha256":doc.source_sha256,"review_revision":doc.revision,"source_frame_indices":indices,"original_segments":doc.segments,"robot_id":doc.robot_id,"run":doc.run});
        // Files are created exclusively; a failure only removes files created by this export.
        for (name, bytes) in [
            ("recording.json", encoded(&edited)?),
            (
                "provenance.json",
                serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?,
            ),
        ] {
            let mut f = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(destination.join(name))
                .map_err(|e| e.to_string())?;
            created.push(destination.join(name));
            f.write_all(&bytes)
                .and_then(|_| f.sync_all())
                .map_err(|e| e.to_string())?;
        }
        load(&destination.join("recording.json"))?;
        Ok(())
    })();
    if result.is_err() {
        for path in created {
            let _ = fs::remove_file(path);
        }
        let _ = fs::remove_dir(destination);
    }
    result
}

impl App {
    pub fn install_review(&mut self, doc: Document, recording: Option<String>) {
        self.simulation.take();
        self.st.sel.robot = doc.robot_id.clone().or_else(|| {
            crate::nexus::recording_robot(&self.st.robots, Some(&doc.robot), &doc.replay)
                .ok()
                .map(|r| r.id.clone())
        });
        self.st.sel.run = doc.run.clone().filter(|id| self.st.run(id).is_some());
        self.st.sel.ck = None;
        self.st.sel.ck_run = None;
        self.mj = None;
        self.cross = None;
        self.preview_key.clear();
        self.st.mode = Mode::Inspect;
        self.st.live.frames = doc.replay.joints.len() as u32;
        self.st.live.frame = 0;
        self.st.live.playing = false;
        self.st.replay_driven = true;
        self.replay = Some(doc.replay.clone());
        self.replay_map = None;
        self.review = State {
            document: Some(doc),
            recording,
            ..Default::default()
        };
    }
    pub fn register_review(&mut self, doc: Document) -> Result<(), String> {
        doc.validate()?;
        let id = format!(
            "rec{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let path = PathBuf::from(crate::nexus::repo_dir())
            .join("recordings")
            .join(format!("{id}.json"));
        save(&path, &doc)?;
        let dist = doc
            .segments
            .iter()
            .map(|s| {
                doc.replay
                    .base
                    .get(s.start..s.end)
                    .unwrap_or(&[])
                    .windows(2)
                    .map(|b| {
                        ((b[1][0] - b[0][0]).powi(2) + (b[1][1] - b[0][1]).powi(2)).sqrt() as f64
                    })
                    .sum::<f64>()
            })
            .sum();
        self.st.recordings.push(Recording {
            robot: Some(doc.robot.clone()),
            run: doc.run.clone(),
            id: id.clone(),
            name: doc.replay.name.clone(),
            scene: self.st.cur_scene().name.clone(),
            frames: doc.replay.joints.len() as u32,
            resets: doc
                .segments
                .iter()
                .filter(|s| s.boundary == "episode reset")
                .count() as u32,
            dist,
            path: Some(path.to_string_lossy().into()),
        });
        if let Err(e) = self
            .workspace_session
            .as_ref()
            .ok_or("Workspace is read-only")?
            .save(&self.st)
        {
            self.st.recordings.pop();
            let _ = fs::remove_file(path);
            return Err(e.to_string());
        }
        self.install_review(doc, Some(id));
        Ok(())
    }
    fn persist_review(&mut self, doc: Document) -> Result<(), String> {
        let id = self
            .review
            .recording
            .as_ref()
            .ok_or("Save this recording before editing")?;
        let path = self
            .st
            .recordings
            .iter()
            .find(|r| &r.id == id)
            .and_then(|r| r.path.as_ref())
            .ok_or("Recording is missing")?;
        save(Path::new(path), &doc)?;
        self.review.document = Some(doc);
        Ok(())
    }
    pub fn review_edit(
        &mut self,
        start: &str,
        end: &str,
        annotation: String,
    ) -> Result<(), String> {
        let mut doc = self.review.document.clone().ok_or("Open a recording")?;
        doc.edit(
            self.review.segment,
            start
                .trim()
                .parse()
                .map_err(|_| "Start must be a frame number")?,
            end.trim()
                .parse()
                .map_err(|_| "End must be a frame number")?,
            annotation,
        )?;
        self.persist_review(doc)
    }
    pub fn review_action(&mut self, act: &str, arg: &str) -> Result<(), String> {
        match act {
            "review-training" => {
                let run = self
                    .st
                    .sel
                    .run
                    .as_ref()
                    .and_then(|id| self.st.run(id))
                    .ok_or("Select a run first")?;
                let robot = run
                    .snapshot
                    .robot
                    .as_ref()
                    .ok_or("This run has no robot recording contract")?;
                let doc = training(
                    &crate::artifacts::runs_root()
                        .join(&run.id)
                        .join("training-motion.jsonl"),
                    &run.id,
                    run.snapshot.robot_id.clone(),
                    robot,
                )?;
                self.register_review(doc)?;
                self.st.toast(
                    "Saved the measured training stream. Original logs are preserved.".into(),
                );
            }
            "review-open" => {
                let rec = self
                    .st
                    .recordings
                    .iter()
                    .find(|r| r.id == arg)
                    .ok_or("Recording missing")?
                    .clone();
                let path = Path::new(rec.path.as_deref().ok_or("Recording has no file")?);
                let doc = match load(path) {
                    Ok(d) => d,
                    Err(e) => {
                        let value: serde_json::Value = serde_json::from_slice(&read_bytes(path)?)
                            .map_err(|e| e.to_string())?;
                        if value.get("review_version").is_some() {
                            return Err(e);
                        }
                        let replay = crate::nexus::load_rollout(path, &rec.name)
                            .ok_or("Invalid recording")?;
                        let robot = crate::nexus::recording_robot(
                            &self.st.robots,
                            rec.robot.as_deref(),
                            &replay,
                        )?;
                        Document::from_replay(
                            replay,
                            Some(robot.id.clone()),
                            robot.model_name().into(),
                            rec.run.clone(),
                        )?
                    }
                };
                if let Some(id) = &doc.robot_id {
                    let robot = self
                        .st
                        .robots
                        .iter()
                        .find(|r| &r.id == id)
                        .ok_or("Recording robot is no longer in the library")?;
                    let model =
                        urdf_rs::read_file(robot.urdf.as_ref().ok_or("Robot model is missing")?)
                            .map_err(|e| e.to_string())?;
                    if robot.model_name() != doc.robot
                        || doc
                            .replay
                            .joint_names
                            .iter()
                            .any(|n| !model.joints.iter().any(|j| &j.name == n))
                    {
                        return Err("Recording identity or joints do not match its robot".into());
                    }
                }
                let legacy: serde_json::Value =
                    serde_json::from_slice(&read_bytes(path)?).map_err(|e| e.to_string())?;
                if legacy.get("review_version").is_none() {
                    self.register_review(doc)?;
                } else {
                    self.install_review(doc, Some(rec.id));
                }
            }
            "review-channel" => {
                self.review.channel = arg.parse::<usize>().map_err(|_| "Invalid channel")?.min(7);
                self.review.page = 0;
            }
            "review-next-page" => {
                self.review.page += 1;
            }
            "review-prev-page" => {
                self.review.page = self.review.page.saturating_sub(1);
            }
            "review-episode" => {
                let i: usize = arg.parse().map_err(|_| "Invalid episode")?;
                let s = self
                    .review
                    .document
                    .as_ref()
                    .and_then(|d| d.segments.get(i))
                    .ok_or("Episode missing")?;
                self.review.segment = i;
                self.st.live.frame = s.keep_start as u32;
                self.st.live.playing = false;
            }
            "review-edit" => {
                if self.review.document.is_none() {
                    return Err("Open a recording".into());
                }
                self.st.modal = AppModal::ReviewEdit;
            }
            "review-export" => {
                if self.review.document.is_none() {
                    return Err("Open a recording".into());
                }
                self.st.modal = AppModal::ReviewExport;
            }
            "review-exclude" | "review-split" | "review-reset" => {
                let mut doc = self.review.document.clone().ok_or("Open a recording")?;
                if act == "review-split" {
                    doc.split(self.review.segment, self.st.live.frame as usize)?;
                } else {
                    let s = doc
                        .segments
                        .get_mut(self.review.segment)
                        .ok_or("Episode missing")?;
                    if act == "review-exclude" {
                        s.excluded = !s.excluded;
                    } else {
                        s.keep_start = s.start;
                        s.keep_end = s.end;
                        s.excluded = false;
                        s.annotation.clear();
                    }
                    doc.revision += 1;
                }
                self.persist_review(doc)?;
                self.st
                    .toast("Episode edit saved; original frames are retained.".into());
            }
            _ => return Err("Unknown review action".into()),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn root() -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "nexus-review-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&p).unwrap();
        p
    }
    fn stream(path: &Path) {
        let mut text = String::new();
        for i in 0..8 {
            let seq = if i >= 6 { i + 2 } else { i + 1 };
            let row = serde_json::json!({"version":1,"run_id":"run-test","robot":"fixture","source":"training","env_index":0,"sequence":seq,"sim_time":seq as f64*0.02,"reset_count":if i>=4{1}else{0},"terminal":i==3,"dt":0.02,"joint_names":["hinge"],"joints":[[i as f64*0.1]],"base":[[i as f64*0.01,0.,0.2,0.,0.,0.,1.]],"observation":[i as f64,1.],"observation_names":["hinge","command"],"action":[i as f64*0.2],"torques":[i as f64*0.3],"targets":[i as f64*0.4],"actuator_joints":["hinge"]});
            text.push_str(&row.to_string());
            text.push('\n');
        }
        text.push_str("{\"partial");
        fs::write(path, text).unwrap();
    }
    #[test]
    fn simulation_preserves_forces_and_policy_identity_and_rejects_mixed_sessions() {
        let root = root();
        let path = root.join("simulation-motion.jsonl");
        stream(&path);
        let sha = "a".repeat(64);
        let rows: Vec<serde_json::Value> = fs::read_to_string(&path)
            .unwrap()
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .map(|mut v| {
                v["source"] = "simulation".into();
                v["robot_id"] = "robot-id".into();
                v["checkpoint_sha256"] = sha.clone().into();
                v["policy_sha256"] = "b".repeat(64).into();
                v["session_id"] = "sim-test".into();
                v["force_world"] = serde_json::json!([1., 2., 0.]);
                v
            })
            .collect();
        let write = |rows: &[serde_json::Value]| {
            fs::write(
                &path,
                rows.iter().map(|v| format!("{v}\n")).collect::<String>(),
            )
            .unwrap()
        };
        write(&rows);
        let doc = simulation(&path, "run-test", Some("robot-id".into()), "fixture", &sha).unwrap();
        assert_eq!(doc.samples[0].force_world, Some(vec![1., 2., 0.]));
        assert_eq!(
            doc.samples[0].checkpoint_sha256.as_deref(),
            Some(sha.as_str())
        );
        assert!(training(&path, "run-test", Some("robot-id".into()), "fixture").is_err());
        assert!(simulation(&path, "run-test", Some("wrong".into()), "fixture", &sha).is_err());
        let destination = root.join("export");
        export(&doc, &destination).unwrap();
        let exported = load(&destination.join("recording.json")).unwrap();
        assert_eq!(exported.samples[0].force_world, doc.samples[0].force_world);
        assert_eq!(
            exported.samples[0].policy_sha256,
            doc.samples[0].policy_sha256
        );
        let mut mixed = rows;
        mixed[1]["session_id"] = "different".into();
        write(&mixed);
        assert!(simulation(&path, "run-test", Some("robot-id".into()), "fixture", &sha).is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn measured_stream_keeps_every_channel_and_segments_resets_and_missing_steps() {
        let root = root();
        let path = root.join("motion.jsonl");
        stream(&path);
        let doc = training(&path, "run-test", Some("robot-id".into()), "fixture").unwrap();
        assert_eq!(doc.samples.len(), 8);
        assert_eq!(
            doc.segments
                .iter()
                .map(|s| (s.start, s.end))
                .collect::<Vec<_>>(),
            vec![(0, 4), (4, 6), (6, 8)]
        );
        assert_eq!(doc.segments[2].boundary, "missing control steps");
        assert_eq!(doc.samples[5].torques, Some(vec![1.5]));
        assert!(training(&path, "another", None, "fixture").is_err());
        let mut bad = doc.clone();
        bad.samples[3].sequence = bad.samples[2].sequence;
        assert!(bad.validate().is_err());
        bad = doc.clone();
        bad.samples[3].torques = Some(vec![1., 2.]);
        assert!(bad.validate().is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn edits_survive_reopen_and_export_has_exact_source_frame_and_telemetry_mapping() {
        let root = root();
        let source = root.join("motion.jsonl");
        stream(&source);
        let bytes = fs::read(&source).unwrap();
        let mut doc = training(&source, "run-test", Some("robot-id".into()), "fixture").unwrap();
        doc.edit(0, 1, 3, "useful approach".into()).unwrap();
        doc.segments[1].excluded = true;
        assert!(doc.edit(0, 3, 2, "invalid".into()).is_err());
        assert_eq!(doc.segments[0].annotation, "useful approach");
        doc.split(2, 7).unwrap();
        let saved = root.join("recording.json");
        save(&saved, &doc).unwrap();
        let loaded = load(&saved).unwrap();
        assert_eq!(loaded.included(), vec![1, 2, 6, 7]);
        assert_eq!(loaded.segments.len(), 4);
        let out = root.join("edited");
        export(&loaded, &out).unwrap();
        let edited = load(&out.join("recording.json")).unwrap();
        assert_eq!(
            edited.replay.joints,
            vec![
                doc.replay.joints[1].clone(),
                doc.replay.joints[2].clone(),
                doc.replay.joints[6].clone(),
                doc.replay.joints[7].clone()
            ]
        );
        assert_eq!(
            edited
                .samples
                .iter()
                .map(|s| s.sequence)
                .collect::<Vec<_>>(),
            vec![2, 3, 8, 9]
        );
        let precision: serde_json::Value =
            serde_json::from_slice(&fs::read(out.join("recording.json")).unwrap()).unwrap();
        assert_eq!(
            precision["joints"][1][0].as_f64(),
            Some(doc.samples[2].joints[0][0])
        );
        assert_eq!(
            precision["base"][1][0].as_f64(),
            Some(doc.samples[2].base[0][0])
        );
        let manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(out.join("provenance.json")).unwrap()).unwrap();
        assert_eq!(
            manifest["source_frame_indices"],
            serde_json::json!([1, 2, 6, 7])
        );
        assert_eq!(edited.robot_id.as_deref(), Some("robot-id"));
        assert_eq!(fs::read(&source).unwrap(), bytes);
        let exported = fs::read(out.join("recording.json")).unwrap();
        assert!(export(&loaded, &out).is_err());
        assert_eq!(fs::read(out.join("recording.json")).unwrap(), exported);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn failed_save_preserves_recording_and_invalid_edits_or_empty_export_do_not_commit() {
        let root = root();
        let source = root.join("motion.jsonl");
        stream(&source);
        let mut doc = training(&source, "run-test", None, "fixture").unwrap();
        let path = root.join("recording.json");
        save(&path, &doc).unwrap();
        let before = fs::read(&path).unwrap();
        doc.segments[0].end = 5;
        assert!(save(&path, &doc).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
        doc = load(&path).unwrap();
        for s in &mut doc.segments {
            s.excluded = true;
        }
        assert!(export(&doc, &root.join("empty")).is_err());
        assert!(!root.join("empty").exists());
        let blocked = root.join("file-not-directory");
        fs::write(&blocked, b"keep").unwrap();
        assert!(save(&blocked.join("child"), &doc).is_err());
        assert_eq!(fs::read(blocked).unwrap(), b"keep");
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn snapshot_identity_survives_duplicate_model_imports() {
        let mut st = Store::seed();
        let mut duplicate = st.robots[0].clone();
        duplicate.id = "separate-id".into();
        st.robots.push(duplicate);
        let mut snapshot = st.runs[0].snapshot.clone();
        snapshot.robot = Some(st.robots[0].model_name().into());
        snapshot.robot_id = Some("separate-id".into());
        assert_eq!(st.snapshot_robot(&snapshot).as_deref(), Some("separate-id"));
        snapshot.robot_id = None;
        assert_eq!(st.snapshot_robot(&snapshot), None);
        snapshot.robot_id = Some("missing-id".into());
        assert_eq!(st.snapshot_robot(&snapshot), None);
    }
}
