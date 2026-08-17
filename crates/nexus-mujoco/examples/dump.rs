//! Load an MJCF and print what the bridge sees — the smoke test for the
//! whole FFI path, runnable without any UI: `cargo run -p nexus-mujoco
//! --example dump -- <model.xml>`.

fn main() {
    let path = std::env::args().nth(1).expect("usage: dump <model.xml>");
    if let Some(why) = nexus_mujoco::why_unavailable() {
        eprintln!("unavailable: {why}");
        std::process::exit(1);
    }
    let mut m = nexus_mujoco::Model::load(&path).expect("load");
    println!("nq={} nkey={} nmesh={}", m.nq(), m.nkey(), m.nmesh());
    if m.nkey() > 0 {
        m.reset_keyframe(0);
    }
    let scene = m.scene();
    println!("scene geoms: {}", scene.len());
    let mut counts = std::collections::BTreeMap::new();
    for g in &scene {
        *counts.entry(format!("{:?}", kind_name(&g.kind))).or_insert(0usize) += 1;
    }
    for (k, n) in counts {
        println!("  {k}: {n}");
    }
    for g in scene.iter().take(6) {
        println!("  e.g. {:?} pos={:?} rgba={:?}", g.kind, g.pos, g.rgba);
    }
    for id in 0..m.nmesh().min(3) {
        if let Some(mesh) = m.mesh(id) {
            println!("mesh {id}: {} corners", mesh.positions.len() / 3);
        }
    }
    if let Some(h) = scene.iter().find_map(|g| match g.kind {
        nexus_mujoco::GeomKind::Hfield { id } => Some(id),
        _ => None,
    }) {
        let hm = m.hfield(h as usize).expect("hfield");
        println!("hfield {h}: {} verts {} tris", hm.positions.len() / 3, hm.indices.len() / 3);
    }
}

fn kind_name(k: &nexus_mujoco::GeomKind) -> &'static str {
    use nexus_mujoco::GeomKind::*;
    match k {
        Plane { .. } => "plane",
        Hfield { .. } => "hfield",
        Sphere { .. } => "sphere",
        Capsule { .. } => "capsule",
        Ellipsoid { .. } => "ellipsoid",
        Cylinder { .. } => "cylinder",
        Box { .. } => "box",
        Mesh { .. } => "mesh",
        Other(_) => "other",
    }
}
