//! Scenes: the simulation configuration, as a named thing you can save.
//!
//! A scene is what the robot is asked to do and under what physics — terrain,
//! duration, commanded velocity, randomisation, seed. It exists as a first
//! class artifact for the reason the design's first law gives: a run that
//! cannot be reproduced is a run you cannot argue with. Training takes a *set*
//! of scenes and trains across that distribution; a recording carries the name
//! of the scene that produced it, so a trajectory is always traceable to the
//! world it happened in.
//!
//! Serialised by hand. The `zealot` feature deliberately adds no dependencies,
//! and a flat map of scalars does not justify pulling in a parser.

use std::path::{Path, PathBuf};

/// Where scenes and recordings live. Overridable so tests do not touch the
/// user's own library.
pub fn root() -> PathBuf {
    std::env::var("DOROBOT_SCENES_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("scenes"))
}

/// One simulation configuration.
///
/// The defaults are zealot's own: flat ground, no spawn randomisation, unit
/// gains — the baseline a result is compared against.
#[derive(Clone, Debug, PartialEq)]
pub struct Scene {
    pub name: String,
    /// Terrain family, or empty for flat ground.
    pub terrain: String,
    /// Rollout duration in seconds.
    pub seconds: f32,
    /// Commanded velocity: forward, lateral, yaw rate.
    pub vx: f32,
    pub vy: f32,
    pub yaw: f32,
    pub seed: u64,
    /// Ground friction (`BIPED_FRICTION`).
    pub friction: f32,
    /// PD gain scale (`BIPED_KP_SCALE`).
    pub kp_scale: f32,
    /// Link-mass randomisation (`BIPED_MASS_DR`).
    pub mass_dr: f32,
    /// Perturbation impulse (`BIPED_PUSH_VEL`); 0 disables pushes.
    pub push_vel: f32,
    /// Spawn-state randomisation (`BIPED_SPAWN_DR`).
    pub spawn_dr: bool,
    /// Terrain height multiplier (`BIPED_TERRAIN_AMP`). 1.0 is zealot's
    /// training terrain; higher makes a family harder without changing its
    /// character, which is how a family becomes a set of variants.
    pub terrain_amp: f32,
    /// Uphill grade along +X in degrees (`BIPED_TERRAIN_SLOPE_DEG`), 0–45.
    pub terrain_slope_deg: f32,
    /// Target stance height in metres (`BIPED_BASE_HEIGHT`).
    ///
    /// This is the reward's height target, not a constraint — lowering it is
    /// how a walking task becomes a crouching one. The G1's leg kinematics put
    /// the ceiling at 0.839 m (knees straight); below ~0.45 m the knee angle
    /// needed is beyond what the gait can hold while moving.
    pub base_height: f32,
}

impl Default for Scene {
    fn default() -> Self {
        Self {
            name: "baseline".into(),
            terrain: String::new(),
            seconds: 6.0,
            vx: 0.3,
            vy: 0.0,
            yaw: 0.0,
            seed: 0xC0FFEE,
            friction: 1.0,
            kp_scale: 1.0,
            mass_dr: 1.0,
            push_vel: 0.0,
            spawn_dr: false,
            terrain_amp: 1.0,
            terrain_slope_deg: 0.0,
            base_height: 0.82,
        }
    }
}

impl Scene {
    /// The `BIPED_*` environment this scene means.
    ///
    /// Every value is written explicitly rather than relying on zealot's
    /// defaults, so a scene fully determines the physics: two scenes that
    /// differ only in what they omit would otherwise be the same run.
    pub fn knobs(&self) -> Vec<(&'static str, String)> {
        let mut k: Vec<(&'static str, String)> = vec![
            ("BIPED_FRICTION", fmt(self.friction)),
            ("BIPED_KP_SCALE", fmt(self.kp_scale)),
            ("BIPED_MASS_DR", fmt(self.mass_dr)),
            ("BIPED_SPAWN_DR", if self.spawn_dr { "1" } else { "0" }.into()),
            ("BIPED_DRIVE_SEED", self.seed.to_string()),
            ("BIPED_BASE_HEIGHT", fmt(self.base_height)),
        ];
        if self.terrain.is_empty() {
            k.push(("BIPED_TERRAIN", "0".into()));
        } else {
            k.push(("BIPED_TERRAIN", "1".into()));
            k.push(("BIPED_TERRAIN_FAMILY", self.terrain.clone()));
            k.push(("BIPED_TERRAIN_AMP", fmt(self.terrain_amp)));
            k.push(("BIPED_TERRAIN_SLOPE_DEG", fmt(self.terrain_slope_deg)));
        }
        // A zero push velocity means "no pushes", which zealot expresses by the
        // interval rather than the magnitude — setting velocity 0 alone would
        // still schedule them.
        if self.push_vel > 0.0 {
            k.push(("BIPED_PUSH_VEL", fmt(self.push_vel)));
        } else {
            k.push(("BIPED_PUSH_VEL", "0".into()));
            // Far beyond any rollout length: pushes are scheduled by interval,
            // so a zero magnitude alone would still perturb the robot.
            k.push(("BIPED_PUSH_INTERVAL", "1000000".into()));
        }
        k
    }

    /// The terrain mesh this scene needs, as `terrain_export` names it.
    ///
    /// Keyed by family, amplitude, slope AND seed: a cache keyed only by
    /// family would draw amp=1 ground under an amp=2 simulation, which is the
    /// exact class of plausible-but-wrong display this product refuses.
    pub fn terrain_mesh_name(&self) -> Option<String> {
        if self.terrain.is_empty() {
            return None;
        }
        Some(format!(
            "terrain_{}_a{:.2}_s{:.1}_{:x}.stl",
            self.terrain, self.terrain_amp, self.terrain_slope_deg, self.seed
        ))
    }

    /// A one-line description for a list row.
    pub fn summary(&self) -> String {
        let terrain = if self.terrain.is_empty() { "flat" } else { &self.terrain };
        format!(
            "{terrain} · {:.0}s · vx {:.2} · fric {:.2} · kp {:.2}{}",
            self.seconds,
            self.vx,
            self.friction,
            self.kp_scale,
            if self.push_vel > 0.0 { " · pushed" } else { "" }
        ) + &if self.terrain.is_empty() {
            String::new()
        } else {
            format!(" · amp {:.1} · {:.0}°", self.terrain_amp, self.terrain_slope_deg)
        }
    }

    pub fn to_json(&self) -> String {
        format!(
            "{{\n  \"name\": {},\n  \"terrain\": {},\n  \"seconds\": {},\n  \
             \"vx\": {},\n  \"vy\": {},\n  \"yaw\": {},\n  \"seed\": {},\n  \
             \"friction\": {},\n  \"kp_scale\": {},\n  \"mass_dr\": {},\n  \
             \"push_vel\": {},\n  \"spawn_dr\": {},\n  \"terrain_amp\": {},\n  \"terrain_slope_deg\": {},\n  \"base_height\": {}\n}}\n",
            quote(&self.name),
            quote(&self.terrain),
            fmt(self.seconds),
            fmt(self.vx),
            fmt(self.vy),
            fmt(self.yaw),
            self.seed,
            fmt(self.friction),
            fmt(self.kp_scale),
            fmt(self.mass_dr),
            fmt(self.push_vel),
            self.spawn_dr,
            fmt(self.terrain_amp),
            fmt(self.terrain_slope_deg),
            fmt(self.base_height),
        )
    }

    /// Parse a scene. Missing fields keep their default, so a file written by
    /// an older build still loads rather than failing whole.
    pub fn from_json(text: &str) -> Self {
        let d = Scene::default();
        Scene {
            name: string_field(text, "name").unwrap_or(d.name),
            terrain: string_field(text, "terrain").unwrap_or(d.terrain),
            seconds: num_field(text, "seconds").unwrap_or(d.seconds),
            vx: num_field(text, "vx").unwrap_or(d.vx),
            vy: num_field(text, "vy").unwrap_or(d.vy),
            yaw: num_field(text, "yaw").unwrap_or(d.yaw),
            seed: num_field(text, "seed").map(|v| v as u64).unwrap_or(d.seed),
            friction: num_field(text, "friction").unwrap_or(d.friction),
            kp_scale: num_field(text, "kp_scale").unwrap_or(d.kp_scale),
            mass_dr: num_field(text, "mass_dr").unwrap_or(d.mass_dr),
            push_vel: num_field(text, "push_vel").unwrap_or(d.push_vel),
            spawn_dr: bool_field(text, "spawn_dr").unwrap_or(d.spawn_dr),
            terrain_amp: num_field(text, "terrain_amp").unwrap_or(d.terrain_amp),
            terrain_slope_deg: num_field(text, "terrain_slope_deg").unwrap_or(d.terrain_slope_deg),
            base_height: num_field(text, "base_height").unwrap_or(d.base_height),
        }
    }

    pub fn path(&self) -> PathBuf {
        root().join(format!("{}.scene.json", slug(&self.name)))
    }

    pub fn save(&self) -> std::io::Result<PathBuf> {
        let dir = root();
        std::fs::create_dir_all(&dir)?;
        let p = self.path();
        std::fs::write(&p, self.to_json())?;
        Ok(p)
    }

    pub fn load(path: &Path) -> std::io::Result<Self> {
        Ok(Scene::from_json(&std::fs::read_to_string(path)?))
    }
}

/// Every saved scene, newest name order, plus the built-in baseline when the
/// library is empty — an empty picker is a dead end.
pub fn list() -> Vec<Scene> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(root()) {
        for e in rd.flatten() {
            let p = e.path();
            if p.to_string_lossy().ends_with(".scene.json") {
                if let Ok(s) = Scene::load(&p) {
                    out.push(s);
                }
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    if out.is_empty() {
        out.push(Scene::default());
    }
    out
}

/// A rollout that was recorded, and the scene it came from.
#[derive(Clone, Debug, PartialEq)]
pub struct Recording {
    pub name: String,
    /// Name of the scene that produced it.
    pub scene: String,
    pub frames: usize,
    pub resets: usize,
    /// Ground distance covered, metres.
    pub distance: f32,
    /// The rollout JSON, as `biped_drive` wrote it.
    pub rollout: PathBuf,
}

impl Recording {
    pub fn dir() -> PathBuf {
        root().join("recordings")
    }

    pub fn summary(&self) -> String {
        format!(
            "{} · {} frames · {} resets · {:.2} m",
            self.scene, self.frames, self.resets, self.distance
        )
    }

    pub fn to_json(&self) -> String {
        format!(
            "{{\n  \"name\": {},\n  \"scene\": {},\n  \"frames\": {},\n  \
             \"resets\": {},\n  \"distance\": {},\n  \"rollout\": {}\n}}\n",
            quote(&self.name),
            quote(&self.scene),
            self.frames,
            self.resets,
            fmt(self.distance),
            quote(&self.rollout.to_string_lossy()),
        )
    }

    pub fn from_json(text: &str) -> Option<Self> {
        Some(Recording {
            name: string_field(text, "name")?,
            scene: string_field(text, "scene").unwrap_or_default(),
            frames: num_field(text, "frames").unwrap_or(0.0) as usize,
            resets: num_field(text, "resets").unwrap_or(0.0) as usize,
            distance: num_field(text, "distance").unwrap_or(0.0),
            rollout: PathBuf::from(string_field(text, "rollout")?),
        })
    }

    pub fn save(&self) -> std::io::Result<PathBuf> {
        let dir = Self::dir();
        std::fs::create_dir_all(&dir)?;
        let p = dir.join(format!("{}.rec.json", slug(&self.name)));
        std::fs::write(&p, self.to_json())?;
        Ok(p)
    }

    pub fn list() -> Vec<Recording> {
        let mut out = Vec::new();
        if let Ok(rd) = std::fs::read_dir(Self::dir()) {
            for e in rd.flatten() {
                let p = e.path();
                if p.to_string_lossy().ends_with(".rec.json") {
                    if let Some(r) = std::fs::read_to_string(&p)
                        .ok()
                        .and_then(|t| Recording::from_json(&t))
                    {
                        out.push(r);
                    }
                }
            }
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }
}

// ---- small helpers -------------------------------------------------------

/// Trailing-zero-free float, so a round number reads as one.
fn fmt(v: f32) -> String {
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-" { "0".into() } else { s.into() }
}

fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Filesystem-safe name, so a scene called "rough / 2 m·s" cannot escape the
/// library directory or collide with a sibling.
pub fn slug(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect();
    let s = s.trim_matches('-').to_string();
    let mut out = String::with_capacity(s.len());
    let mut prev_dash = false;
    for c in s.chars() {
        if c == '-' {
            if !prev_dash {
                out.push(c);
            }
            prev_dash = true;
        } else {
            out.push(c);
            prev_dash = false;
        }
    }
    if out.is_empty() { "scene".into() } else { out }
}

fn field_start(text: &str, key: &str) -> Option<usize> {
    let pat = format!("\"{key}\"");
    let k = text.find(&pat)? + pat.len();
    let colon = text[k..].find(':')? + k + 1;
    Some(colon)
}

fn num_field(text: &str, key: &str) -> Option<f32> {
    let rest = text[field_start(text, key)?..].trim_start();
    let end = rest
        .find(|c: char| !(c.is_ascii_digit() || matches!(c, '-' | '+' | '.' | 'e' | 'E')))
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

fn string_field(text: &str, key: &str) -> Option<String> {
    let rest = text[field_start(text, key)?..].trim_start();
    let rest = rest.strip_prefix('"')?;
    let mut out = String::new();
    let mut esc = false;
    for c in rest.chars() {
        match (esc, c) {
            (true, c) => {
                out.push(c);
                esc = false;
            }
            (false, '\\') => esc = true,
            (false, '"') => return Some(out),
            (false, c) => out.push(c),
        }
    }
    None
}

fn bool_field(text: &str, key: &str) -> Option<bool> {
    let rest = text[field_start(text, key)?..].trim_start();
    if rest.starts_with("true") {
        Some(true)
    } else if rest.starts_with("false") {
        Some(false)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scene_round_trips_through_json() {
        let s = Scene {
            name: "rough push".into(),
            terrain: "rough".into(),
            seconds: 8.0,
            vx: 0.45,
            vy: -0.1,
            yaw: 0.25,
            seed: 12345,
            friction: 0.6,
            kp_scale: 1.25,
            mass_dr: 1.3,
            push_vel: 0.8,
            spawn_dr: true,
            terrain_amp: 1.8,
            terrain_slope_deg: 12.0,
            base_height: 0.61,
        };
        assert_eq!(Scene::from_json(&s.to_json()), s);
    }

    #[test]
    fn a_missing_field_keeps_its_default_rather_than_failing() {
        // A file from an older build, without the newer knobs.
        let s = Scene::from_json(r#"{ "name": "old", "terrain": "wave" }"#);
        assert_eq!(s.name, "old");
        assert_eq!(s.terrain, "wave");
        assert_eq!(s.kp_scale, Scene::default().kp_scale);
    }

    #[test]
    fn knobs_fully_determine_the_physics() {
        let k = Scene::default().knobs();
        let keys: Vec<&str> = k.iter().map(|(a, _)| *a).collect();
        // Every physics knob is written, not left to zealot's default: two
        // scenes must not be able to differ only in what they omit.
        for want in ["BIPED_FRICTION", "BIPED_KP_SCALE", "BIPED_MASS_DR", "BIPED_SPAWN_DR"] {
            assert!(keys.contains(&want), "missing {want}");
        }
        // Flat ground turns terrain off rather than leaving it unset.
        assert!(k.contains(&("BIPED_TERRAIN", "0".to_string())));
    }

    #[test]
    fn a_terrain_scene_names_its_family() {
        let s = Scene { terrain: "step".into(), ..Scene::default() };
        let k = s.knobs();
        assert!(k.contains(&("BIPED_TERRAIN", "1".to_string())));
        assert!(k.contains(&("BIPED_TERRAIN_FAMILY", "step".to_string())));
    }

    #[test]
    fn a_terrain_variant_gets_its_own_mesh_identity() {
        let a = Scene { terrain: "rough".into(), ..Scene::default() };
        let b = Scene { terrain_amp: 2.5, ..a.clone() };
        let c = Scene { terrain_slope_deg: 10.0, ..a.clone() };
        let d = Scene { seed: 99, ..a.clone() };
        // Amplitude, slope and seed all change the geometry, so all must
        // change the filename — otherwise the renderer serves one variant's
        // ground under another's simulation.
        let names: Vec<String> = [&a, &b, &c, &d]
            .iter()
            .map(|s| s.terrain_mesh_name().expect("named"))
            .collect();
        let mut uniq = names.clone();
        uniq.sort();
        uniq.dedup();
        assert_eq!(uniq.len(), 4, "variants collided: {names:?}");
        assert!(names[0].starts_with("terrain_rough_a1.00_s0.0_"));
    }

    #[test]
    fn flat_ground_needs_no_mesh() {
        assert!(Scene::default().terrain_mesh_name().is_none());
    }

    #[test]
    fn slugs_cannot_escape_the_library() {
        assert_eq!(slug("rough / 2 m·s"), "rough-2-m-s");
        assert_eq!(slug("../../etc/passwd"), "etc-passwd");
        assert_eq!(slug(""), "scene");
    }

    #[test]
    fn a_recording_round_trips_and_names_its_scene() {
        let r = Recording {
            name: "run-7".into(),
            scene: "rough push".into(),
            frames: 301,
            resets: 4,
            distance: 3.75,
            rollout: PathBuf::from("/tmp/x.json"),
        };
        let back = Recording::from_json(&r.to_json()).expect("parse");
        assert_eq!(back, r);
    }
}
