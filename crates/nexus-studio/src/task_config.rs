//! Versioned task authoring. Saved runs carry a value copy of this contract.
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, path::Path};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Ppo {
    pub num_envs: u32,
    pub iterations: u32,
    pub steps_per_env: u32,
    pub learning_rate: f64,
    pub epochs: u32,
    pub minibatches: u32,
    pub gamma: f64,
    pub gae_lambda: f64,
    pub entropy: f64,
    pub clip: f64,
    pub hidden: Vec<u32>,
    pub device: String,
    pub checkpoint_every: u32,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Simulation {
    pub timestep: f64,
    pub decimation: u32,
    pub episode_seconds: f64,
    /// Zero places the lowest collision geometry just above the floor.
    pub spawn_height: f64,
    pub fixed_base: bool,
    pub friction: f64,
    pub min_height: f64,
    pub max_tilt_degrees: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CommandRange {
    pub forward: [f64; 2],
    pub lateral: [f64; 2],
    pub yaw: [f64; 2],
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Term {
    pub name: String,
    pub weight: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Observation {
    pub name: String,
    pub enabled: bool,
    pub scale: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Actuator {
    pub joint: String,
    /// None holds this joint at its neutral position; other slots are a permutation.
    pub action_index: Option<usize>,
    pub direction: f64,
    pub scale: f64,
    pub offset: f64,
    pub kp: f64,
    pub kd: f64,
    pub effort: f64,
    pub velocity: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub options: std::collections::BTreeMap<String, String>,
    pub version: u32,
    pub revision: u32,
    pub name: String,
    pub kind: String,
    pub robot: String,
    pub model: String,
    pub ppo: Ppo,
    pub simulation: Simulation,
    pub commands: CommandRange,
    pub rewards: Vec<Term>,
    pub observations: Vec<Observation>,
    pub actuators: Vec<Actuator>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Draft {
    pub robot_id: String,
    pub config: Config,
    #[serde(default)]
    pub page: crate::task_editor::Page,
    #[serde(default)]
    pub form: Option<crate::task_editor::Form>,
}

pub const GENERIC_OBSERVATIONS: &[&str] = &[
    "angular_velocity",
    "gravity",
    "command",
    "joint_position",
    "joint_velocity",
    "previous_action",
    "linear_velocity",
];
pub const GENERIC_REWARDS: &[&str] = &[
    "track_linear_velocity",
    "track_angular_velocity",
    "upright",
    "alive",
    "joint_pose",
    "torque_l2",
    "action_rate_l2",
];

impl Config {
    pub fn specialized(robot: &crate::state::Robot) -> Result<Self, String> {
        let json = match robot.model_name() {
            "Open Duck Mini v2" => {
                include_str!("../../../tools/training/task-presets/open_duck.json")
            }
            "Microduck" => include_str!("../../../tools/training/task-presets/microduck.json"),
            _ => return Err("This robot has no editable specialized task yet".into()),
        };
        let mut cfg: Self = serde_json::from_str(json).map_err(|e| e.to_string())?;
        cfg.model = robot.urdf.clone().ok_or("Import the robot model first")?;
        cfg.ppo.device = if cfg!(target_os = "macos") {
            "mps"
        } else {
            "cpu"
        }
        .into();
        cfg.validate_model()?;
        Ok(cfg)
    }
    pub fn generic(robot: &crate::state::Robot) -> Result<Self, String> {
        let model_path = robot.urdf.as_ref().ok_or("Import a robot model first")?;
        let model = urdf_rs::read_file(model_path).map_err(|e| e.to_string())?;
        let mut actuators = vec![];
        for joint in &model.joints {
            if !matches!(
                joint.joint_type,
                urdf_rs::JointType::Revolute
                    | urdf_rs::JointType::Continuous
                    | urdf_rs::JointType::Prismatic
            ) {
                continue;
            }
            if joint.mimic.is_some() {
                return Err("Mimic joints require an explicit simulator constraint before creating this task".into());
            }
            actuators.push(Actuator {
                joint: joint.name.clone(),
                action_index: Some(actuators.len()),
                direction: 1.0,
                scale: 0.25,
                offset: 0.0,
                kp: if robot.model_name().contains("G1") {
                    60.0
                } else {
                    3.0
                },
                kd: if robot.model_name().contains("G1") {
                    2.0
                } else {
                    0.1
                },
                effort: if joint.limit.effort > 0.0 {
                    joint.limit.effort
                } else {
                    2.0
                },
                velocity: if joint.limit.velocity > 0.0 {
                    joint.limit.velocity
                } else {
                    5.0
                },
            });
        }
        let config = Self {
            options: Default::default(),
            version: 1,
            revision: 1,
            name: format!("{} · velocity task", robot.name),
            kind: "generic_mujoco".into(),
            robot: robot.model_name().into(),
            model: model_path.clone(),
            ppo: Ppo {
                num_envs: 32,
                iterations: 1000,
                steps_per_env: 24,
                learning_rate: 0.0003,
                epochs: 5,
                minibatches: 4,
                gamma: 0.99,
                gae_lambda: 0.95,
                entropy: 0.01,
                clip: 0.2,
                hidden: vec![256, 128],
                device: if cfg!(target_os = "macos") {
                    "mps"
                } else {
                    "cpu"
                }
                .into(),
                checkpoint_every: 100,
            },
            simulation: Simulation {
                timestep: 0.002,
                decimation: 10,
                episode_seconds: 10.0,
                spawn_height: 0.0,
                fixed_base: false,
                friction: 1.0,
                min_height: 0.03,
                max_tilt_degrees: 65.0,
            },
            commands: CommandRange {
                forward: [-0.2, 0.2],
                lateral: [-0.1, 0.1],
                yaw: [-0.5, 0.5],
            },
            rewards: GENERIC_REWARDS
                .iter()
                .zip([2.0, 1.0, 1.0, 0.2, -0.05, -0.001, -0.01])
                .map(|(n, w)| Term {
                    name: (*n).into(),
                    weight: w,
                })
                .collect(),
            observations: GENERIC_OBSERVATIONS
                .iter()
                .map(|n| Observation {
                    name: (*n).into(),
                    enabled: *n != "linear_velocity",
                    scale: if *n == "joint_velocity" { 0.05 } else { 1.0 },
                })
                .collect(),
            actuators,
        };
        config.validate()?;
        Ok(config)
    }
    pub fn actions(&self) -> usize {
        self.actuators
            .iter()
            .filter(|a| a.action_index.is_some())
            .count()
    }
    pub fn observation_size(&self) -> usize {
        self.observations
            .iter()
            .filter(|o| o.enabled)
            .map(|o| match o.name.as_str() {
                "command" if self.kind == "open_duck" => 7,
                "action_history" => 42,
                "targets" => 14,
                "contacts" | "phase" => 2,
                "head_command" => 4,
                "body_command" => 6,
                "joint_pos" | "joint_vel" | "actions" => 14,
                "joint_position" | "joint_velocity" => self.actuators.len(),
                "previous_action" => self.actions(),
                _ => 3,
            })
            .sum()
    }
    pub fn validate(&self) -> Result<(), String> {
        fn range(label: &str, v: f64, min: f64, max: f64) -> Result<(), String> {
            if !v.is_finite() || !(min..=max).contains(&v) {
                Err(format!("{label} must be between {min} and {max}"))
            } else {
                Ok(())
            }
        }
        if self.version != 1 {
            return Err("Unsupported task version".into());
        }
        if self.name.trim().is_empty() || self.name.len() > 128 || self.robot.trim().is_empty() {
            return Err(
                "Task and robot names must be nonempty; task names allow at most 128 bytes".into(),
            );
        }
        if !["generic_mujoco", "microduck", "open_duck"].contains(&self.kind.as_str()) {
            return Err("Unknown task backend".into());
        }
        if self.model.is_empty() {
            return Err("Task model is missing".into());
        }
        if self.kind != "generic_mujoco" {
            let expected = if self.kind == "open_duck" {
                "Open Duck Mini v2"
            } else {
                "Microduck"
            };
            if self.robot != expected
                || self.simulation.fixed_base
                || self.actuators.len() != 14
                || self.actions() != 14
            {
                return Err("The specialized duck task requires its floating base and all 14 servo channels".into());
            }
            let baseline: Self = serde_json::from_str(if self.kind == "open_duck" {
                include_str!("../../../tools/training/task-presets/open_duck.json")
            } else {
                include_str!("../../../tools/training/task-presets/microduck.json")
            })
            .map_err(|e| e.to_string())?;
            if self
                .actuators
                .iter()
                .map(|a| &a.joint)
                .ne(baseline.actuators.iter().map(|a| &a.joint))
                || self
                    .observations
                    .iter()
                    .map(|o| &o.name)
                    .collect::<BTreeSet<_>>()
                    != baseline.observations.iter().map(|o| &o.name).collect()
                || self
                    .rewards
                    .iter()
                    .map(|r| &r.name)
                    .collect::<BTreeSet<_>>()
                    != baseline.rewards.iter().map(|r| &r.name).collect()
            {
                return Err("Specialized task joints, observations and reward terms must match the original recipe".into());
            }
        }
        if self.kind == "microduck"
            && (self.actuators.iter().any(|a| a.kd != 0.)
                || !["upstream", "hold", "off"].contains(
                    &self
                        .options
                        .get("curriculum")
                        .map(String::as_str)
                        .unwrap_or("upstream"),
                ))
        {
            return Err("Microduck BAM uses firmware Kp and no separate PD Kd; curriculum must be upstream, hold or off".into());
        }
        let p = &self.ppo;
        range(
            "Environments",
            p.num_envs as f64,
            if self.kind == "open_duck" { 4.0 } else { 1.0 },
            65536.0,
        )?;
        range("Iterations", p.iterations as f64, 1.0, 1000000.0)?;
        range("Rollout steps", p.steps_per_env as f64, 4.0, 4096.0)?;
        range("Learning rate", p.learning_rate, 1e-7, 0.1)?;
        range("Epochs", p.epochs as f64, 1.0, 100.0)?;
        range(
            "Minibatches",
            p.minibatches as f64,
            1.0,
            (p.num_envs as u64 * p.steps_per_env as u64) as f64,
        )?;
        range("Discount", p.gamma, 0.0, 1.0)?;
        range("GAE lambda", p.gae_lambda, 0.0, 1.0)?;
        range("Entropy weight", p.entropy, 0.0, 1.0)?;
        range("PPO clip", p.clip, 0.001, 1.0)?;
        range(
            "Checkpoint interval",
            p.checkpoint_every as f64,
            1.0,
            1000000.0,
        )?;
        if p.hidden.is_empty()
            || p.hidden.len() > 8
            || p.hidden.iter().any(|n| !(4..=4096).contains(n))
        {
            return Err("Use 1–8 hidden layers, each with 4–4096 neurons".into());
        }
        if !["cpu", "mps", "cuda"].contains(&p.device.as_str()) {
            return Err("Learner device must be cpu, mps or cuda".into());
        }
        let s = &self.simulation;
        range("Physics timestep", s.timestep, 0.0001, 0.02)?;
        range("Control decimation", s.decimation as f64, 1.0, 100.0)?;
        range(
            "Control period",
            s.timestep * s.decimation as f64,
            0.001,
            0.2,
        )?;
        range("Episode seconds", s.episode_seconds, 0.1, 300.0)?;
        range("Spawn height", s.spawn_height, 0.0, 10.0)?;
        range("Floor friction", s.friction, 0.01, 5.0)?;
        range("Fall height", s.min_height, 0.0, 5.0)?;
        range("Fall tilt", s.max_tilt_degrees, 5.0, 175.0)?;
        for (name, bounds) in [
            ("Forward command", self.commands.forward),
            ("Lateral command", self.commands.lateral),
            ("Yaw command", self.commands.yaw),
        ] {
            range(name, bounds[0], -10.0, 10.0)?;
            range(name, bounds[1], bounds[0], 10.0)?;
        }
        let mut names = BTreeSet::new();
        if self.rewards.is_empty() {
            return Err("Add at least one reward term".into());
        }
        for term in &self.rewards {
            if !names.insert(&term.name)
                || (self.kind == "generic_mujoco" && !GENERIC_REWARDS.contains(&term.name.as_str()))
            {
                return Err(format!("Unknown or duplicate reward: {}", term.name));
            }
            range("Reward weight", term.weight, -10000.0, 10000.0)?;
        }
        names.clear();
        for obs in &self.observations {
            if !names.insert(&obs.name)
                || (self.kind == "generic_mujoco"
                    && !GENERIC_OBSERVATIONS.contains(&obs.name.as_str()))
            {
                return Err(format!("Unknown or duplicate observation: {}", obs.name));
            }
            range("Observation scale", obs.scale, 0.000001, 10000.0)?;
        }
        if self.observation_size() == 0 {
            return Err("Enable at least one observation".into());
        }
        if self.actuators.is_empty() || self.actuators.len() > 256 || self.actions() == 0 {
            return Err("Map 1–256 joints and enable at least one action".into());
        }
        let slots: BTreeSet<_> = self
            .actuators
            .iter()
            .filter_map(|a| a.action_index)
            .collect();
        if slots.len() != self.actions() || slots.into_iter().ne(0..self.actions()) {
            return Err("Action slots must be unique and contiguous from zero".into());
        }
        names.clear();
        for a in &self.actuators {
            if a.joint.is_empty() || !names.insert(&a.joint) {
                return Err("Actuator joint names must be unique and nonempty".into());
            }
            if ![-1.0, 1.0].contains(&a.direction) {
                return Err("Actuator direction must be +1 or -1".into());
            }
            range("Action scale", a.scale, 0.000001, 10.0)?;
            range("Joint offset", a.offset, -10.0, 10.0)?;
            range("Proportional gain", a.kp, 0.0, 10000.0)?;
            range("Derivative gain", a.kd, 0.0, 1000.0)?;
            range(
                "Effort limit",
                a.effort,
                if self.kind == "microduck" { 0. } else { 0.0001 },
                10000.0,
            )?;
            range(
                "Velocity limit",
                a.velocity,
                if self.kind == "microduck" { 0. } else { 0.0001 },
                1000.0,
            )?;
        }
        Ok(())
    }
    pub fn validate_model(&self) -> Result<(), String> {
        self.validate()?;
        let robot = urdf_rs::read_file(Path::new(&self.model)).map_err(|e| e.to_string())?;
        let joints: BTreeSet<_> = robot
            .joints
            .iter()
            .filter(|j| {
                matches!(
                    j.joint_type,
                    urdf_rs::JointType::Revolute
                        | urdf_rs::JointType::Continuous
                        | urdf_rs::JointType::Prismatic
                )
            })
            .map(|j| j.name.as_str())
            .collect();
        if self
            .actuators
            .iter()
            .any(|a| !joints.contains(a.joint.as_str()))
        {
            return Err("Actuator mapping references a joint outside this URDF".into());
        }
        Ok(())
    }
    /// Reorder slots as a swap so every intermediate editor state stays valid.
    pub fn set_action_slot(&mut self, joint: usize, slot: Option<usize>) -> Result<(), String> {
        let old = self
            .actuators
            .get(joint)
            .ok_or("Unknown joint")?
            .action_index;
        let count = self.actions();
        if old.is_some() && slot.is_none() && count == 1 {
            return Err("Keep at least one active action".into());
        }
        if slot.is_some_and(|i| i >= count + usize::from(old.is_none())) {
            return Err("Action slot is out of range".into());
        }
        if let Some(slot) = slot {
            if let Some(other) = self
                .actuators
                .iter()
                .position(|a| a.action_index == Some(slot))
            {
                self.actuators[other].action_index = old.or(Some(count));
            }
            self.actuators[joint].action_index = Some(slot);
        } else if let Some(old) = old {
            self.actuators[joint].action_index = None;
            for a in &mut self.actuators {
                if let Some(i) = a.action_index {
                    if i > old {
                        a.action_index = Some(i - 1);
                    }
                }
            }
        }
        Ok(())
    }
}
