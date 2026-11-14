//! Portable, digest-checked robot packages. Import creates a new library identity.
use crate::state::Robot;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path, PathBuf},
};

#[derive(Serialize, Deserialize)]
pub struct Manifest {
    pub version: u32,
    pub robot: Robot,
    pub files: BTreeMap<String, String>,
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn local(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let rel = Path::new(relative);
    if rel.as_os_str().is_empty() || !rel.components().all(|c| matches!(c, Component::Normal(_))) {
        return Err("Bundle contains a non-local path".into());
    }
    let mut path = root.to_path_buf();
    for component in rel.components() {
        path.push(component);
        if fs::symlink_metadata(&path)
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false)
        {
            return Err("Linked bundle assets are not portable".into());
        }
    }
    Ok(path)
}
fn files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut out = vec![];
    for entry in fs::read_dir(root).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let ty = entry.file_type().map_err(|e| e.to_string())?;
        if ty.is_symlink() {
            return Err("Linked bundle assets are not portable".into());
        }
        if ty.is_dir() {
            out.extend(files(&entry.path())?);
        } else if ty.is_file() {
            out.push(entry.path());
        } else {
            return Err("Bundle contains a non-file asset".into());
        }
    }
    Ok(out)
}
fn manifest_path(path: &Path) -> PathBuf {
    if path.is_dir() {
        path.join("bundle.json")
    } else {
        path.to_path_buf()
    }
}
pub fn read(path: &Path) -> Result<(PathBuf, Manifest), String> {
    let path = manifest_path(path)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let root = path
        .parent()
        .ok_or("Bundle needs a directory")?
        .to_path_buf();
    let manifest: Manifest = serde_json::from_slice(&fs::read(&path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    if manifest.version != 1 || path.file_name().is_none_or(|n| n != "bundle.json") {
        return Err("Unsupported robot bundle".into());
    }
    if manifest.robot.name.trim().is_empty()
        || manifest.robot.name.len() > 128
        || manifest.robot.name.chars().any(char::is_control)
    {
        return Err("Bundle robot has an invalid display name".into());
    }
    for (relative, digest) in &manifest.files {
        let bytes = fs::read(local(&root, relative)?).map_err(|e| e.to_string())?;
        if hash(&bytes) != *digest {
            return Err(format!("Bundle asset changed or is damaged: {relative}"));
        }
    }
    for file in files(&root)? {
        if file != path
            && !manifest.files.contains_key(
                &file
                    .strip_prefix(&root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
            )
        {
            return Err(format!("Bundle has an unlisted file: {}", file.display()));
        }
    }
    let model = manifest
        .robot
        .urdf
        .as_ref()
        .ok_or("Bundle has no robot model")?;
    if !manifest.files.contains_key(model) {
        return Err("Bundle model is not listed in the asset manifest".into());
    }
    let model_path = local(&root, model)?;
    crate::robot_import::inspect(&model_path)?;
    for source in crate::robot_import::asset_sources(&model_path)? {
        let relative = source
            .strip_prefix(&root)
            .map_err(|_| "Bundle references an external asset")?
            .to_string_lossy();
        if !manifest.files.contains_key(relative.as_ref()) {
            return Err(format!("Bundle references an unlisted asset: {relative}"));
        }
    }
    if let Some(task) = &manifest.robot.task {
        if task.model != *model || task.robot != manifest.robot.model_name() {
            return Err("Bundle task differs from its robot model contract".into());
        }
        let mut task = task.clone();
        task.model = model_path.to_string_lossy().into();
        task.validate_model()?;
    } else if manifest.robot.use_task {
        return Err("Bundle activates a missing task".into());
    }
    Ok((root, manifest))
}
pub fn inspect(path: &Path) -> Result<String, String> {
    let (_, manifest) = read(path)?;
    Ok(format!("{} · {} verified files. {}. Import creates a separate robot entry and preserves the model contract.", manifest.robot.name, manifest.files.len(), manifest.robot.task.as_ref().map(|c| format!("Saved {} task, revision {}", c.kind, c.revision)).unwrap_or("No saved task".into())))
}
pub fn export(robot: &Robot, destination: &Path) -> Result<(), String> {
    let source = Path::new(robot.urdf.as_ref().ok_or("Robot has no model")?);
    if let Some(task) = &robot.task {
        task.validate_model()?;
        if Path::new(&task.model)
            .canonicalize()
            .map_err(|e| e.to_string())?
            != source.canonicalize().map_err(|e| e.to_string())?
        {
            return Err("Task and robot point to different models".into());
        }
    }
    if !destination.is_absolute() {
        return Err("Choose an absolute export directory".into());
    }
    fs::create_dir_all(
        destination
            .parent()
            .ok_or("Choose a named export directory")?,
    )
    .map_err(|e| e.to_string())?;
    fs::create_dir(destination).map_err(|e| format!("Choose a new export directory: {e}"))?;
    let stage = destination.with_file_name(format!(
        ".robot-export-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let result = (|| {
        fs::create_dir(&stage).map_err(|e| e.to_string())?;
        crate::robot_import::import(source, "model", &stage)?;
        let mut copy = robot.clone();
        copy.used_by.clear();
        copy.urdf = Some("model/robot.urdf".into());
        if let Some(task) = &mut copy.task {
            task.model = "model/robot.urdf".into();
        }
        fs::write(stage.join("README.md"), "# Nexus robot bundle\n\nImport bundle.json through Robots → Import bundle. Geometry, materials, textures and saved task settings are included. Task-specific trainers and Python dependencies must be installed separately. This package contains no trained policy or hardware approval.\n").map_err(|e| e.to_string())?;
        let mut digests = BTreeMap::new();
        for file in files(&stage)? {
            digests.insert(
                file.strip_prefix(&stage)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
                hash(&fs::read(file).map_err(|e| e.to_string())?),
            );
        }
        let manifest = Manifest {
            version: 1,
            robot: copy,
            files: digests,
        };
        fs::write(
            stage.join("bundle.json"),
            serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        read(&stage)?;
        if !destination.is_dir()
            || destination.is_symlink()
            || fs::read_dir(destination)
                .map_err(|e| e.to_string())?
                .next()
                .is_some()
        {
            return Err("Export destination changed; existing files preserved".into());
        }
        fs::rename(&stage, destination).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&stage);
        let _ = fs::remove_dir(destination);
    }
    result
}
pub fn import(path: &Path, id: &str, library: &Path) -> Result<Robot, String> {
    if !nexus_engine::ckpt::valid_id(id) {
        return Err("Invalid robot identity".into());
    }
    let (source, manifest) = read(path)?;
    fs::create_dir_all(library).map_err(|e| e.to_string())?;
    if library.is_symlink() {
        return Err("Robot library is linked; choose an owned workspace".into());
    }
    let destination = library.join(id);
    if destination.exists() || destination.is_symlink() {
        return Err("Robot identity already has assets".into());
    }
    let stage = library.join(format!(".{id}.staging"));
    fs::create_dir(&stage).map_err(|e| e.to_string())?;
    let result = (|| {
        for (relative, digest) in &manifest.files {
            let bytes = fs::read(local(&source, relative)?).map_err(|e| e.to_string())?;
            if hash(&bytes) != *digest {
                return Err("Bundle changed during import".into());
            }
            let target = local(&stage, relative)?;
            fs::create_dir_all(target.parent().unwrap()).map_err(|e| e.to_string())?;
            fs::write(target, bytes).map_err(|e| e.to_string())?;
        }
        fs::write(
            stage.join("bundle.json"),
            serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        read(&stage)?;
        let mut robot = manifest.robot;
        robot.id = id.into();
        robot.used_by.clear();
        let relative = robot.urdf.as_ref().unwrap().clone();
        robot.urdf = Some(destination.join(&relative).to_string_lossy().into());
        if let Some(task) = &mut robot.task {
            task.model = robot.urdf.clone().unwrap();
        }
        let model_path = stage.join(relative);
        robot.meshes = crate::robot_import::asset_sources(&model_path)?.len() as u32;
        let model = urdf_rs::read_file(model_path).map_err(|e| e.to_string())?;
        robot.links = model.links.len() as u32;
        robot.joints = model.joints.len() as u32;
        robot.movable = model
            .joints
            .iter()
            .filter(|j| j.joint_type != urdf_rs::JointType::Fixed)
            .count() as u32;
        robot.mapped = robot.task.as_ref().map(|t| t.actions() as u32).unwrap_or(0);
        fs::rename(&stage, &destination).map_err(|e| e.to_string())?;
        Ok(robot)
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(stage);
    }
    result
}

pub fn recover(directory: &Path, id: &str) -> Result<Robot, String> {
    if directory.join("bundle.json").is_file() {
        let (root, manifest) = read(directory)?;
        let mut robot = manifest.robot;
        robot.id = id.into();
        robot.used_by.clear();
        robot.urdf = Some(
            root.join(robot.urdf.as_ref().unwrap())
                .to_string_lossy()
                .into(),
        );
        if let Some(task) = &mut robot.task {
            task.model = robot.urdf.clone().unwrap();
        }
        Ok(robot)
    } else {
        crate::robot_import::describe(&directory.join("robot.urdf"), id)
    }
}
fn distinct_name(st: &crate::state::Store, robot: &mut Robot) {
    let original = robot.name.clone();
    let mut n = 2;
    if st
        .robots
        .iter()
        .any(|r| r.name.eq_ignore_ascii_case(&robot.name))
    {
        robot.model_name = Some(robot.model_name().into());
        while st
            .robots
            .iter()
            .any(|r| r.name.eq_ignore_ascii_case(&robot.name))
        {
            let suffix = format!(" ({n})");
            let mut bytes = original.len().min(128usize.saturating_sub(suffix.len()));
            while !original.is_char_boundary(bytes) {
                bytes -= 1;
            }
            robot.name = format!("{}{suffix}", &original[..bytes]);
            n += 1;
        }
    }
}
impl crate::App {
    pub fn import_robot_path(&mut self, path: &str) {
        let Some(session) = &self.workspace_session else {
            self.st.toast("Workspace is read-only".into());
            return;
        };
        let library = PathBuf::from(crate::nexus::repo_dir()).join("robots");
        let id = loop {
            let id = self.st.nid("robot");
            if !library.join(&id).exists() && !self.st.robots.iter().any(|r| r.id == id) {
                break id;
            }
        };
        let path = Path::new(path);
        let result = if path.extension().is_some_and(|x| x == "urdf") {
            crate::robot_import::import(path, &id, &library)
        } else {
            import(path, &id, &library)
        };
        match result {
            Ok(mut robot) => {
                distinct_name(&self.st, &mut robot);
                let selected = self.st.sel.robot.clone();
                self.st.robots.push(robot);
                self.st.sel.robot = Some(id.clone());
                match session.save(&self.st) {
                    Ok(()) => {
                        self.st.modal = crate::state::AppModal::None;
                        self.st.toast(
                            "Robot assets and saved settings imported with a new library identity"
                                .into(),
                        );
                    }
                    Err(e) => {
                        self.st.robots.retain(|r| r.id != id);
                        self.st.sel.robot = selected;
                        self.st.toast(format!("Workspace save failed: {e}. Copied assets remain available through Discover assets."));
                    }
                }
            }
            Err(e) => self.st.toast(format!("Robot import failed: {e}")),
        }
    }
    pub fn discover_robot_assets(&mut self) {
        let Some(session) = &self.workspace_session else {
            self.st.toast("Workspace is read-only".into());
            return;
        };
        let library = PathBuf::from(crate::nexus::repo_dir()).join("robots");
        let before = self.st.robots.clone();
        let selected = self.st.sel.robot.clone();
        let result = (|| -> Result<(usize, Vec<String>), String> {
            if library.is_symlink() {
                return Err("Linked robot library is read-only".into());
            }
            let mut count = 0;
            let mut errors = vec![];
            for entry in fs::read_dir(&library).map_err(|e| e.to_string())? {
                let entry = entry.map_err(|e| e.to_string())?;
                let id = entry.file_name().to_string_lossy().into_owned();
                if !entry.file_type().map_err(|e| e.to_string())?.is_dir() || id.starts_with('.') || !nexus_engine::ckpt::valid_id(&id) || self.st.robots.iter().any(|r| r.id == id) || self.st.trash.iter().any(|e| matches!(&e.removal, crate::artifacts::Removal::Robot { robot, .. } if robot.id == id)) { continue; }
                if !entry.path().join("robot.urdf").is_file()
                    && !entry.path().join("bundle.json").is_file()
                {
                    continue;
                }
                match recover(&entry.path(), &id) {
                    Ok(mut robot) => {
                        distinct_name(&self.st, &mut robot);
                        self.st.sel.robot = Some(id);
                        self.st.robots.push(robot);
                        count += 1;
                    }
                    Err(e) => errors.push(format!("{id}: {e}")),
                }
            }
            session.save(&self.st).map_err(|e| e.to_string())?;
            Ok((count, errors))
        })();
        match result {
            Ok((count, errors)) => self.st.toast(format!("Recovered {count} robot entries. Plain URDF folders recover geometry; bundle folders also recover their exported task settings. {}", errors.join("; "))),
            Err(e) => { self.st.robots = before; self.st.sel.robot = selected; self.st.toast(format!("Robot discovery failed: {e}")); }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn temp() -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "nexus-bundle-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&p).unwrap();
        p.canonicalize().unwrap()
    }
    fn fixture(root: &Path) -> Robot {
        let source = root.join("source");
        fs::create_dir_all(source.join("textures")).unwrap();
        fs::write(
            source.join("textures/red paint.png"),
            b"actual texture bytes preserved by packaging",
        )
        .unwrap();
        fs::write(
            source.join("material.mtl"),
            "newmtl red\nmap_Kd -s 1 1 1 textures/red paint.png\n",
        )
        .unwrap();
        fs::write(
            source.join("body.obj"),
            "mtllib material.mtl\nv 0 0 0\nv 1 0 0\nv 0 1 0\nusemtl red\nf 1 2 3\n",
        )
        .unwrap();
        fs::write(source.join("LICENSE"), "Fixture asset license\n").unwrap();
        let urdf = source.join("robot.urdf");
        fs::write(&urdf, r#"<robot name="fixture"><link name="base"><visual><geometry><mesh filename="body.obj"/></geometry></visual></link><link name="arm"/><joint name="hinge" type="revolute"><parent link="base"/><child link="arm"/><axis xyz="0 1 0"/><limit lower="-1" upper="1" effort="2" velocity="3"/></joint></robot>"#).unwrap();
        let mut robot =
            crate::robot_import::import(&urdf, "original", &root.join("library")).unwrap();
        robot.model_name = Some(robot.name.clone());
        robot.name = "Calibrated robot".into();
        let mut task = crate::task_config::Config::generic(&robot).unwrap();
        task.actuators[0].kp = 7.;
        task.revision = 3;
        robot.task = Some(task);
        robot.use_task = true;
        robot
    }
    #[test]
    fn moved_bundle_restores_task_and_transitive_assets_without_original_sources() {
        let root = temp();
        let robot = fixture(&root);
        let exported = root.join("export");
        export(&robot, &exported).unwrap();
        let moved = root.join("moved");
        fs::rename(&exported, &moved).unwrap();
        fs::remove_dir_all(root.join("source")).unwrap();
        fs::remove_dir_all(root.join("library")).unwrap();
        let imported = import(&moved, "new-id", &root.join("destination")).unwrap();
        assert_eq!(imported.id, "new-id");
        assert_eq!(imported.name, "Calibrated robot");
        assert_eq!(imported.model_name(), "fixture");
        let task = imported.task.as_ref().unwrap();
        assert_eq!(task.revision, 3);
        assert_eq!(task.actuators[0].kp, 7.);
        assert!(imported.use_task);
        task.validate_model().unwrap();
        let assets =
            crate::robot_import::asset_sources(Path::new(imported.urdf.as_ref().unwrap())).unwrap();
        assert_eq!(assets.len(), 3);
        assert!(assets
            .iter()
            .all(|p| p.starts_with(root.join("destination/new-id"))));
        let texture = assets
            .iter()
            .find(|p| p.extension().is_some_and(|e| e == "png"))
            .unwrap();
        assert_eq!(
            fs::read(texture).unwrap(),
            b"actual texture bytes preserved by packaging"
        );
        assert!(root.join("destination/new-id/model/LICENSE").is_file());
        assert!(export(&imported, &moved).is_err());
        read(&moved).unwrap();
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn corrupt_unlisted_and_external_assets_fail_without_partial_import() {
        let root = temp();
        let robot = fixture(&root);
        let output = root.join("export");
        export(&robot, &output).unwrap();
        let (_, manifest) = read(&output).unwrap();
        let texture = manifest.files.keys().find(|p| p.ends_with(".png")).unwrap();
        let original = fs::read(output.join(texture)).unwrap();
        fs::write(output.join(texture), b"changed").unwrap();
        assert!(import(&output, "broken", &root.join("imports"))
            .unwrap_err()
            .contains("changed"));
        assert!(!root.join("imports/broken").exists());
        fs::write(output.join(texture), original).unwrap();
        fs::write(output.join("unlisted.txt"), b"keep").unwrap();
        assert!(read(&output).is_err());
        fs::remove_file(output.join("unlisted.txt")).unwrap();
        let model = output.join(manifest.robot.urdf.as_ref().unwrap());
        let mut urdf = urdf_rs::read_file(&model).unwrap();
        if let urdf_rs::Geometry::Mesh { filename, .. } = &mut urdf.links[0].visual[0].geometry {
            *filename = root.join("source/body.obj").to_string_lossy().into();
        }
        fs::write(&model, urdf_rs::write_to_string(&urdf).unwrap()).unwrap();
        let mut manifest = manifest;
        manifest.files.insert(
            manifest.robot.urdf.clone().unwrap(),
            hash(&fs::read(&model).unwrap()),
        );
        fs::write(
            output.join("bundle.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        assert!(read(&output).err().unwrap().contains("external"));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn linked_asset_is_rejected_even_when_contents_match_manifest() {
        let root = temp();
        let robot = fixture(&root);
        let output = root.join("export");
        export(&robot, &output).unwrap();
        let (_, manifest) = read(&output).unwrap();
        let texture = manifest.files.keys().find(|p| p.ends_with(".png")).unwrap();
        fs::remove_file(output.join(texture)).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(
            root.join("source/textures/red paint.png"),
            output.join(texture),
        )
        .unwrap();
        assert!(read(&output).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
