//! Artifact discovery and reversible, journaled filesystem changes.
//!
//! The workspace document is the commit point. Before it commits, recovery
//! returns files to their original locations. Afterwards it keeps them in trash.
//! Both deletion and restore use this rule, including an interruption mid-move.
use crate::state::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FileEntry {
    pub relative: PathBuf,
    pub bytes: u64,
    fingerprint: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Removal {
    Robot {
        robot: Robot,
        index: usize,
    },
    Checkpoints {
        run: String,
        checkpoints: Vec<(usize, Ckpt)>,
    },
    Run {
        run: Run,
        index: usize,
        kept: Vec<Ckpt>,
    },
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Storage {
    #[default]
    Runs,
    Robots,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrashEntry {
    #[serde(default)]
    pub storage: Storage,
    #[serde(default)]
    pub purging: bool,
    pub id: String,
    pub label: String,
    pub created_ms: u64,
    pub files: Vec<FileEntry>,
    pub removal: Removal,
}
#[derive(Clone, Debug)]
pub struct Plan {
    pub entry: TrashEntry,
    pub protected: Vec<String>,
}
impl Plan {
    pub fn bytes(&self) -> u64 {
        self.entry.files.iter().map(|f| f.bytes).sum()
    }
    pub fn description(&self) -> String {
        let mut lines = vec![format!("{} files · {:.2} MiB will move to workspace trash. Restore remains available after restart.", self.entry.files.len(), self.bytes() as f64 / 1048576.0)];
        lines.extend(
            self.entry
                .files
                .iter()
                .map(|f| format!("{} · {} bytes", f.relative.display(), f.bytes)),
        );
        if !self.protected.is_empty() {
            lines.push("Retained:".into());
            lines.extend(self.protected.clone());
        }
        lines.join("\n")
    }
}

fn err(message: impl Into<String>) -> io::Error {
    io::Error::other(message.into())
}
fn fingerprint(path: &Path) -> io::Result<(u64, u64)> {
    let mut file = fs::File::open(path)?;
    let (mut hash, mut size) = (0xcbf29ce484222325u64, 0u64);
    let mut buf = [0; 65536];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        size += n as u64;
        for b in &buf[..n] {
            hash = (hash ^ *b as u64).wrapping_mul(0x100000001b3);
        }
    }
    Ok((size, hash))
}
fn normal(path: &Path) -> bool {
    !path.as_os_str().is_empty() && path.components().all(|c| matches!(c, Component::Normal(_)))
}
/// All traversed components must be real directories/files, never symlinks.
fn checked(root: &Path, relative: &Path) -> io::Result<PathBuf> {
    if !normal(relative) {
        return Err(err("Invalid artifact path"));
    }
    let mut path = root.to_path_buf();
    for c in relative.components() {
        path.push(c);
        match fs::symlink_metadata(&path) {
            Ok(m) if m.file_type().is_symlink() => {
                return Err(err(format!(
                    "Linked artifacts are read-only: {}",
                    path.display()
                )))
            }
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    Ok(path)
}
fn relative(root: &Path, path: &Path) -> io::Result<PathBuf> {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    // macOS exposes /tmp and /var through system aliases. Resolve ancestors
    // only up to our ownership boundary, preserving every component below it
    // so an artifact symlink still fails checked().
    let boundary = path
        .ancestors()
        .find(|p| p.canonicalize().ok().as_deref() == Some(root))
        .ok_or_else(|| err("Artifact is outside the workspace's run storage; it is read-only"))?;
    let rel = path
        .strip_prefix(boundary)
        .map_err(|_| err("Invalid artifact path"))?
        .to_path_buf();
    checked(root, &rel)?;
    Ok(rel)
}
fn collect(dir: &Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for e in fs::read_dir(dir)? {
        let e = e?;
        let ty = e.file_type()?;
        if ty.is_symlink() {
            return Err(err(format!(
                "Linked artifacts are read-only: {}",
                e.path().display()
            )));
        }
        if ty.is_dir() {
            collect(&e.path(), out)?;
        }
        if ty.is_file() {
            out.push(e.path());
        }
    }
    Ok(())
}
fn key(path: &str) -> PathBuf {
    Path::new(path)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(path))
}
fn profile_paths(value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::String(s) if Path::new(s).is_absolute() && Path::new(s).is_file() => {
            out.push(s.clone())
        }
        serde_json::Value::Array(v) => v.iter().for_each(|v| profile_paths(v, out)),
        serde_json::Value::Object(v) => v.values().for_each(|v| profile_paths(v, out)),
        _ => {}
    }
}
fn references(st: &Store, excluded: Option<&str>) -> BTreeMap<PathBuf, String> {
    let mut out = BTreeMap::new();
    for run in st.runs.iter().filter(|r| Some(r.id.as_str()) != excluded) {
        let mut paths = run.snapshot.warm.iter().cloned().collect::<Vec<_>>();
        if let Some(p) = &run.snapshot.training_profile {
            profile_paths(p, &mut paths);
        }
        for p in paths {
            out.insert(key(&p), format!("saved recipe: {}", run.name));
        }
        for ck in &run.ckpts {
            out.insert(key(&ck.id), format!("checkpoint in {}", run.name));
        }
    }
    for rec in &st.recordings {
        if let Some(path) = &rec.path {
            out.insert(key(path), format!("recording: {}", rec.name));
        }
    }
    out
}
fn checkpoint_files(path: &Path) -> Vec<PathBuf> {
    let mut out = vec![path.to_path_buf()];
    if path.extension().is_some_and(|e| e == "bin") && path.with_extension("txt").is_file() {
        out.push(path.with_extension("txt"));
    }
    out
}
fn new_entry(label: String, files: Vec<FileEntry>, removal: Removal) -> TrashEntry {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let serial = SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    TrashEntry {
        storage: Storage::Runs,
        purging: false,
        id: format!("{}-{}-{serial}", now.as_nanos(), std::process::id()),
        label,
        created_ms: now.as_millis() as u64,
        files,
        removal,
    }
}
fn entries(root: &Path, paths: Vec<PathBuf>) -> io::Result<Vec<FileEntry>> {
    let mut out = vec![];
    for path in paths.into_iter().collect::<BTreeSet<_>>() {
        let relative = relative(root, &path)?;
        let (bytes, fingerprint) = fingerprint(&path)?;
        out.push(FileEntry {
            relative,
            bytes,
            fingerprint,
        });
    }
    Ok(out)
}

/// Preview only. No files or metadata change until Session::remove_artifacts.
pub fn checkpoint_plan(
    st: &Store,
    runs_root: &Path,
    run_id: &str,
    selected: Option<&str>,
    keep_last: usize,
) -> io::Result<Plan> {
    let run = st.run(run_id).ok_or_else(|| err("Select a run"))?;
    if matches!(
        run.state,
        RunState::Running
            | RunState::Paused
            | RunState::Queued
            | RunState::Starting
            | RunState::Stopping
            | RunState::Unknown
    ) {
        return Err(err("Stop the run before cleaning its artifacts"));
    }
    let root = runs_root.canonicalize()?;
    let refs = references(st, Some(run_id));
    let mut ordered: Vec<_> = run
        .ckpts
        .iter()
        .filter(|c| Path::new(&c.id).is_file())
        .collect();
    ordered.sort_by_key(|c| fs::metadata(&c.id).and_then(|m| m.modified()).ok());
    let newest: BTreeSet<_> = ordered
        .iter()
        .rev()
        .take(keep_last)
        .map(|c| c.id.as_str())
        .collect();
    let best = run
        .ckpts
        .iter()
        .filter(|c| c.score_known)
        .max_by(|a, b| a.score.total_cmp(&b.score))
        .map(|c| c.id.as_str());
    let mut protected = vec![];
    let mut removed = vec![];
    let mut paths = vec![];
    for (index, ck) in run.ckpts.iter().enumerate() {
        if selected.is_some_and(|id| id != ck.id) {
            continue;
        }
        let reason = if ck.st == CkSt::Prom {
            Some("promoted".to_string())
        } else if let Some(reason) = refs.get(&key(&ck.id)) {
            Some(reason.clone())
        } else if selected.is_none()
            && (newest.contains(ck.id.as_str())
                || Some(ck.id.as_str()) == best
                || ck.st == CkSt::Best)
        {
            Some("newest or best measured reward".into())
        } else if selected.is_none() && Path::new(&ck.id).file_stem().is_some_and(|s| s == "policy")
        {
            Some("final policy and export provenance".into())
        } else {
            None
        };
        if let Some(reason) = reason {
            if selected.is_some() {
                return Err(err(format!("{} is protected: {reason}", ck.label)));
            }
            protected.push(format!("{} — {reason}", ck.label));
            continue;
        }
        // Check ownership even for a missing file: a ledger entry outside the
        // root cannot authorize file operations there.
        relative(&root, Path::new(&ck.id))?;
        paths.extend(
            checkpoint_files(Path::new(&ck.id))
                .into_iter()
                .filter(|p| p.is_file()),
        );
        removed.push((index, ck.clone()));
    }
    if removed.is_empty() {
        return Err(err("No unprotected checkpoints match this cleanup"));
    }
    Ok(Plan {
        entry: new_entry(
            format!("{} · {} checkpoints", run.name, removed.len()),
            entries(&root, paths)?,
            Removal::Checkpoints {
                run: run_id.into(),
                checkpoints: removed,
            },
        ),
        protected,
    })
}

pub fn run_plan(st: &Store, runs_root: &Path, id: &str) -> io::Result<Plan> {
    let index = st
        .runs
        .iter()
        .position(|r| r.id == id)
        .ok_or_else(|| err("Select a run"))?;
    let run = &st.runs[index];
    if matches!(
        run.state,
        RunState::Running
            | RunState::Paused
            | RunState::Queued
            | RunState::Starting
            | RunState::Stopping
            | RunState::Unknown
    ) {
        return Err(err("Stop the run before deleting it"));
    }
    if run.coll && run.ckpts.iter().any(|c| c.st == CkSt::Prom) {
        return Err(err(
            "Demote protected checkpoints before deleting their retained collection",
        ));
    }
    let root = runs_root.canonicalize()?;
    let dir = checked(&root, Path::new(id))?;
    let refs = references(st, Some(id));
    let kept: Vec<_> = run
        .ckpts
        .iter()
        .filter(|c| c.st == CkSt::Prom || refs.contains_key(&key(&c.id)))
        .cloned()
        .collect();
    let mut protected: Vec<_> = kept
        .iter()
        .map(|c| format!("{} — promoted or referenced; retained collection", c.label))
        .collect();
    let mut files = vec![];
    if kept.is_empty() {
        collect(&dir, &mut files)?;
        files.retain(|p| {
            if let Some(reason) = refs.get(&key(&p.to_string_lossy())) {
                protected.push(format!("{} — {reason}", p.display()));
                false
            } else {
                true
            }
        });
    } else {
        // A retained policy needs its model, export provenance, logs and task
        // configuration. Only ordinary weights can be removed in this case.
        for ck in run
            .ckpts
            .iter()
            .filter(|c| !kept.iter().any(|k| k.id == c.id))
        {
            files.extend(
                checkpoint_files(Path::new(&ck.id))
                    .into_iter()
                    .filter(|p| p.is_file()),
            );
        }
        protected.push(
            "Run metadata, exports and recordings retained with protected checkpoints".into(),
        );
    }
    Ok(Plan {
        entry: new_entry(
            run.name.clone(),
            entries(&root, files)?,
            Removal::Run {
                run: run.clone(),
                index,
                kept,
            },
        ),
        protected,
    })
}

pub fn robot_plan(st: &Store, root: &Path, id: &str) -> io::Result<Plan> {
    if !nexus_engine::ckpt::valid_id(id) || fs::symlink_metadata(root)?.file_type().is_symlink() {
        return Err(err("Invalid robot identity or linked robot library"));
    }
    let index = st
        .robots
        .iter()
        .position(|r| r.id == id)
        .ok_or_else(|| err("Select a robot"))?;
    let robot = &st.robots[index];
    let robot_refs = st.robot_references(id);
    if !robot_refs.is_empty() {
        return Err(err(format!(
            "Robot is still referenced by {}. Remove or restore those references first.",
            robot_refs.join(", ")
        )));
    }
    let mut files = vec![];
    let mut protected = vec![];
    let directory = checked(root, Path::new(id))?;
    // Only this robot's owned directory is eligible. External source files
    // and built-in models remain untouched when their library entry is removed.
    let owned = robot.urdf.as_ref().is_some_and(|p| {
        relative(root, Path::new(p))
            .ok()
            .is_some_and(|p| p.starts_with(id))
    });
    if owned {
        collect(&directory, &mut files)?;
        let mut dependencies = references(st, None);
        for entry in &st.trash {
            if let Removal::Run { run, .. } = &entry.removal {
                if let Some(profile) = &run.snapshot.training_profile {
                    let mut paths = vec![];
                    profile_paths(profile, &mut paths);
                    for p in paths {
                        dependencies.insert(key(&p), format!("recoverable recipe: {}", run.name));
                    }
                }
            }
        }
        if let Some((_, reason)) = dependencies
            .iter()
            .find(|(path, _)| path.starts_with(&directory))
        {
            return Err(err(format!("Robot asset files are still used by {reason}")));
        }
        for other in st.robots.iter().filter(|r| r.id != id) {
            let uses = other.urdf.iter().chain(other.task.iter().map(|t| &t.model));
            if uses.into_iter().any(|p| key(p).starts_with(&directory)) {
                return Err(err(format!("Robot assets are also used by {}", other.name)));
            }
        }
    } else {
        protected.push("External or built-in model files remain in their source location".into());
    }
    let mut entry = new_entry(
        format!("Robot · {}", robot.name),
        entries(root, files)?,
        Removal::Robot {
            robot: robot.clone(),
            index,
        },
    );
    entry.storage = Storage::Robots;
    Ok(Plan { entry, protected })
}

pub fn apply_removal(st: &mut Store, entry: &TrashEntry) {
    match &entry.removal {
        Removal::Robot { robot, .. } => {
            st.robots.retain(|r| r.id != robot.id);
            st.sel.robot = st.robots.first().map(|r| r.id.clone());
        }
        Removal::Checkpoints { run, checkpoints } => {
            if let Some(r) = st.run_mut(run) {
                r.ckpts
                    .retain(|c| !checkpoints.iter().any(|(_, removed)| removed.id == c.id));
            }
        }
        Removal::Run { run, kept, .. } => {
            st.runs.retain(|r| r.id != run.id);
            if !kept.is_empty() {
                let mut collection = run.clone();
                // Keep the source identity so its saved task and validation paths remain usable.
                collection.id = run.id.clone();
                collection.name = format!("Retained · {}", run.name);
                collection.coll = true;
                collection.archived = true;
                collection.ckpts = kept.clone();
                st.runs.push(collection);
            }
            st.sel.run = st.runs.first().map(|r| r.id.clone());
        }
    }
    st.sel.ck = None;
    st.sel.ck_run = None;
    st.dg_invalidate(None);
    st.trash.push(entry.clone());
}
pub fn apply_restore(st: &mut Store, entry: &TrashEntry) -> io::Result<()> {
    match &entry.removal {
        Removal::Robot { robot, index } => {
            if st
                .robots
                .iter()
                .any(|r| r.id == robot.id || r.name.eq_ignore_ascii_case(&robot.name))
            {
                return Err(err("Robot identity or name is already in the library; rename the current entry before restoring"));
            }
            st.robots
                .insert((*index).min(st.robots.len()), robot.clone());
            st.sel.robot = Some(robot.id.clone());
        }
        Removal::Checkpoints { run, checkpoints } => {
            let r = st
                .run_mut(run)
                .ok_or_else(|| err("Restore the containing run first"))?;
            for (index, ck) in checkpoints {
                if !r.ckpts.iter().any(|c| c.id == ck.id) {
                    r.ckpts.insert((*index).min(r.ckpts.len()), ck.clone());
                }
            }
            st.sel.run = Some(run.clone());
        }
        Removal::Run { run, index, kept } => {
            if let Some(existing) = st.run(&run.id) {
                if !existing.coll
                    || serde_json::to_value(&existing.ckpts)? != serde_json::to_value(kept)?
                    || serde_json::to_value(&existing.snapshot)?
                        != serde_json::to_value(&run.snapshot)?
                {
                    return Err(err(
                        "Run identity or retained policies changed; restore would overwrite edits",
                    ));
                }
            }
            st.runs
                .retain(|r| r.id != run.id && r.id != format!("kept-{}", entry.id));
            st.runs.insert((*index).min(st.runs.len()), run.clone());
            st.sel.run = Some(run.id.clone());
        }
    }
    st.trash.retain(|e| e.id != entry.id);
    st.sel.ck = None;
    st.sel.ck_run = None;
    Ok(())
}

fn trash_dir(root: &Path, id: &str) -> io::Result<PathBuf> {
    if !nexus_engine::ckpt::valid_id(id) {
        return Err(err("Invalid trash identity"));
    }
    checked(root, &PathBuf::from(".studio-trash").join(id))
}
fn atomic_json(path: &Path, value: &impl Serialize) -> io::Result<()> {
    let temp = path.with_extension("tmp");
    let mut file = fs::File::create(&temp)?;
    file.write_all(&serde_json::to_vec_pretty(value)?)?;
    file.sync_all()?;
    fs::rename(temp, path)
}
pub fn prepare(root: &Path, entry: &TrashEntry) -> io::Result<()> {
    // Check every preview fingerprint before any mutation.
    for file in &entry.files {
        let source = checked(root, &file.relative)?;
        if fingerprint(&source)? != (file.bytes, file.fingerprint) {
            return Err(err(
                "Artifact changed since preview. Refresh the cleanup plan.",
            ));
        }
    }
    let dir = trash_dir(root, &entry.id)?;
    fs::create_dir_all(&dir)?;
    atomic_json(&dir.join("journal.json"), entry)?;
    match reconcile(root, entry, true) {
        Ok(()) => Ok(()),
        Err(e) => {
            reconcile(root, entry, false)
                .map_err(|rollback| err(format!("{e}; recovery pending: {rollback}")))?;
            Err(e)
        }
    }
}
/// Idempotent and collision-intolerant: recovery never overwrites a new file.
pub fn reconcile(root: &Path, entry: &TrashEntry, in_trash: bool) -> io::Result<()> {
    let dir = trash_dir(root, &entry.id)?;
    let mut moves = vec![];
    for file in &entry.files {
        let original = checked(root, &file.relative)?;
        let stored = checked(&dir, &PathBuf::from("files").join(&file.relative))?;
        let (source, dest) = if in_trash {
            (original, stored)
        } else {
            (stored, original)
        };
        if dest.exists() {
            if source.exists() || fingerprint(&dest)? != (file.bytes, file.fingerprint) {
                return Err(err(format!(
                    "Recovery collision at {}; both versions preserved",
                    dest.display()
                )));
            }
            continue;
        }
        if fingerprint(&source)? != (file.bytes, file.fingerprint) {
            return Err(err(format!(
                "Artifact contents changed: {}",
                source.display()
            )));
        }
        moves.push((source, dest));
    }
    for (source, dest) in moves {
        fs::create_dir_all(dest.parent().unwrap())?;
        fs::rename(source, dest)?;
    }
    Ok(())
}
pub fn finish_restore(root: &Path, entry: &TrashEntry) -> io::Result<()> {
    fs::remove_dir_all(trash_dir(root, &entry.id)?)
}
pub fn mark_purge(root: &Path, entry: &TrashEntry, purging: bool) -> io::Result<()> {
    let mut journal = entry.clone();
    journal.purging = purging;
    atomic_json(&trash_dir(root, &entry.id)?.join("journal.json"), &journal)
}
pub fn check_purge(root: &Path, entry: &TrashEntry, allow_missing: bool) -> io::Result<()> {
    let dir = trash_dir(root, &entry.id)?;
    let mut expected = BTreeSet::new();
    for file in &entry.files {
        let path = checked(&dir, &PathBuf::from("files").join(&file.relative))?;
        if (!allow_missing || path.exists())
            && fingerprint(&path)? != (file.bytes, file.fingerprint)
        {
            return Err(err(format!(
                "Trash contents changed at {}; preserved for review",
                path.display()
            )));
        }
        expected.insert(path);
    }
    let mut actual = vec![];
    collect(&dir.join("files"), &mut actual)?;
    if actual.iter().any(|p| !expected.contains(p)) {
        return Err(err("Unexpected file in trash; preserved for review"));
    }
    Ok(())
}
pub fn finish_purge(root: &Path, entry: &TrashEntry) -> io::Result<()> {
    let dir = trash_dir(root, &entry.id)?;
    // Retain the journal until every artifact has gone, so an interrupted
    // purge continues on startup. Unknown files are not silently discarded.
    check_purge(root, entry, true)?;
    for file in &entry.files {
        let path = checked(&dir, &PathBuf::from("files").join(&file.relative))?;
        if path.is_file() {
            fs::remove_file(path)?;
        }
    }
    fn empty_dirs(path: &Path) -> io::Result<()> {
        if !path.exists() {
            return Ok(());
        }
        for child in fs::read_dir(path)? {
            let child = child?;
            if !child.file_type()?.is_dir() {
                return Err(err("Unexpected file in trash; preserved for review"));
            }
            empty_dirs(&child.path())?;
        }
        fs::remove_dir(path)
    }
    empty_dirs(&dir.join("files"))?;
    fs::remove_file(dir.join("journal.json"))?;
    fs::remove_dir(dir)
}
pub fn recover(root: &Path, committed: &[String]) -> io::Result<()> {
    let trash = checked(root, Path::new(".studio-trash"))?;
    if !trash.is_dir() {
        return Ok(());
    }
    for dir in fs::read_dir(trash)? {
        let dir = dir?;
        let path = trash_dir(root, &dir.file_name().to_string_lossy())?;
        let journal = path.join("journal.json");
        if !journal.is_file() {
            continue;
        }
        let entry: TrashEntry = serde_json::from_slice(&fs::read(&journal)?)?;
        if path != trash_dir(root, &entry.id)? {
            return Err(err("Trash journal identity mismatch"));
        }
        let deleted = committed.contains(&entry.id);
        if entry.purging && !deleted {
            finish_purge(root, &entry)?;
            continue;
        }
        reconcile(root, &entry, deleted)?;
        if entry.purging && deleted {
            mark_purge(root, &entry, false)?;
        }
        if !deleted {
            finish_restore(root, &entry)?;
        }
    }
    Ok(())
}

pub fn is_safetensors(path: &str) -> bool {
    let name = Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    name.ends_with(".safetensors")
        || name
            .rsplit_once(".safetensors.iter")
            .is_some_and(|(_, n)| !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()))
}

/// Recover saved run records that are missing from the ledger. A launch-time
/// record cannot prove completion, so imported jobs are explicitly stopped.
pub fn discover_runs(st: &mut Store, root: &Path) -> io::Result<usize> {
    let mut count = 0;
    if !root.is_dir() {
        return Ok(0);
    }
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let id = entry.file_name().to_string_lossy().into_owned();
        if !entry.file_type()?.is_dir()
            || !nexus_engine::ckpt::valid_id(&id)
            || st.run(&id).is_some()
        {
            continue;
        }
        if st
            .trash
            .iter()
            .any(|e| matches!(&e.removal, Removal::Run { run, .. } if run.id == id))
        {
            continue;
        }
        let path = entry.path().join("run.json");
        if !path.is_file() {
            continue;
        }
        let mut run: Run = serde_json::from_slice(&fs::read(path)?)?;
        if run.id != id {
            continue;
        }
        if run.snapshot.backend.is_none() && entry.path().join("training-profile.json").is_file() {
            run.snapshot.training_profile = Some(serde_json::from_slice(&fs::read(
                entry.path().join("training-profile.json"),
            )?)?);
            run.snapshot.backend = Some("external".into());
        }
        run.state = RunState::Stopped;
        run.diagnosis = Some("Recovered saved run artifacts. The original worker's final status is unavailable; validate a checkpoint before continuing.".into());
        run.snapshot.robot_id = st
            .robots
            .iter()
            .find(|r| Some(r.model_name()) == run.snapshot.robot.as_deref())
            .map(|r| r.id.clone());
        discover(&mut run, &entry.path())?;
        st.runs.push(run);
        count += 1;
    }
    Ok(count)
}

