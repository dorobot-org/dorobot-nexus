//! Import a self-contained URDF asset with measured structural facts.
use crate::state::Robot;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

fn source_asset(base: &Path, reference: &str) -> Result<PathBuf, String> {
    crate::robot_assets::resolve(base, reference)
}
fn prepare(path: &Path) -> Result<(urdf_rs::Robot, BTreeMap<String, PathBuf>), String> {
    let model = urdf_rs::read_file(path).map_err(|e| format!("Invalid URDF: {e}"))?;
    let mut names = BTreeSet::new();
    for link in &model.links {
        if link.name.is_empty() || !names.insert(link.name.clone()) {
            return Err("Link names must be unique and nonempty".into());
        }
    }
    if names.is_empty() {
        return Err("Robot has no links".into());
    }
    let mut joint_names = BTreeSet::new();
    let mut parents = BTreeMap::new();
    for joint in &model.joints {
        if joint
            .origin
            .xyz
            .0
            .iter()
            .chain(joint.origin.rpy.0.iter())
            .any(|v| !v.is_finite())
        {
            return Err(format!("Joint {} has a nonfinite origin", joint.name));
        }
        if matches!(
            joint.joint_type,
            urdf_rs::JointType::Revolute
                | urdf_rs::JointType::Continuous
                | urdf_rs::JointType::Prismatic
        ) && (joint.axis.xyz.0.iter().any(|v| !v.is_finite())
            || joint.axis.xyz.0.iter().map(|v| v * v).sum::<f64>() <= 1e-16)
        {
            return Err(format!("Joint {} needs a finite, nonzero axis", joint.name));
        }
        if joint.name.is_empty()
            || !joint_names.insert(joint.name.clone())
            || !names.contains(&joint.parent.link)
            || !names.contains(&joint.child.link)
        {
            return Err(format!(
                "Invalid joint {}: duplicate name or missing link",
                joint.name
            ));
        }
        if parents
            .insert(joint.child.link.clone(), joint.parent.link.clone())
            .is_some()
        {
            return Err("A link has multiple parent joints".into());
        }
        if matches!(
            joint.joint_type,
            urdf_rs::JointType::Revolute | urdf_rs::JointType::Prismatic
        ) && (!joint.limit.lower.is_finite()
            || !joint.limit.upper.is_finite()
            || joint.limit.lower > joint.limit.upper)
        {
            return Err(format!("Invalid limits on joint {}", joint.name));
        }
    }
    if names.iter().filter(|n| !parents.contains_key(*n)).count() != 1 {
        return Err("URDF must have exactly one root link".into());
    }
    for name in &names {
        let mut visited = BTreeSet::new();
        let mut current = name;
        while let Some(parent) = parents.get(current) {
            if !visited.insert(current) {
                return Err("Joint graph contains a cycle".into());
            }
            current = parent;
        }
    }
    let mut assets = BTreeMap::new();
    let base = path.parent().unwrap_or(Path::new("."));
    let mut add = |reference: &str| -> Result<(), String> {
        assets.insert(reference.to_string(), source_asset(base, reference)?);
        Ok(())
    };
    for link in &model.links {
        for geometry in link
            .visual
            .iter()
            .map(|v| &v.geometry)
            .chain(link.collision.iter().map(|c| &c.geometry))
        {
            if let urdf_rs::Geometry::Mesh { filename, scale } = geometry {
                if scale
                    .as_ref()
                    .is_some_and(|s| s.0.iter().any(|v| !v.is_finite() || *v == 0.))
                {
                    return Err(format!("Mesh {filename} has an invalid scale"));
                }
                add(filename)?;
            }
        }
        for visual in &link.visual {
            if let Some(texture) = visual.material.as_ref().and_then(|m| m.texture.as_ref()) {
                add(&texture.filename)?;
            }
        }
    }
    for material in &model.materials {
        if let Some(texture) = &material.texture {
            add(&texture.filename)?;
        }
    }
    Ok((model, assets))
}
pub fn inspect(path: &Path) -> Result<String, String> {
    let (model, assets) = prepare(path)?;
    let mut graph = crate::robot_assets::Graph::default();
    for source in assets.values() {
        graph.capture(source)?;
    }
    let movable = model
        .joints
        .iter()
        .filter(|j| j.joint_type != urdf_rs::JointType::Fixed)
        .count();
    let no_collision = model
        .links
        .iter()
        .filter(|l| l.collision.is_empty())
        .count();
    Ok(format!("{} links · {} joints · {movable} movable joints · {} asset files, including materials/textures.\n{no_collision} links have no collision geometry.\nStructure and asset paths checked. Create a task or select a matching trainer to configure physics and learning.", model.links.len(), model.joints.len(), graph.len()))
}
pub fn asset_sources(path: &Path) -> Result<Vec<PathBuf>, String> {
    let (_, assets) = prepare(path)?;
    let mut graph = crate::robot_assets::Graph::default();
    for source in assets.values() {
        graph.capture(source)?;
    }
    Ok(graph.sources().cloned().collect())
}
pub fn describe(path: &Path, id: &str) -> Result<Robot, String> {
    let (model, _) = prepare(path)?;
    let files = asset_sources(path)?;
    Ok(Robot {
        id: id.into(),
        name: model.name,
        model_name: None,
        use_task: false,
        task: None,
        links: model.links.len() as u32,
        joints: model.joints.len() as u32,
        movable: model
            .joints
            .iter()
            .filter(|j| j.joint_type != urdf_rs::JointType::Fixed)
            .count() as u32,
        mapped: 0,
        meshes: files.len() as u32,
        used_by: vec![],
        urdf: Some(path.to_string_lossy().into()),
    })
}
pub fn import(path: &Path, id: &str, library: &Path) -> Result<Robot, String> {
    if !nexus_engine::ckpt::valid_id(id) {
        return Err("Invalid robot id".into());
    }
    let (mut model, assets) = prepare(path)?;
    let dest = library.join(id);
    if dest.exists() {
        return Err("Robot already exists in library".into());
    }
    fs::create_dir_all(library).map_err(|e| e.to_string())?;
    if library.is_symlink() {
        return Err("Robot library is linked; choose an owned workspace".into());
    }
    let stage = library.join(format!(".{id}.staging"));
    fs::create_dir(&stage).map_err(|e| e.to_string())?;
    let result = (|| -> Result<Robot, String> {
        let mut mapping = BTreeMap::new();
        let mut graph = crate::robot_assets::Graph::default();
        for (reference, source) in &assets {
            mapping.insert(
                reference.clone(),
                format!("meshes/{}", graph.capture(source)?),
            );
        }
        graph.write(&stage.join("meshes"))?;
        for parent in path
            .parent()
            .into_iter()
            .flat_map(|p| p.ancestors().take(4))
        {
            let mut found = false;
            for name in ["LICENSE", "LICENSE.md", "LICENSE.txt", "COPYING"] {
                if parent.join(name).is_file() {
                    fs::copy(parent.join(name), stage.join(name)).map_err(|e| e.to_string())?;
                    found = true;
                }
            }
            if found {
                break;
            }
        }
        for link in &mut model.links {
            for geometry in link
                .visual
                .iter_mut()
                .map(|v| &mut v.geometry)
                .chain(link.collision.iter_mut().map(|c| &mut c.geometry))
            {
                if let urdf_rs::Geometry::Mesh { filename, .. } = geometry {
                    *filename = mapping[filename].clone();
                }
            }
            for visual in &mut link.visual {
                if let Some(texture) = visual.material.as_mut().and_then(|m| m.texture.as_mut()) {
                    texture.filename = mapping[&texture.filename].clone();
                }
            }
        }
        for material in &mut model.materials {
            if let Some(texture) = &mut material.texture {
                texture.filename = mapping[&texture.filename].clone();
            }
        }
        fs::write(
            stage.join("robot.urdf"),
            urdf_rs::write_to_string(&model).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        fs::write(stage.join("validation.txt"), inspect(path)?).map_err(|e| e.to_string())?;
        fs::rename(&stage, &dest).map_err(|e| e.to_string())?;
        Ok(Robot {
            use_task: false,
            task: None,
            model_name: None,
            id: id.into(),
            name: model.name,
            links: model.links.len() as u32,
            joints: model.joints.len() as u32,
            movable: model
                .joints
                .iter()
                .filter(|j| j.joint_type != urdf_rs::JointType::Fixed)
                .count() as u32,
            mapped: 0,
            meshes: graph.len() as u32,
            used_by: Vec::new(),
            urdf: Some(dest.join("robot.urdf").to_string_lossy().to_string()),
        })
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(stage);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_prismatic_replay_translates_in_the_rotated_joint_frame() {
        use makepad_urdf_player::robot::{load_robot_from_string, ForwardKinematics};
        let xml = r#"<robot name="linear"><link name="base"/><link name="carriage"/><joint name="slide" type="prismatic"><parent link="base"/><child link="carriage"/><origin xyz="1 2 3" rpy="0 0 1.5707963267948966"/><axis xyz="1 0 0"/><limit lower="0" upper="0.5" effort="1" velocity="1"/></joint></robot>"#;
        let mut robot = load_robot_from_string(xml, ".").unwrap();
        assert!(robot.joints[0].is_movable());
        robot.joints[0].set_angle_unclamped(0.25);
        ForwardKinematics::update(&mut robot);
        let position = ForwardKinematics::get_link_position(&robot, "carriage")
            .unwrap()
            .to_array();
        for (a, b) in position.into_iter().zip([1., 2.25, 3.]) {
            assert!((a - b).abs() < 1e-6);
        }
        robot.joints[0].set_angle(2.);
        assert_eq!(robot.joints[0].angle, 0.5);
    }
    #[test]
    fn renderer_scales_each_axis_and_keeps_normals_and_winding_consistent() {
        use makepad_urdf_player::mesh::MeshData;
        let mut mesh = MeshData::from_obj_str("v 0 0 0\nv 1 0 1\nv 0 1 1\nf 1 2 3\n").unwrap();
        mesh.apply_scale_xyz([-2., 3., 0.5]).unwrap();
        assert_eq!(mesh.bounds_min, [-2., 0., 0.]);
        assert_eq!(mesh.bounds_max, [0., 3., 0.5]);
        assert_eq!(mesh.indices, vec![0, 2, 1]);
        let expected = [0.5f32, -1. / 3., 2.];
        let norm = expected.iter().map(|x| x * x).sum::<f32>().sqrt();
        for (actual, want) in mesh.vertices[4..7].iter().zip(expected) {
            assert!((*actual - want / norm).abs() < 1e-6);
        }
        let before = mesh.vertices.clone();
        assert!(mesh.apply_scale_xyz([1., 0., 1.]).is_err());
        assert_eq!(mesh.vertices, before);
    }
    #[test]
    fn native_urdf_loader_preserves_nonuniform_mesh_dimensions() {
        let root = std::env::temp_dir().join(format!("nexus-scale-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("shape.obj"),
            "v -0.5 -0.5 -0.5\nv 0.5 -0.5 -0.5\nv 0 0.5 0.5\nf 1 2 3\n",
        )
        .unwrap();
        let xml = r#"<robot name="scaled"><link name="base"><visual><origin xyz="0 0 0.1"/><geometry><mesh filename="shape.obj" scale="0.04 0.04 0.2"/></geometry></visual></link></robot>"#;
        let robot =
            makepad_urdf_player::robot::load_robot_from_string(xml, &root.to_string_lossy())
                .unwrap();
        let mesh = robot.links[0].mesh_data.as_ref().unwrap();
        for (extent, expected) in (0..3)
            .map(|i| mesh.bounds_max[i] - mesh.bounds_min[i])
            .zip([0.04, 0.04, 0.2])
        {
            assert!((extent - expected).abs() < 1e-6, "{extent} != {expected}");
        }
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn copies_assets_and_rejects_missing_meshes() {
        let root = std::env::temp_dir().join(format!("nexus-import-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("cube.stl"), "solid cube\nendsolid cube").unwrap();
        let xml = "<robot name=\"custom\"><link name=\"base\"><visual><geometry><mesh filename=\"cube.stl\"/></geometry></visual></link></robot>";
        let source = root.join("source.urdf");
        fs::write(&source, xml).unwrap();
        let robot = import(&source, "robot1", &root.join("library")).unwrap();
        assert_eq!(robot.links, 1);
        assert_eq!(robot.mapped, 0);
        fs::remove_file(root.join("cube.stl")).unwrap();
        assert!(inspect(&source).is_err());
        assert!(inspect(Path::new(robot.urdf.as_ref().unwrap())).is_ok());
        let _ = fs::remove_dir_all(root);
    }
}
