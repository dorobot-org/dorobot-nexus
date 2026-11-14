//! Persistent authoring state. Runtime evidence and deployment gates are never restored.
use crate::state::*;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

#[derive(Serialize, Deserialize)]
struct Document {
    version: u32,
    #[serde(default)]
    job_settings: crate::jobs::Settings,
    #[serde(default = "queue_default")]
    queue_enabled: bool,
    #[serde(default)]
    task_draft: Option<crate::task_config::Draft>,
    #[serde(default)]
    trash: Vec<crate::artifacts::TrashEntry>,
    #[serde(default = "retention_default")]
    retention_keep: usize,
    #[serde(default)]
    draft: Option<Scene>,
    #[serde(default)]
    selection: Option<Sel>,
    #[serde(default)]
    mode: Option<Mode>,
    #[serde(default)]
    training_profile: Option<String>,
    #[serde(default)]
    backend: Option<String>,
    scenes: Vec<Scene>,
    sets: Vec<SceneSet>,
    robots: Vec<Robot>,
    runs: Vec<Run>,
    recordings: Vec<Recording>,
    #[serde(default)]
    targets: Option<Vec<Target>>,
    #[serde(default)]
    theme: Option<Theme>,
    #[serde(default)]
    lang: Option<usize>,
}
fn queue_default() -> bool {
    true
}
fn retention_default() -> usize {
    4
}
fn path() -> PathBuf {
    PathBuf::from(crate::nexus::repo_dir()).join("studio-workspace.json")
}
/// An OS lock survives atomic document replacement and is released on crash.
/// https://doc.rust-lang.org/std/fs/struct.File.html#method.try_lock
pub struct Session {
    path: PathBuf,
    runs_root: PathBuf,
    robots_root: PathBuf,
    _lock: fs::File,
}
impl Session {
    pub fn open(root: &Path) -> io::Result<Self> {
        Self::open_with_runs(root, &root.join("runs"))
    }
    pub fn open_with_runs(root: &Path, runs_root: &Path) -> io::Result<Self> {
        fs::create_dir_all(root)?;
        let lock = fs::File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join(".studio-workspace.lock"))?;
        lock.try_lock().map_err(|e| {
            io::Error::other(format!(
                "Another Studio owns this workspace, or it cannot be locked: {e}"
            ))
        })?;
        fs::create_dir_all(runs_root)?;
        let runs_root = runs_root.canonicalize()?;
        let robots_root = root.canonicalize()?.join("robots");
        fs::create_dir_all(&robots_root)?;
        let committed = if root.join("studio-workspace.json").is_file() {
            let doc: Document =
                serde_json::from_slice(&fs::read(root.join("studio-workspace.json"))?)?;
            if !(1..=5).contains(&doc.version) || doc.scenes.is_empty() {
                return Err(io::Error::other(
                    "Unsupported workspace version or empty scene library; opened read-only",
                ));
            }
            doc.trash
                .iter()
                .map(|e| (e.id.clone(), e.storage))
                .collect::<Vec<_>>()
        } else {
            vec![]
        };
        crate::artifacts::recover(
            &runs_root,
            &committed
                .iter()
                .filter(|(_, scope)| *scope == crate::artifacts::Storage::Runs)
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>(),
        )?;
        if !fs::symlink_metadata(&robots_root)?.file_type().is_symlink() {
            crate::artifacts::recover(
                &robots_root,
                &committed
                    .iter()
                    .filter(|(_, scope)| *scope == crate::artifacts::Storage::Robots)
                    .map(|(id, _)| id.clone())
                    .collect::<Vec<_>>(),
            )?;
        }
        Ok(Self {
            path: root.join("studio-workspace.json"),
            runs_root,
            robots_root,
            _lock: lock,
        })
    }
    fn storage_root(&self, entry: &crate::artifacts::TrashEntry) -> io::Result<&Path> {
        use crate::artifacts::{Removal, Storage};
        match (&entry.storage, &entry.removal) {
            (Storage::Robots, Removal::Robot { .. })
                if !fs::symlink_metadata(&self.robots_root)?
                    .file_type()
                    .is_symlink() =>
            {
                Ok(&self.robots_root)
            }
            (Storage::Runs, Removal::Run { .. } | Removal::Checkpoints { .. }) => {
                Ok(&self.runs_root)
            }
            _ => Err(io::Error::other(
                "Invalid artifact storage or linked robot library; files preserved",
            )),
        }
    }
    pub fn remove_artifacts(
        &self,
        st: &mut Store,
        plan: &crate::artifacts::Plan,
    ) -> io::Result<()> {
        let root = self.storage_root(&plan.entry)?;
        crate::artifacts::validate_plan(st, root, plan)?;
        self.save(st)?;
        let before = (
            st.runs.clone(),
            st.robots.clone(),
            st.sel.clone(),
            st.trash.clone(),
        );
        crate::artifacts::prepare(root, &plan.entry)?;
        crate::artifacts::apply_removal(st, &plan.entry);
        if let Err(e) = self.save(st) {
            (st.runs, st.robots, st.sel, st.trash) = before;
            crate::artifacts::reconcile(root, &plan.entry, false)?;
            crate::artifacts::finish_restore(root, &plan.entry)?;
            return Err(e);
        }
        Ok(())
    }
    pub fn restore_artifacts(&self, st: &mut Store, id: &str) -> io::Result<()> {
        let entry = st
            .trash
            .iter()
            .find(|e| e.id == id)
            .cloned()
            .ok_or_else(|| io::Error::other("Trash entry no longer exists"))?;
        self.save(st)?;
        let root = self.storage_root(&entry)?;
        let before = (
            st.runs.clone(),
            st.robots.clone(),
            st.sel.clone(),
            st.trash.clone(),
        );
        crate::artifacts::apply_restore(st, &entry)?;
        let result = crate::artifacts::reconcile(root, &entry, false).and_then(|_| self.save(st));
        if let Err(e) = result {
            (st.runs, st.robots, st.sel, st.trash) = before;
            crate::artifacts::reconcile(root, &entry, true)?;
            return Err(e);
        }
        // A leftover journal is safe: startup finishes the already committed restore.
        let _ = crate::artifacts::finish_restore(root, &entry);
        Ok(())
    }
    pub fn purge_artifacts(&self, st: &mut Store, id: &str) -> io::Result<()> {
        let entry = st
            .trash
            .iter()
            .find(|e| e.id == id)
            .cloned()
            .ok_or_else(|| io::Error::other("Trash entry no longer exists"))?;
        self.save(st)?;
        let root = self.storage_root(&entry)?;
        crate::artifacts::check_purge(root, &entry, false)?;
        crate::artifacts::mark_purge(root, &entry, true)?;
        let before = st.trash.clone();
        st.trash.retain(|e| e.id != id);
        if let Err(e) = self.save(st) {
            st.trash = before;
            crate::artifacts::mark_purge(root, &entry, false)?;
            return Err(e);
        }
        crate::artifacts::finish_purge(root, &entry).map_err(|e| {
            io::Error::other(format!(
                "Purge committed; remaining files will be retried on restart: {e}"
            ))
        })
    }
    pub fn save(&self, st: &Store) -> io::Result<()> {
        if st.demo {
            return Ok(());
        }
        let doc = Document {
            version: 5,
            job_settings: st.job_settings.clone(),
            queue_enabled: st.queue_enabled,
            task_draft: st.task_draft.clone(),
            trash: st.trash.clone(),
            retention_keep: st.retention_keep,
            draft: st.dirty.then(|| st.draft.clone()).flatten(),
            selection: Some(st.sel.clone()),
            mode: Some(st.mode),
            training_profile: std::env::var("NEXUS_TRAINING_PROFILE").ok(),
            backend: std::env::var("NEXUS_BACKEND").ok(),
            scenes: st.scenes.clone(),
            sets: st.sets.clone(),
            robots: st.robots.clone(),
            runs: st.runs.clone(),
            recordings: st.recordings.clone(),
            targets: Some(st.targets.clone()),
            theme: Some(st.theme),
            lang: Some(st.lang),
        };
        let bytes = serde_json::to_vec_pretty(&doc)?;
        let p = &self.path;
        if let Ok(previous) = fs::read(p) {
            if previous == bytes {
                return Ok(());
            }
            let old: Document = serde_json::from_slice(&previous)?;
            if !(1..=5).contains(&old.version) || old.scenes.is_empty() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Workspace version is unsupported or document is empty; original preserved",
                ));
            }
            let backup = p.with_extension(format!("v{}.backup.json", old.version));
            if old.version < 5 && !backup.exists() {
                fs::write(backup, previous)?;
            }
        }
        let temp = p.with_extension("json.tmp");
        let mut file = fs::File::create(&temp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        fs::rename(&temp, p)?;
        Ok(())
    }
}
pub fn restore(st: &mut Store) -> io::Result<()> {
    restore_at(st, &path(), true)
}
pub(crate) fn restore_at(st: &mut Store, p: &Path, environment: bool) -> io::Result<()> {
    if !p.is_file() {
        return Ok(());
    }
    let doc: Document = serde_json::from_slice(&fs::read(p)?)?;
    if !(1..=5).contains(&doc.version) || doc.scenes.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unsupported or empty workspace",
        ));
    }
    if environment
        && std::env::var_os("NEXUS_TRAINING_PROFILE").is_none()
        && std::env::var_os("NEXUS_BACKEND").is_none()
    {
        if let Some(p) = doc.training_profile {
            std::env::set_var("NEXUS_TRAINING_PROFILE", p);
        }
    }
    if environment && std::env::var_os("NEXUS_BACKEND").is_none() {
        if let Some(b) = doc.backend {
            std::env::set_var("NEXUS_BACKEND", b);
        }
    }
    st.trash = doc.trash;
    st.retention_keep = doc.retention_keep.clamp(1, 1000);
    st.scenes = doc.scenes;
    st.sets = doc.sets;
    st.robots = doc.robots;
    st.runs = doc.runs;
    st.recordings = doc.recordings;
    if let Some(targets) = doc.targets {
        st.targets = targets;
    }
    if let Some(theme) = doc.theme {
        st.theme = theme;
    }
    if let Some(lang) = doc.lang {
        st.lang = lang.min(1);
    }
    st.job_settings = doc.job_settings;
    st.queue_enabled = doc.queue_enabled;
    st.dg.target = st.targets.first().map(|t| t.id.clone());
    for run in &mut st.runs {
        crate::nexus::load_run_metrics(run);
        if run.job.is_none() && matches!(run.state, RunState::Running | RunState::Paused) {
            run.state = RunState::Stopped;
            run.diagnosis = Some(
                "Studio closed before the run finished; saved checkpoints remain available.".into(),
            );
        }
    }
    st.sel.scene = st.scenes.first().map(|s| s.id.clone());
    st.sel.set = st.sets.first().map(|s| s.id.clone());
    st.sel.robot = st.robots.first().map(|s| s.id.clone());
    st.sel.run = st.runs.first().map(|s| s.id.clone());
    for run in &mut st.runs {
        if run.snapshot.robot_id.is_none() {
            run.snapshot.robot_id = st
                .robots
                .iter()
                .find(|r| Some(r.model_name()) == run.snapshot.robot.as_deref())
                .map(|r| r.id.clone());
        }
    }
    if let Some(sel) = doc.selection {
        if sel.scene.as_ref().is_some_and(|id| st.scene(id).is_some()) {
            st.sel.scene = sel.scene;
        }
        if sel.set.as_ref().is_some_and(|id| st.set(id).is_some()) {
            st.sel.set = sel.set;
        }
        if sel
            .robot
            .as_ref()
            .is_some_and(|id| st.robots.iter().any(|r| &r.id == id))
        {
            st.sel.robot = sel.robot;
        }
        if sel.run.as_ref().is_some_and(|id| st.run(id).is_some()) {
            st.sel.run = sel.run;
        }
        if sel.ck.as_ref().is_some_and(|id| {
            st.sel
                .run
                .as_ref()
                .and_then(|r| st.run(r))
                .is_some_and(|r| r.ckpts.iter().any(|c| &c.id == id))
        }) {
            st.sel.ck = sel.ck;
            st.sel.ck_run = sel.ck_run;
        }
    }
    if let Some(mode) = doc.mode {
        st.mode = mode;
    }
    if matches!(st.mode, Mode::Runs | Mode::Train | Mode::Validate) {
        if let Some(run) = st.sel.run.as_ref().and_then(|id| st.run(id)) {
            st.sel.robot = st
                .robots
                .iter()
                .find(|robot| {
                    run.snapshot.robot_id.as_deref() == Some(&robot.id)
                        || run.snapshot.robot.as_deref() == Some(robot.model_name())
                })
                .map(|robot| robot.id.clone());
        }
    }
    if let Some(draft) = doc
        .task_draft
        .filter(|d| st.robots.iter().any(|r| r.id == d.robot_id))
    {
        st.sel.robot = Some(draft.robot_id.clone());
        st.task_draft = Some(draft);
        st.modal = crate::state::AppModal::TaskEditor;
        st.mode = Mode::Robots;
        st.toast("Recovered unsaved RL task draft".into());
    }
    if let Some(draft) = doc.draft.filter(|d| st.scene(&d.id).is_some()) {
        st.sel.scene = Some(draft.id.clone());
        st.draft = Some(draft);
        st.dirty = true;
        // Recover the draft without taking the user away from their last
        // workspace. Older documents did not record a workspace choice.
        if doc.mode.is_none() {
            st.mode = Mode::Scenes;
        }
        st.toast("Recovered unsaved scene draft. Save or revert to resolve it.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn temp() -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "nexus-workspace-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&p).unwrap();
        p
    }
    #[test]
    fn draft_selection_and_last_workspace_recover_after_reopen() {
        let root = temp();
        let mut st = Store::seed();
        st.demo = false;
        st.sel.scene = Some(st.scenes[1].id.clone());
        st.edit_field("friction", 0.371);
        st.mode = Mode::Robots;
        st.theme = Theme::Light;
        st.lang = 1;
        st.robots[0].model_name = Some(st.robots[0].name.clone());
        st.robots[0].name = "Lab robot".into();
        let session = Session::open(&root).unwrap();
        session.save(&st).unwrap();
        drop(session);
        let mut reopened = Store::seed();
        restore_at(&mut reopened, &root.join("studio-workspace.json"), false).unwrap();
        assert!(reopened.dirty);
        assert_eq!(reopened.sel.scene, st.sel.scene);
        assert_eq!(reopened.draft.as_ref().unwrap().friction, 0.371);
        assert_eq!(reopened.theme, Theme::Light);
        assert_eq!(reopened.lang, 1);
        assert_eq!(reopened.robots[0].name, "Lab robot");
        assert_eq!(reopened.robots[0].model_name(), "Unitree G1-29dof");
        assert_eq!(reopened.mode, Mode::Robots);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn old_workspace_migrates_once_with_exact_backup() {
        let root = temp();
        let mut st = Store::seed();
        st.demo = false;
        let original = serde_json::to_vec(&serde_json::json!({"version":1,"scenes":st.scenes,"sets":st.sets,"robots":st.robots,"runs":[],"recordings":[]})).unwrap();
        fs::write(root.join("studio-workspace.json"), &original).unwrap();
        let session = Session::open(&root).unwrap();
        session.save(&st).unwrap();
        let now: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join("studio-workspace.json")).unwrap()).unwrap();
        assert_eq!(now["version"], 5);
        assert_eq!(
            fs::read(root.join("studio-workspace.v1.backup.json")).unwrap(),
            original
        );
        st.lang = 1;
        session.save(&st).unwrap();
        assert_eq!(
            fs::read(root.join("studio-workspace.v1.backup.json")).unwrap(),
            original
        );
        drop(session);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn task_form_recovers_unparsed_text_and_v2_migrates_with_exact_backup() {
        let root = temp();
        let mut st = Store::seed();
        st.demo = false;
        st.sel.robot = Some(st.robots[0].id.clone());
        let previous=serde_json::to_vec(&serde_json::json!({"version":2,"scenes":st.scenes,"sets":st.sets,"robots":st.robots,"runs":[],"recordings":[]})).unwrap();
        fs::write(root.join("studio-workspace.json"), &previous).unwrap();
        crate::task_editor::start(&mut st).unwrap();
        crate::task_editor::option(&mut st, 1).unwrap();
        crate::task_editor::option(&mut st, 0).unwrap();
        crate::task_editor::input(&mut st, 0, "unfinished input".into());
        let session = Session::open(&root).unwrap();
        session.save(&st).unwrap();
        drop(session);
        assert_eq!(
            fs::read(root.join("studio-workspace.v2.backup.json")).unwrap(),
            previous
        );
        let mut reopened = Store::seed();
        restore_at(&mut reopened, &root.join("studio-workspace.json"), false).unwrap();
        assert_eq!(reopened.modal, crate::state::AppModal::TaskEditor);
        let draft = reopened.task_draft.unwrap();
        assert_eq!(draft.form.unwrap().values[0], "unfinished input");
        assert_eq!(draft.config.ppo.num_envs, 32);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn only_one_writer_can_own_workspace_and_drop_releases_lock() {
        let root = temp();
        let first = Session::open(&root).unwrap();
        assert!(Session::open(&root).is_err());
        drop(first);
        let next = Session::open(&root).unwrap();
        drop(next);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn failed_atomic_save_preserves_previous_document_and_draft() {
        let root = temp();
        let mut st = Store::seed();
        st.demo = false;
        let session = Session::open(&root).unwrap();
        session.save(&st).unwrap();
        let original = fs::read(root.join("studio-workspace.json")).unwrap();
        st.edit_field("friction", 0.123);
        fs::create_dir(root.join("studio-workspace.json.tmp")).unwrap();
        assert!(session.save(&st).is_err());
        assert!(st.dirty);
        assert_eq!(
            fs::read(root.join("studio-workspace.json")).unwrap(),
            original
        );
        drop(session);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn target_settings_survive_serialization_but_network_evidence_does_not() {
        let mut target = Store::seed().targets[0].clone();
        target.ip = "192.0.2.10".into();
        target.ping = Ping::Ok(4);
        let restored: Target =
            serde_json::from_slice(&serde_json::to_vec(&target).unwrap()).unwrap();
        assert_eq!(restored.ip, "192.0.2.10");
        assert_eq!(restored.ping, Ping::Unprobed);
    }
    #[test]
    fn previous_workspace_schema_loads_without_new_optional_fields() {
        let st = Store::seed();
        let v = serde_json::json!({"version":1,"scenes":st.scenes,"sets":st.sets,"robots":st.robots,"runs":[],"recordings":[]});
        let doc: Document = serde_json::from_value(v).unwrap();
        assert!(doc.targets.is_none());
        assert!(doc.theme.is_none());
        assert!(doc.lang.is_none());
    }
}