/// Only known checkpoint locations are searched; input and smoke policies are
/// never offered as outputs. Scores without matching evidence stay unknown.
pub fn discover(run: &mut Run, dir: &Path) -> io::Result<usize> {
    let mut paths = vec![];
    for folder in [dir.to_path_buf(), dir.join("training")] {
        if !folder.is_dir() {
            continue;
        }
        for e in fs::read_dir(folder)? {
            let e = e?;
            if !e.file_type()?.is_file() {
                continue;
            }
            let p = e.path();
            let ext = p.extension().and_then(|s| s.to_str()).unwrap_or("");
            let name = e.file_name().to_string_lossy().into_owned();
            if ext == "bin"
                || is_safetensors(&name)
                || (ext == "pt" && (name == "policy.pt" || name.starts_with("model_")))
            {
                paths.push(p);
            }
        }
    }
    let mut samples = BTreeMap::new();
    if let Ok(log) = fs::read_to_string(dir.join("metrics.jsonl")) {
        for line in log.lines() {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
                if v["event"] == "sample" {
                    if let (Some(iter), Some(reward)) = (
                        v["total_iteration"]
                            .as_u64()
                            .or_else(|| v["iteration"].as_u64()),
                        v["reward"].as_f64(),
                    ) {
                        samples.insert(iter, reward);
                    }
                }
            }
        }
    }
    let mut added = 0;
    for path in paths {
        let id = path.canonicalize()?.to_string_lossy().into_owned();
        if run.ckpts.iter().any(|c| key(&c.id) == Path::new(&id)) {
            continue;
        }
        let label = path.file_name().unwrap().to_string_lossy().into_owned();
        let score = if path.extension().is_some_and(|s| s == "bin") {
            nexus_engine::ckpt::read(&path).ok().map(|(_, m)| m.score)
        } else if let Some(n) = path
            .file_stem()
            .and_then(|s| s.to_str())
            .and_then(|s| s.strip_prefix("model_"))
            .and_then(|s| s.parse::<u64>().ok())
        {
            samples.get(&(n + 1)).copied()
        } else {
            None
        };
        run.ckpts.push(Ckpt {
            id,
            label,
            score: score.unwrap_or(0.0),
            score_known: score.is_some(),
            st: CkSt::Ord,
            pname: None,
        });
        added += 1;
    }
    run.ckpts
        .sort_by_key(|c| std::cmp::Reverse(fs::metadata(&c.id).and_then(|m| m.modified()).ok()));
    Ok(added)
}

pub fn runs_root() -> PathBuf {
    nexus_engine::ckpt::run_dir("storage-root")
        .parent()
        .unwrap()
        .to_path_buf()
}

pub fn validate_plan(st: &Store, root: &Path, plan: &Plan) -> io::Result<()> {
    match &plan.entry.removal {
        Removal::Robot { robot, .. } => {
            let current = st
                .robots
                .iter()
                .find(|r| r.id == robot.id)
                .ok_or_else(|| err("Robot changed since preview"))?;
            if serde_json::to_value(current)? != serde_json::to_value(robot)? {
                return Err(err("Robot changed since preview. Review a new plan."));
            }
            let fresh = robot_plan(st, root, &robot.id)?;
            if plan.entry.storage != Storage::Robots
                || serde_json::to_value(&fresh.entry.files)?
                    != serde_json::to_value(&plan.entry.files)?
            {
                return Err(err("Robot files or references changed since preview"));
            }
        }
        Removal::Run { run, .. } => {
            let current = st
                .run(&run.id)
                .ok_or_else(|| err("Run changed since preview"))?;
            if serde_json::to_value(current)? != serde_json::to_value(run)? {
                return Err(err("Run changed since preview. Review a new plan."));
            }
            let current_plan = run_plan(st, root, &run.id)?;
            let refs = references(st, Some(&run.id));
            if plan
                .entry
                .files
                .iter()
                .any(|f| refs.contains_key(&key(&root.join(&f.relative).to_string_lossy())))
                || current_plan.entry.files.len() != plan.entry.files.len()
            {
                return Err(err("Artifact references changed since preview"));
            }
        }
        Removal::Checkpoints { run, checkpoints } => {
            let current = st
                .run(run)
                .ok_or_else(|| err("Run changed since preview"))?;
            if matches!(
                current.state,
                RunState::Running
                    | RunState::Paused
                    | RunState::Queued
                    | RunState::Starting
                    | RunState::Stopping
                    | RunState::Unknown
            ) {
                return Err(err("Run is active; cleanup is blocked"));
            }
            let refs = references(st, Some(run));
            for (_, ck) in checkpoints {
                let now = current
                    .ckpts
                    .iter()
                    .find(|c| c.id == ck.id)
                    .ok_or_else(|| err("Checkpoint changed since preview"))?;
                if now.st == CkSt::Prom || refs.contains_key(&key(&ck.id)) {
                    return Err(err(
                        "Checkpoint gained a protected reference; review a new plan",
                    ));
                }
            }
        }
    }
    Ok(())
}

impl crate::App {
    pub fn preview_robot_removal(&mut self) {
        let Some(id) = self.st.sel.robot.clone() else {
            return;
        };
        let root = PathBuf::from(crate::nexus::repo_dir()).join("robots");
        let root = root
            .parent()
            .and_then(|p| p.canonicalize().ok())
            .map(|p| p.join("robots"))
            .unwrap_or(root);
        match robot_plan(&self.st, &root, &id) {
            Ok(plan) => {
                self.artifact_plan = Some(plan);
                self.st.modal = AppModal::ArtifactCleanup { page: 0 };
            }
            Err(e) => self.st.toast(e.to_string()),
        }
    }
    pub fn refresh_artifacts(&mut self) {
        if self.st.demo {
            return;
        }
        let mut added = 0;
        let mut errors = vec![];
        for run in self.st.runs.iter_mut().filter(|r| !r.coll) {
            match discover(run, &nexus_engine::ckpt::run_dir(&run.id)) {
                Ok(n) => added += n,
                Err(e) => errors.push(format!("{}: {e}", run.name)),
            }
        }
        if !errors.is_empty() {
            self.st
                .toast(format!("Artifact scan failed: {}", errors.join("; ")));
        } else if added > 0 {
            self.st
                .toast(format!("Discovered {added} saved checkpoints"));
        }
    }
    pub fn preview_artifacts(&mut self, selected: Option<&str>, whole_run: bool) {
        let Some(id) = self.st.sel.run.clone() else {
            self.st.toast("Select a run".into());
            return;
        };
        if self
            .real_proc
            .as_ref()
            .is_some_and(|p| p.run_id.as_ref() == Some(&id))
        {
            self.st
                .toast("Wait for this run’s worker to stop before cleanup".into());
            return;
        }
        if self
            .export_job
            .as_ref()
            .is_some_and(|job| job.run.as_ref() == Some(&id))
        {
            self.st
                .toast("Finish or cancel this run’s export before cleanup".into());
            return;
        }
        let plan = if whole_run {
            run_plan(&self.st, &runs_root(), &id)
        } else {
            checkpoint_plan(
                &self.st,
                &runs_root(),
                &id,
                selected,
                self.st.retention_keep,
            )
        };
        match plan {
            Ok(plan) => {
                self.artifact_plan = Some(plan);
                self.st.modal = AppModal::ArtifactCleanup { page: 0 };
            }
            Err(e) => self.st.toast(e.to_string()),
        }
    }
    pub fn commit_artifact_cleanup(&mut self) {
        let Some(plan) = self.artifact_plan.as_ref() else {
            return;
        };
        let owner = match &plan.entry.removal {
            Removal::Run { run, .. } => &run.id,
            Removal::Checkpoints { run, .. } => run,
            Removal::Robot { .. } => "",
        };
        if self
            .real_proc
            .as_ref()
            .is_some_and(|p| p.run_id.as_deref() == Some(owner))
        {
            self.st
                .toast("Wait for this run’s worker to stop before cleanup".into());
            return;
        }
        if self
            .export_job
            .as_ref()
            .is_some_and(|job| job.run.as_deref() == Some(owner))
        {
            self.st
                .toast("Finish or cancel this run’s export before cleanup".into());
            return;
        }
        let result = self
            .workspace_session
            .as_ref()
            .ok_or_else(|| err("Workspace is read-only"))
            .and_then(|session| session.remove_artifacts(&mut self.st, plan));
        match result {
            Ok(()) => {
                self.st.modal = AppModal::None;
                self.st.toast(format!("Moved {} files to trash. Runs → Trash restores files and ledger entries, including after restart.", plan.entry.files.len()));
                self.artifact_plan = None;
                self.external_motion = crate::external_motion::State::default();
            }
            Err(e) => self.st.toast(format!("Cleanup failed: {e}")),
        }
    }
    pub fn restore_trashed_artifacts(&mut self, id: &str) {
        let result = self
            .workspace_session
            .as_ref()
            .ok_or_else(|| err("Workspace is read-only"))
            .and_then(|session| session.restore_artifacts(&mut self.st, id));
        match result {
            Ok(()) => {
                self.st.modal = AppModal::None;
                self.st
                    .toast("Restored artifact files and ledger entries".into());
            }
            Err(e) => self.st.toast(format!("Restore failed: {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::Session;
    struct Fixture {
        root: PathBuf,
        st: Store,
    }
    fn unused_robot(f: &mut Fixture) -> Robot {
        let directory = f.root.join("robots/unused-robot");
        fs::create_dir_all(&directory).unwrap();
        let model = directory.join("robot.urdf");
        fs::write(&model, r#"<robot name="unused"><link name="base"/><link name="arm"/><joint name="hinge" type="revolute"><parent link="base"/><child link="arm"/><axis xyz="0 1 0"/><limit lower="-1" upper="1" effort="2" velocity="3"/></joint></robot>"#).unwrap();
        fs::write(directory.join("LICENSE"), b"retained license").unwrap();
        let mut robot = crate::robot_import::describe(&model, "unused-robot").unwrap();
        let mut task = crate::task_config::Config::generic(&robot).unwrap();
        task.actuators[0].kp = 9.;
        task.revision = 4;
        robot.task = Some(task);
        robot.use_task = true;
        f.st.robots.push(robot.clone());
        f.st.sel.robot = Some(robot.id.clone());
        robot
    }
    #[test]
    fn robot_files_and_calibration_restore_after_restart_and_can_be_purged() {
        let mut f = Fixture::new(1);
        let robot = unused_robot(&mut f);
        let root = f.root.join("robots");
        let session = Session::open(&f.root).unwrap();
        session.save(&f.st).unwrap();
        let original_runs = serde_json::to_value(&f.st.runs).unwrap();
        let plan = robot_plan(&f.st, &root, &robot.id).unwrap();
        assert_eq!(plan.entry.files.len(), 2);
        session.remove_artifacts(&mut f.st, &plan).unwrap();
        assert!(!Path::new(robot.urdf.as_ref().unwrap()).exists());
        assert!(!f.st.robots.iter().any(|r| r.id == robot.id));
        assert_eq!(serde_json::to_value(&f.st.runs).unwrap(), original_runs);
        drop(session);
        let session = Session::open(&f.root).unwrap();
        let mut reopened = Store::seed();
        crate::workspace::restore_at(&mut reopened, &f.root.join("studio-workspace.json"), false)
            .unwrap();
        session
            .restore_artifacts(&mut reopened, &plan.entry.id)
            .unwrap();
        let restored = reopened.robots.iter().find(|r| r.id == robot.id).unwrap();
        assert_eq!(
            serde_json::to_value(restored).unwrap(),
            serde_json::to_value(&robot).unwrap()
        );
        assert!(Path::new(restored.urdf.as_ref().unwrap()).is_file());
        let next = robot_plan(&reopened, &root, &robot.id).unwrap();
        session.remove_artifacts(&mut reopened, &next).unwrap();
        session
            .purge_artifacts(&mut reopened, &next.entry.id)
            .unwrap();
        assert!(!trash_dir(&root, &next.entry.id).unwrap().exists());
        assert!(reopened.trash.is_empty());
    }
    #[test]
    fn interrupted_robot_removal_rolls_back_and_restore_collisions_preserve_both_files() {
        let mut f = Fixture::new(0);
        let robot = unused_robot(&mut f);
        let root = f.root.join("robots");
        let session = Session::open(&f.root).unwrap();
        session.save(&f.st).unwrap();
        let plan = robot_plan(&f.st, &root, &robot.id).unwrap();
        prepare(&root, &plan.entry).unwrap();
        drop(session); // crash before workspace commit
        let session = Session::open(&f.root).unwrap();
        assert!(Path::new(robot.urdf.as_ref().unwrap()).is_file());
        session.remove_artifacts(&mut f.st, &plan).unwrap();
        fs::write(robot.urdf.as_ref().unwrap(), b"new file").unwrap();
        assert!(session
            .restore_artifacts(&mut f.st, &plan.entry.id)
            .is_err());
        assert_eq!(fs::read(robot.urdf.as_ref().unwrap()).unwrap(), b"new file");
        assert!(!f.st.robots.iter().any(|r| r.id == robot.id));
        assert_eq!(f.st.trash.len(), 1);
        fs::remove_file(robot.urdf.as_ref().unwrap()).unwrap();
        session
            .restore_artifacts(&mut f.st, &plan.entry.id)
            .unwrap();
        assert!(fs::read_to_string(robot.urdf.as_ref().unwrap())
            .unwrap()
            .contains("<robot"));
    }
    #[test]
    fn robot_removal_protects_drafts_shared_assets_and_recoverable_run_recipes() {
        let mut f = Fixture::new(1);
        let robot = unused_robot(&mut f);
        let root = f.root.join("robots");
        crate::task_editor::start(&mut f.st).unwrap();
        assert!(robot_plan(&f.st, &root, &robot.id).is_err());
        f.st.task_draft = None;
        let mut other = robot.clone();
        other.id = "other".into();
        f.st.robots.push(other);
        assert!(robot_plan(&f.st, &root, &robot.id)
            .unwrap_err()
            .to_string()
            .contains("also used"));
        f.st.robots.pop();
        let mut run = f.st.runs[0].clone();
        run.snapshot.robot_id = Some(robot.id.clone());
        run.snapshot.robot = Some(robot.name.clone());
        f.st.trash.push(new_entry(
            "Old run".into(),
            vec![],
            Removal::Run {
                run,
                index: 0,
                kept: vec![],
            },
        ));
        assert!(robot_plan(&f.st, &root, &robot.id)
            .unwrap_err()
            .to_string()
            .contains("recoverable run"));
        f.st.trash.clear();
        f.st.runs[0].snapshot.training_profile = Some(serde_json::json!({"model":robot.urdf}));
        assert!(robot_plan(&f.st, &root, &robot.id)
            .unwrap_err()
            .to_string()
            .contains("saved recipe"));
    }
    #[test]
    fn robot_removal_rechecks_new_references_and_preserves_external_files() {
        let mut f = Fixture::new(1);
        let robot = unused_robot(&mut f);
        let root = f.root.join("robots");
        let session = Session::open(&f.root).unwrap();
        let plan = robot_plan(&f.st, &root, &robot.id).unwrap();
        f.st.runs[0].snapshot.robot_id = Some(robot.id.clone());
        assert!(session.remove_artifacts(&mut f.st, &plan).is_err());
        assert!(Path::new(robot.urdf.as_ref().unwrap()).is_file());
        f.st.runs.clear();
        let outside = f.root.join("outside.urdf");
        fs::copy(robot.urdf.as_ref().unwrap(), &outside).unwrap();
        let r = f.st.robots.last_mut().unwrap();
        r.urdf = Some(outside.to_string_lossy().into());
        r.task = None;
        r.use_task = false;
        let external = robot_plan(&f.st, &root, &robot.id).unwrap();
        assert!(external.entry.files.is_empty());
        session.remove_artifacts(&mut f.st, &external).unwrap();
        assert!(outside.is_file());
    }
    impl Fixture {
        fn new(count: usize) -> Self {
            static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let serial = SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "nexus-artifact-{}-{}-{serial}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir_all(root.join("runs/run-test/training")).unwrap();
            let root = root.canonicalize().unwrap();
            let mut st = Store::seed();
            st.demo = false;
            let mut run = st.runs[0].clone();
            run.id = "run-test".into();
            run.state = RunState::Completed;
            run.snapshot.warm = None;
            run.snapshot.training_profile = None;
            run.ckpts.clear();
            for n in 0..count {
                let path = root.join(format!("runs/run-test/training/model_{n}.pt"));
                fs::write(&path, format!("trained weights {n}")).unwrap();
                fs::File::options()
                    .write(true)
                    .open(&path)
                    .unwrap()
                    .set_times(fs::FileTimes::new().set_modified(
                        std::time::UNIX_EPOCH + std::time::Duration::from_secs(1000 + n as u64),
                    ))
                    .unwrap();
                run.ckpts.push(Ckpt {
                    id: path.to_string_lossy().into(),
                    label: format!("model_{n}.pt"),
                    score: n as f64,
                    score_known: true,
                    st: CkSt::Ord,
                    pname: None,
                });
            }
            st.runs = vec![run];
            st.recordings.clear();
            st.sel.run = Some("run-test".into());
            Self { root, st }
        }
        fn runs(&self) -> PathBuf {
            self.root.join("runs")
        }
        fn reopen(&self) -> Store {
            let mut st = Store::seed();
            st.demo = false;
            crate::workspace::restore_at(&mut st, &self.root.join("studio-workspace.json"), false)
                .unwrap();
            st
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn discovers_periodic_outputs_and_only_assigns_observed_scores() {
        let mut f = Fixture::new(3);
        let dir = f.runs().join("run-test");
        fs::create_dir_all(dir.join("input")).unwrap();
        fs::write(dir.join("input/model_900.pt"), "input").unwrap();
        fs::write(dir.join("training/model_3.pt.tmp"), "partial").unwrap();
        fs::write(
            dir.join("metrics.jsonl"),
            "{\"event\":\"sample\",\"total_iteration\":2,\"reward\":7.5}\n{partial",
        )
        .unwrap();
        f.st.runs[0].ckpts.clear();
        assert_eq!(discover(&mut f.st.runs[0], &dir).unwrap(), 3);
        assert_eq!(discover(&mut f.st.runs[0], &dir).unwrap(), 0);
        let ck = &f.st.runs[0].ckpts;
        assert_eq!(ck.iter().filter(|c| c.score_known).count(), 1);
        assert_eq!(ck.iter().find(|c| c.score_known).unwrap().score, 7.5);
        assert!(serde_json::to_value(&f.st.runs[0]).is_ok());
    }
    #[test]
    fn imports_saved_run_once_without_claiming_its_worker_completed() {
        let mut f = Fixture::new(2);
        let original = f.st.runs.remove(0);
        fs::write(
            f.runs().join("run-test/run.json"),
            serde_json::to_vec(&original).unwrap(),
        )
        .unwrap();
        let root = f.runs();
        assert_eq!(discover_runs(&mut f.st, &root).unwrap(), 1);
        assert_eq!(discover_runs(&mut f.st, &root).unwrap(), 0);
        assert_eq!(f.st.runs[0].state, RunState::Stopped);
        assert_eq!(f.st.runs[0].ckpts.len(), 2);
        assert_eq!(f.st.runs[0].snapshot.robot, original.snapshot.robot);
        assert!(f.st.runs[0]
            .diagnosis
            .as_deref()
            .unwrap()
            .contains("final status is unavailable"));
    }
    #[test]
    fn gpu_periodic_filename_is_discovered_and_supported_by_format_checks() {
        let mut f = Fixture::new(0);
        let dir = f.runs().join("run-test");
        fs::write(
            dir.join("policy.safetensors.iter500"),
            "file listing fixture",
        )
        .unwrap();
        fs::write(dir.join("policy.safetensors.iter500.tmp"), "partial").unwrap();
        assert_eq!(discover(&mut f.st.runs[0], &dir).unwrap(), 1);
        assert!(is_safetensors(&f.st.runs[0].ckpts[0].id));
        assert!(!is_safetensors("policy.safetensors.iter500.tmp"));
        assert!(!f.st.runs[0].ckpts[0].score_known);
    }
    #[test]
    fn retention_protects_newest_best_promoted_and_recipe_references() {
        let mut f = Fixture::new(6);
        f.st.runs[0].ckpts[0].score = 1000.0;
        f.st.runs[0].ckpts[1].st = CkSt::Prom;
        let mut continued = f.st.runs[0].clone();
        continued.id = "continued".into();
        continued.ckpts.clear();
        continued.snapshot.warm = Some(f.st.runs[0].ckpts[2].id.clone());
        f.st.runs.push(continued);
        let plan = checkpoint_plan(&f.st, &f.runs(), "run-test", None, 1).unwrap();
        assert_eq!(plan.entry.files.len(), 2);
        assert!(plan.entry.files.iter().all(|p| ["model_3.pt", "model_4.pt"]
            .contains(&p.relative.file_name().unwrap().to_str().unwrap())));
        assert_eq!(plan.protected.len(), 4);
        f.st.runs[0].state = RunState::Paused;
        assert!(checkpoint_plan(&f.st, &f.runs(), "run-test", None, 1).is_err());
    }
    #[test]
    fn deletion_and_durable_restore_recover_exact_files_and_metadata() {
        let mut f = Fixture::new(3);
        let source = f.st.runs[0].ckpts[0].id.clone();
        let bytes = fs::read(&source).unwrap();
        let plan = checkpoint_plan(&f.st, &f.runs(), "run-test", Some(&source), 1).unwrap();
        let session = Session::open(&f.root).unwrap();
        session.remove_artifacts(&mut f.st, &plan).unwrap();
        assert!(!Path::new(&source).exists());
        assert_eq!(f.st.runs[0].ckpts.len(), 2);
        drop(session);
        let session = Session::open(&f.root).unwrap();
        let mut recovered = f.reopen();
        assert_eq!(recovered.trash.len(), 1);
        session
            .restore_artifacts(&mut recovered, &plan.entry.id)
            .unwrap();
        assert_eq!(fs::read(&source).unwrap(), bytes);
        assert_eq!(recovered.runs[0].ckpts.len(), 3);
        assert!(f.reopen().trash.is_empty());
        assert!(!trash_dir(&f.runs(), &plan.entry.id).unwrap().exists());
    }
    #[test]
    fn interruption_mid_delete_and_mid_restore_reconciles_to_document_commit() {
        let mut f = Fixture::new(4);
        let plan = checkpoint_plan(&f.st, &f.runs(), "run-test", None, 1).unwrap();
        assert_eq!(plan.entry.files.len(), 3);
        let session = Session::open(&f.root).unwrap();
        session.save(&f.st).unwrap();
        let dir = trash_dir(&f.runs(), &plan.entry.id).unwrap();
        fs::create_dir_all(&dir).unwrap();
        atomic_json(&dir.join("journal.json"), &plan.entry).unwrap();
        let first = &plan.entry.files[0].relative;
        let stored = dir.join("files").join(first);
        fs::create_dir_all(stored.parent().unwrap()).unwrap();
        fs::rename(f.runs().join(first), &stored).unwrap();
        drop(session); // simulate process loss before workspace commit
        let session = Session::open(&f.root).unwrap();
        assert!(plan
            .entry
            .files
            .iter()
            .all(|e| f.runs().join(&e.relative).is_file()));
        assert!(!dir.exists());
        session.remove_artifacts(&mut f.st, &plan).unwrap();
        // Interruption in a restore before the document removes its trash entry.
        fs::rename(&stored, f.runs().join(first)).unwrap();
        drop(session);
        let session = Session::open(&f.root).unwrap();
        assert!(stored.is_file());
        assert!(!f.runs().join(first).exists());
        let mut recovered = f.reopen();
        session
            .restore_artifacts(&mut recovered, &plan.entry.id)
            .unwrap();
        assert!(plan
            .entry
            .files
            .iter()
            .all(|e| f.runs().join(&e.relative).is_file()));
    }
    #[test]
    fn collision_never_overwrites_and_preflights_the_whole_restore() {
        let mut f = Fixture::new(4);
        let plan = checkpoint_plan(&f.st, &f.runs(), "run-test", None, 1).unwrap();
        let session = Session::open(&f.root).unwrap();
        session.remove_artifacts(&mut f.st, &plan).unwrap();
        let collision = f.runs().join(&plan.entry.files.last().unwrap().relative);
        fs::write(&collision, "new independent file").unwrap();
        assert!(session
            .restore_artifacts(&mut f.st, &plan.entry.id)
            .is_err());
        assert_eq!(
            fs::read_to_string(&collision).unwrap(),
            "new independent file"
        );
        assert!(!f.runs().join(&plan.entry.files[0].relative).exists());
        assert_eq!(f.st.trash.len(), 1);
        fs::remove_file(collision).unwrap();
        session
            .restore_artifacts(&mut f.st, &plan.entry.id)
            .unwrap();
    }
    #[test]
    fn changed_files_new_references_and_links_reject_cleanup_without_mutation() {
        let mut f = Fixture::new(2);
        let source = f.st.runs[0].ckpts[0].id.clone();
        let plan = checkpoint_plan(&f.st, &f.runs(), "run-test", Some(&source), 1).unwrap();
        let session = Session::open(&f.root).unwrap();
        f.st.runs[0].ckpts[0].st = CkSt::Prom;
        assert!(session.remove_artifacts(&mut f.st, &plan).is_err());
        assert!(
            Path::new(&source).is_file(),
            "source removed during reference rejection: {source}"
        );
        f.st.runs[0].ckpts[0].st = CkSt::Ord;
        fs::write(&source, "changed weights 0").unwrap();
        let result = session.remove_artifacts(&mut f.st, &plan);
        assert!(result.is_err(), "changed fingerprint accepted");
        assert!(f.st.trash.is_empty());
        assert!(Path::new(&source).is_file());
        let outside = f.root.join("outside.pt");
        fs::write(&outside, "external").unwrap();
        f.st.runs[0].ckpts[0].id = outside.to_string_lossy().into();
        assert!(checkpoint_plan(
            &f.st,
            &f.runs(),
            "run-test",
            Some(&outside.to_string_lossy()),
            1
        )
        .is_err());
        #[cfg(unix)]
        {
            let linked = f.runs().join("run-test/link.pt");
            std::os::unix::fs::symlink(&outside, &linked).unwrap();
            f.st.runs[0].ckpts[0].id = linked.to_string_lossy().into();
            assert!(checkpoint_plan(
                &f.st,
                &f.runs(),
                "run-test",
                Some(&linked.to_string_lossy()),
                1
            )
            .is_err());
        }
    }
    #[cfg(unix)]
    #[test]
    fn storage_root_alias_is_accepted_but_artifact_links_are_read_only() {
        let mut f = Fixture::new(2);
        let alias = f.root.join("runs-alias");
        std::os::unix::fs::symlink(f.runs(), &alias).unwrap();
        let source = alias.join("run-test/training/model_0.pt");
        f.st.runs[0].ckpts[0].id = source.to_string_lossy().into();
        let plan = checkpoint_plan(
            &f.st,
            &f.runs(),
            "run-test",
            Some(&source.to_string_lossy()),
            1,
        )
        .unwrap();
        let session = Session::open(&f.root).unwrap();
        session.remove_artifacts(&mut f.st, &plan).unwrap();
        assert!(!source.exists());
        session
            .restore_artifacts(&mut f.st, &plan.entry.id)
            .unwrap();
        assert_eq!(fs::read_to_string(source).unwrap(), "trained weights 0");
    }
    #[test]
    fn permanent_purge_releases_files_and_removes_restore_point() {
        let mut f = Fixture::new(4);
        let plan = checkpoint_plan(&f.st, &f.runs(), "run-test", None, 1).unwrap();
        let session = Session::open(&f.root).unwrap();
        session.remove_artifacts(&mut f.st, &plan).unwrap();
        session.purge_artifacts(&mut f.st, &plan.entry.id).unwrap();
        assert!(f.st.trash.is_empty());
        assert!(!trash_dir(&f.runs(), &plan.entry.id).unwrap().exists());
        assert!(plan
            .entry
            .files
            .iter()
            .all(|e| !f.runs().join(&e.relative).exists()));
    }
    #[test]
    fn purge_interruption_obeys_workspace_commit_and_finishes_remaining_files() {
        let mut f = Fixture::new(4);
        let plan = checkpoint_plan(&f.st, &f.runs(), "run-test", None, 1).unwrap();
        let session = Session::open(&f.root).unwrap();
        session.remove_artifacts(&mut f.st, &plan).unwrap();
        mark_purge(&f.runs(), &plan.entry, true).unwrap();
        drop(session); // purge intent without document commit keeps restore point
        let session = Session::open(&f.root).unwrap();
        let dir = trash_dir(&f.runs(), &plan.entry.id).unwrap();
        assert!(plan
            .entry
            .files
            .iter()
            .all(|e| dir.join("files").join(&e.relative).is_file()));
        mark_purge(&f.runs(), &plan.entry, true).unwrap();
        f.st.trash.clear();
        session.save(&f.st).unwrap();
        fs::remove_file(dir.join("files").join(&plan.entry.files[0].relative)).unwrap();
        drop(session); // partial unlink after commit must finish, not restore
        let _session = Session::open(&f.root).unwrap();
        assert!(!dir.exists());
        assert!(f.reopen().trash.is_empty());
    }
    #[test]
    fn run_removal_keeps_promoted_contract_and_recording_then_restores_without_duplicates() {
        let mut f = Fixture::new(3);
        f.st.runs[0].ckpts[0].st = CkSt::Prom;
        let original = f.st.runs[0].clone();
        let recording = f.runs().join("run-test/recording.json");
        fs::write(&recording, "saved motion").unwrap();
        f.st.recordings.push(Recording {
            id: "rec".into(),
            name: "motion".into(),
            robot: original.snapshot.robot.clone(),
            run: Some(original.id.clone()),
            scene: "flat".into(),
            frames: 2,
            resets: 0,
            dist: 0.0,
            path: Some(recording.to_string_lossy().into()),
        });
        let plan = run_plan(&f.st, &f.runs(), "run-test").unwrap();
        let session = Session::open(&f.root).unwrap();
        session.remove_artifacts(&mut f.st, &plan).unwrap();
        assert_eq!(f.st.runs.len(), 1);
        assert!(f.st.runs[0].coll);
        assert_eq!(f.st.runs[0].snapshot.robot, original.snapshot.robot);
        assert!(recording.is_file());
        assert!(Path::new(&original.ckpts[0].id).is_file());
        assert!(!Path::new(&original.ckpts[1].id).exists());
        session
            .restore_artifacts(&mut f.st, &plan.entry.id)
            .unwrap();
        assert_eq!(f.st.runs.len(), 1);
        assert_eq!(f.st.runs[0].ckpts.len(), 3);
        assert!(!f.st.runs[0].coll);
        assert_eq!(f.st.recordings.len(), 1);
    }
}
