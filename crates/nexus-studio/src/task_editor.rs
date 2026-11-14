//! Native task editor, with recoverable text and validated value commits.
use crate::{
    state::*,
    task_config::{Config, Draft},
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Field {
    Curriculum,
    Name,
    Budget,
    Update,
    Objective,
    Network,
    Device,
    Timing,
    World,
    Termination,
    Commands,
    Reward(usize),
    Observation(usize),
    Actuator(usize, u8),
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum Page {
    #[default]
    Home,
    Ppo,
    Simulation,
    Rewards(usize),
    Observations(usize),
    Actuators(usize),
    Fields(Field),
    Discard,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Form {
    pub field: Field,
    pub values: Vec<String>,
}
pub struct View {
    pub title: String,
    pub info: String,
    pub inputs: Vec<(String, String)>,
    pub options: Vec<(String, bool)>,
    pub ok: Option<String>,
    pub danger: Option<String>,
    pub back: bool,
}

fn choices(draft: &Draft) -> Vec<(String, Page)> {
    use Field::*;
    let cfg = &draft.config;
    match &draft.page {
        Page::Home => {
            let mut items = vec![
                ("Task name", Page::Fields(Name)),
                ("PPO learner", Page::Ppo),
                ("Simulation", Page::Simulation),
                ("Velocity commands", Page::Fields(Commands)),
                ("Reward weights", Page::Rewards(0)),
                ("Observations", Page::Observations(0)),
                ("Actuator mapping and limits", Page::Actuators(0)),
            ]
            .into_iter()
            .map(|(s, p)| (s.into(), p))
            .collect::<Vec<_>>();
            if cfg.kind == "microduck" {
                items.push(("Curriculum schedule".into(), Page::Fields(Curriculum)));
            }
            items
        }
        Page::Ppo => vec![
            ("Budget: environments, iterations, rollout steps", Budget),
            ("Updates: learning rate, epochs, minibatches", Update),
            ("Objective: discount, GAE, entropy", Objective),
            ("Network, clipping and checkpoint interval", Network),
            ("Learner device: Mac GPU / CPU / CUDA", Device),
        ]
        .into_iter()
        .map(|(s, f)| (s.into(), Page::Fields(f)))
        .collect(),
        Page::Simulation => vec![
            (
                "Physics timestep, control decimation, episode duration",
                Timing,
            ),
            ("Base mounting, spawn height, floor friction", World),
            ("Fall height and tilt", Termination),
        ]
        .into_iter()
        .map(|(s, f)| (s.into(), Page::Fields(f)))
        .collect(),
        Page::Rewards(page) => paged(
            *page,
            cfg.rewards.len(),
            |i| {
                (
                    format!("{}  = {}", cfg.rewards[i].name, cfg.rewards[i].weight),
                    Page::Fields(Reward(i)),
                )
            },
            Page::Rewards,
        ),
        Page::Observations(page) => paged(
            *page,
            cfg.observations.len(),
            |i| {
                let o = &cfg.observations[i];
                (
                    format!(
                        "{} · {} · scale {}",
                        o.name,
                        if o.enabled { "on" } else { "off" },
                        o.scale
                    ),
                    Page::Fields(Observation(i)),
                )
            },
            Page::Observations,
        ),
        Page::Actuators(page) => paged(
            *page,
            cfg.actuators.len(),
            |i| {
                let a = &cfg.actuators[i];
                (
                    format!(
                        "{} · action {}",
                        a.joint,
                        a.action_index
                            .map(|n| n.to_string())
                            .unwrap_or("hold".into())
                    ),
                    Page::Fields(Actuator(i, 0)),
                )
            },
            Page::Actuators,
        ),
        _ => vec![],
    }
}
fn paged(
    page: usize,
    count: usize,
    item: impl Fn(usize) -> (String, Page),
    make: impl Fn(usize) -> Page,
) -> Vec<(String, Page)> {
    let mut rows = (page * 6..((page + 1) * 6).min(count))
        .map(item)
        .collect::<Vec<_>>();
    if page > 0 {
        rows.push(("Previous page".into(), make(page - 1)));
    }
    if (page + 1) * 6 < count {
        rows.push(("Next page".into(), make(page + 1)));
    }
    rows
}
fn fields(cfg: &Config, field: &Field) -> Result<Vec<(String, String)>, String> {
    use Field::*;
    let p = &cfg.ppo;
    let s = &cfg.simulation;
    let values: Vec<(&str, String)> = match field {
        Curriculum => vec![(
            "Schedule: upstream, hold, or off",
            cfg.options
                .get("curriculum")
                .cloned()
                .unwrap_or("upstream".into()),
        )],
        Name => vec![("Task name", cfg.name.clone())],
        Budget => vec![
            ("Environments", p.num_envs.to_string()),
            ("Learning iterations", p.iterations.to_string()),
            ("Rollout steps per environment", p.steps_per_env.to_string()),
        ],
        Update => vec![
            ("Learning rate", p.learning_rate.to_string()),
            ("Epochs per update", p.epochs.to_string()),
            ("Minibatches", p.minibatches.to_string()),
        ],
        Objective => vec![
            ("Discount gamma", p.gamma.to_string()),
            ("GAE lambda", p.gae_lambda.to_string()),
            ("Entropy weight", p.entropy.to_string()),
        ],
        Network => vec![
            ("PPO clip", p.clip.to_string()),
            (
                "Hidden neurons per layer (comma separated)",
                p.hidden
                    .iter()
                    .map(|n| n.to_string())
                    .collect::<Vec<_>>()
                    .join(","),
            ),
            (
                "Checkpoint every N iterations",
                p.checkpoint_every.to_string(),
            ),
        ],
        Device => vec![("Learner device: mps, cpu or cuda", p.device.clone())],
        Timing => vec![
            ("Physics timestep (seconds)", s.timestep.to_string()),
            ("Physics steps per action", s.decimation.to_string()),
            ("Episode duration (seconds)", s.episode_seconds.to_string()),
        ],
        World if cfg.kind != "generic_mujoco" => vec![
            (
                "Root spawn height (m; 0 = original)",
                s.spawn_height.to_string(),
            ),
            ("Contact friction multiplier", s.friction.to_string()),
        ],
        World => vec![
            ("Fixed base: true or false", s.fixed_base.to_string()),
            (
                "Root spawn height (m; 0 = automatic)",
                s.spawn_height.to_string(),
            ),
            ("Contact friction", s.friction.to_string()),
        ],
        Termination => vec![
            ("Fall root height (m)", s.min_height.to_string()),
            ("Fall tilt (degrees)", s.max_tilt_degrees.to_string()),
        ],
        Commands => vec![
            (
                "Forward range (min,max m/s)",
                format!("{},{}", cfg.commands.forward[0], cfg.commands.forward[1]),
            ),
            (
                "Lateral range (min,max m/s)",
                format!("{},{}", cfg.commands.lateral[0], cfg.commands.lateral[1]),
            ),
            (
                "Yaw range (min,max rad/s)",
                format!("{},{}", cfg.commands.yaw[0], cfg.commands.yaw[1]),
            ),
        ],
        Reward(i) => vec![(
            if cfg.kind == "microduck" {
                "Initial weight (curriculum may change it)"
            } else {
                "Weight (0 disables this contribution)"
            },
            cfg.rewards
                .get(*i)
                .ok_or("Unknown reward")?
                .weight
                .to_string(),
        )],
        Observation(i) => {
            let o = cfg.observations.get(*i).ok_or("Unknown observation")?;
            vec![
                ("Enabled: true or false", o.enabled.to_string()),
                ("Observation scale", o.scale.to_string()),
            ]
        }
        Actuator(i, group) => {
            let a = cfg.actuators.get(*i).ok_or("Unknown actuator")?;
            match group {
                0 => vec![
                    (
                        if cfg.kind == "generic_mujoco" {
                            "Action slot (zero based; off = hold)"
                        } else {
                            "Action slot (0–13; all servos required)"
                        },
                        a.action_index
                            .map(|i| i.to_string())
                            .unwrap_or("off".into()),
                    ),
                    ("Direction (+1 or -1)", a.direction.to_string()),
                    ("Target scale (rad or m per action)", a.scale.to_string()),
                ],
                1 if cfg.kind == "microduck" => vec![
                    ("Target offset (rad)", a.offset.to_string()),
                    ("BAM firmware proportional gain", a.kp.to_string()),
                ],
                1 => vec![
                    ("Target offset (rad or m)", a.offset.to_string()),
                    ("Proportional gain Kp", a.kp.to_string()),
                    ("Derivative gain Kd", a.kd.to_string()),
                ],
                _ => vec![
                    (
                        if cfg.kind == "microduck" {
                            "Effort cap (Nm; 0 = original BAM limit)"
                        } else {
                            "Effort limit (Nm or N)"
                        },
                        a.effort.to_string(),
                    ),
                    (
                        if cfg.kind == "microduck" {
                            "Target velocity cap (rad/s; 0 = no added cap)"
                        } else {
                            "Target velocity limit (rad/s or m/s)"
                        },
                        a.velocity.to_string(),
                    ),
                ],
            }
        }
    };
    Ok(values.into_iter().map(|(a, b)| (a.into(), b)).collect())
}
pub fn view(d: &Draft) -> Result<View, String> {
    let c = &d.config;
    let (title,info) = match &d.page {
        Page::Home=>(c.name.clone(),format!("{} · revision {} · {} observations / {} actions. {} environments × {} steps × {} iterations on {}. Save activates this task for new runs. Existing runs keep their captured recipe.", match c.kind.as_str() {"open_duck"=>"Open Duck joystick / imitation task", "microduck"=>"Official Microduck flat velocity / BAM task", _=>"Editable URDF velocity task"},c.revision,c.observation_size(),c.actions(),c.ppo.num_envs,c.ppo.steps_per_env,c.ppo.iterations,c.ppo.device)),
        Page::Ppo=>("PPO learner".into(),if c.kind == "microduck" {"mps runs policy learning on the Mac GPU with CPU physics. cuda runs learning and mjlab physics on an available NVIDIA GPU."} else {"Physics runs in MuJoCo on CPU; mps runs policy learning on the Mac GPU."}.into()),
        Page::Simulation=>("Simulation".into(),"These settings control the captured MuJoCo model and episode termination.".into()),
        Page::Rewards(_)=>("Reward weights".into(),match c.kind.as_str() {
            "microduck"=>"Weighted terms are summed and multiplied by the control period. The curriculum can change weights during training. Use curriculum off for constant weights.",
            "open_duck"=>"Weighted terms are summed and multiplied by the control period, then clipped to 0–10,000 as in the original task. Zero disables a contribution.",
            _=>"Weighted terms are summed and multiplied by the control period. Zero disables a contribution."
        }.into()),
        Page::Observations(_)=>("Observation layout".into(),"Enabled observations are packed in this order. Scale applies before the learned normalizer. Layout changes create a new policy contract.".into()),
        Page::Actuators(_)=>("Actuator mapping".into(),if c.kind=="generic_mujoco" {"Selecting an occupied action slot swaps it. Off holds the joint at its neutral pose plus offset. Each joint's gains and limits affect physics."} else {"All 14 servo channels are required. Selecting an occupied action slot swaps it. Each joint's calibration and limits affect physics."}.into()),
        Page::Fields(Field::Curriculum)=>("Curriculum schedule".into(),"upstream follows the original staged schedule with your edited weights; hold keeps the initial stage; off uses your constant reward weights without a curriculum.".into()),
        Page::Fields(f)=>(match f {Field::Reward(i)=>format!("Reward · {}",c.rewards.get(*i).ok_or("Unknown reward")?.name),
            Field::Observation(i)=>format!("Observation · {}",c.observations.get(*i).ok_or("Unknown observation")?.name),
            Field::Actuator(i,g)=>format!("{} · {}/3",c.actuators.get(*i).ok_or("Unknown actuator")?.joint,g+1),_=>format!("{f:?}")},
            "Apply validates these values and keeps them in the recoverable draft. Save task activates all edits.".into()),
        Page::Discard=>("Discard task draft?".into(),"This removes the saved draft and its unapplied text. The active task and historical runs are preserved.".into()),
    };
    let mut inputs = if let Page::Fields(f) = &d.page {
        fields(c, f)?
    } else {
        vec![]
    };
    if let Some(form) = &d.form {
        if d.page == Page::Fields(form.field.clone()) {
            for (input, value) in inputs.iter_mut().zip(&form.values) {
                input.1 = value.clone();
            }
        }
    }
    Ok(View {
        title,
        info,
        inputs,
        options: choices(d).into_iter().map(|(s, _)| (s, false)).collect(),
        ok: match d.page {
            Page::Home => Some("Save task".into()),
            Page::Fields(Field::Actuator(_, g)) if g < 2 => Some("Apply and next".into()),
            Page::Fields(_) => Some("Apply".into()),
            _ => None,
        },
        danger: match d.page {
            Page::Home => Some("Discard draft…".into()),
            Page::Discard => Some("Discard draft".into()),
            _ => None,
        },
        back: d.page != Page::Home,
    })
}
fn apply_form(d: &mut Draft) -> Result<(), String> {
    let Some(form) = &d.form else { return Ok(()) };
    let mut c = d.config.clone();
    fn number<T: std::str::FromStr>(v: &[String], i: usize) -> Result<T, String> {
        v.get(i)
            .ok_or("Missing input")?
            .trim()
            .parse()
            .map_err(|_| format!("Invalid value in field {}", i + 1))
    }
    fn bounds(v: &str) -> Result<[f64; 2], String> {
        let n = v
            .split(',')
            .map(|s| s.trim().parse::<f64>())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| "Use two numeric bounds separated by a comma")?;
        n.try_into().map_err(|_| "Use exactly two bounds".into())
    }
    let v = &form.values;
    use Field::*;
    match &form.field {
        Curriculum => {
            c.options.insert(
                "curriculum".into(),
                v.first().ok_or("Missing curriculum mode")?.trim().into(),
            );
        }
        Name => c.name = v.first().ok_or("Missing name")?.trim().into(),
        Budget => {
            c.ppo.num_envs = number(v, 0)?;
            c.ppo.iterations = number(v, 1)?;
            c.ppo.steps_per_env = number(v, 2)?;
        }
        Update => {
            c.ppo.learning_rate = number(v, 0)?;
            c.ppo.epochs = number(v, 1)?;
            c.ppo.minibatches = number(v, 2)?;
        }
        Objective => {
            c.ppo.gamma = number(v, 0)?;
            c.ppo.gae_lambda = number(v, 1)?;
            c.ppo.entropy = number(v, 2)?;
        }
        Network => {
            c.ppo.clip = number(v, 0)?;
            c.ppo.hidden = v
                .get(1)
                .ok_or("Missing hidden layers")?
                .split(',')
                .map(|s| s.trim().parse::<u32>())
                .collect::<Result<_, _>>()
                .map_err(|_| "Use comma-separated integer layer sizes")?;
            c.ppo.checkpoint_every = number(v, 2)?;
        }
        Device => c.ppo.device = v.first().ok_or("Missing device")?.trim().into(),
        Timing => {
            c.simulation.timestep = number(v, 0)?;
            c.simulation.decimation = number(v, 1)?;
            c.simulation.episode_seconds = number(v, 2)?;
        }
        World => {
            if c.kind == "generic_mujoco" {
                c.simulation.fixed_base = number(v, 0)?;
                c.simulation.spawn_height = number(v, 1)?;
                c.simulation.friction = number(v, 2)?;
            } else {
                c.simulation.spawn_height = number(v, 0)?;
                c.simulation.friction = number(v, 1)?;
            }
        }
        Termination => {
            c.simulation.min_height = number(v, 0)?;
            c.simulation.max_tilt_degrees = number(v, 1)?;
        }
        Commands => {
            if v.len() != 3 {
                return Err("Missing command ranges".into());
            }
            c.commands.forward = bounds(&v[0])?;
            c.commands.lateral = bounds(&v[1])?;
            c.commands.yaw = bounds(&v[2])?;
        }
        Reward(i) => c.rewards.get_mut(*i).ok_or("Unknown reward")?.weight = number(v, 0)?,
        Observation(i) => {
            let o = c.observations.get_mut(*i).ok_or("Unknown observation")?;
            o.enabled = number(v, 0)?;
            o.scale = number(v, 1)?;
        }
        Actuator(i, g) => {
            if *g == 0 {
                let slot = if v.first().is_some_and(|s| s.trim() == "off") {
                    None
                } else {
                    Some(number(v, 0)?)
                };
                c.set_action_slot(*i, slot)?;
            }
            let a = c.actuators.get_mut(*i).ok_or("Unknown actuator")?;
            match g {
                0 => {
                    a.direction = number(v, 1)?;
                    a.scale = number(v, 2)?;
                }
                1 => {
                    a.offset = number(v, 0)?;
                    a.kp = number(v, 1)?;
                    if c.kind != "microduck" {
                        a.kd = number(v, 2)?;
                    }
                }
                _ => {
                    a.effort = number(v, 0)?;
                    a.velocity = number(v, 1)?;
                }
            }
        }
    }
    c.validate()?;
    d.config = c;
    d.form = None;
    Ok(())
}
fn open(d: &mut Draft, page: Page) -> Result<(), String> {
    apply_form(d)?;
    if let Page::Fields(f) = &page {
        d.form = Some(Form {
            field: f.clone(),
            values: fields(&d.config, f)?.into_iter().map(|(_, v)| v).collect(),
        });
    }
    d.page = page;
    Ok(())
}
pub fn start(st: &mut Store) -> Result<(), String> {
    if st.task_draft.is_none() {
        let r = st
            .sel
            .robot
            .as_ref()
            .and_then(|id| st.robots.iter().find(|r| &r.id == id))
            .ok_or("Select a robot")?;
        st.task_draft = Some(Draft {
            robot_id: r.id.clone(),
            config: r
                .task
                .clone()
                .map(Ok)
                .unwrap_or_else(|| Config::generic(r))?,
            page: Page::Home,
            form: None,
        });
    }
    let d = st.task_draft.as_ref().unwrap();
    st.sel.robot = Some(d.robot_id.clone());
    st.mode = Mode::Robots;
    st.modal = AppModal::TaskEditor;
    Ok(())
}
pub fn start_specialized(st: &mut Store) -> Result<(), String> {
    if st.task_draft.is_some() {
        return start(st);
    }
    let robot = st
        .sel
        .robot
        .as_ref()
        .and_then(|id| st.robots.iter().find(|r| &r.id == id))
        .ok_or("Select a robot")?;
    let config = robot
        .task
        .as_ref()
        .filter(|c| c.kind != "generic_mujoco")
        .cloned()
        .map(Ok)
        .unwrap_or_else(|| Config::specialized(robot))?;
    st.task_draft = Some(Draft {
        robot_id: robot.id.clone(),
        config,
        page: Page::Home,
        form: None,
    });
    start(st)
}
pub fn start_generic(st: &mut Store) -> Result<(), String> {
    if st.task_draft.is_some() {
        return start(st);
    }
    let robot = st
        .sel
        .robot
        .as_ref()
        .and_then(|id| st.robots.iter().find(|r| &r.id == id))
        .ok_or("Select a robot")?;
    let config = robot
        .task
        .as_ref()
        .filter(|c| c.kind == "generic_mujoco")
        .cloned()
        .map(Ok)
        .unwrap_or_else(|| Config::generic(robot))?;
    st.task_draft = Some(Draft {
        robot_id: robot.id.clone(),
        config,
        page: Page::Home,
        form: None,
    });
    start(st)
}
pub fn input(st: &mut Store, i: usize, value: String) {
    if st.modal != AppModal::TaskEditor {
        return;
    }
    if let Some(v) = st
        .task_draft
        .as_mut()
        .and_then(|d| d.form.as_mut())
        .and_then(|f| f.values.get_mut(i))
    {
        *v = value;
    }
}
pub fn option(st: &mut Store, i: usize) -> Result<(), String> {
    let d = st.task_draft.as_mut().ok_or("No task draft")?;
    let page = choices(d)
        .get(i)
        .map(|(_, p)| p.clone())
        .ok_or("Unknown editor option")?;
    open(d, page)
}
pub fn back(st: &mut Store) -> Result<(), String> {
    let d = st.task_draft.as_mut().ok_or("No task draft")?;
    // Back keeps invalid/unapplied text in the current field; cancellation
    // closes the editor while preserving it for restart recovery.
    let page = match &d.page {
        Page::Fields(Field::Actuator(i, _)) => Page::Actuators(i / 6),
        Page::Fields(Field::Reward(i)) => Page::Rewards(i / 6),
        Page::Fields(Field::Observation(i)) => Page::Observations(i / 6),
        _ => Page::Home,
    };
    open(d, page)
}
pub fn accept(st: &mut Store) -> Result<(), String> {
    let d = st.task_draft.as_mut().ok_or("No task draft")?;
    let page = d.page.clone();
    apply_form(d)?;
    if page == Page::Home {
        d.config.validate_model()?;
        let r = st
            .robots
            .iter_mut()
            .find(|r| r.id == d.robot_id)
            .ok_or("Draft robot was removed")?;
        if d.config.robot != r.model_name() {
            return Err("Task robot identity changed".into());
        }
        d.config.revision = r.task.as_ref().map(|c| c.revision + 1).unwrap_or(1);
        r.mapped = d.config.actions() as u32;
        r.task = Some(d.config.clone());
        r.use_task = true;
        st.task_draft = None;
        st.modal = AppModal::None;
        st.toast("Task saved. New runs will use this captured configuration.".into());
    } else {
        let next = match page {
            Page::Fields(Field::Actuator(i, g)) if g < 2 => Page::Fields(Field::Actuator(i, g + 1)),
            Page::Fields(Field::Actuator(i, _)) => Page::Actuators(i / 6),
            Page::Fields(Field::Reward(i)) => Page::Rewards(i / 6),
            Page::Fields(Field::Observation(i)) => Page::Observations(i / 6),
            _ => Page::Home,
        };
        open(d, next)?;
    }
    Ok(())
}
pub fn discard(st: &mut Store) {
    if st
        .task_draft
        .as_ref()
        .is_some_and(|d| d.page == Page::Discard)
    {
        st.task_draft = None;
        st.modal = AppModal::None;
        st.toast("Task draft discarded".into());
    } else if let Some(d) = &mut st.task_draft {
        d.page = Page::Discard;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn specialized_presets_enforce_original_servo_and_observation_contracts() {
        for (json, expected) in [
            (
                include_str!("../../../tools/training/task-presets/open_duck.json"),
                101,
            ),
            (
                include_str!("../../../tools/training/task-presets/microduck.json"),
                61,
            ),
        ] {
            let mut c: Config = serde_json::from_str(json).unwrap();
            c.model = "robot.urdf".into();
            c.validate().unwrap();
            assert_eq!(c.observation_size(), expected);
            c.set_action_slot(0, Some(1)).unwrap();
            c.validate().unwrap();
            assert_eq!(c.actuators[1].action_index, Some(0));
            c.actuators[0].action_index = None;
            assert!(c.validate().is_err());
            c.actuators[0].action_index = Some(1);
            c.simulation.fixed_base = true;
            assert!(c.validate().is_err());
        }
    }
    #[test]
    fn microduck_editor_preserves_firmware_calibration_and_curriculum() {
        let mut c: Config = serde_json::from_str(include_str!(
            "../../../tools/training/task-presets/microduck.json"
        ))
        .unwrap();
        c.model = "robot.urdf".into();
        let mut d = Draft {
            robot_id: "micro".into(),
            config: c,
            page: Page::Home,
            form: None,
        };
        assert!(view(&d).unwrap().info.contains("Official Microduck"));
        open(&mut d, Page::Fields(Field::Curriculum)).unwrap();
        d.form.as_mut().unwrap().values[0] = "invalid".into();
        assert!(apply_form(&mut d).is_err());
        d.form.as_mut().unwrap().values[0] = "hold".into();
        apply_form(&mut d).unwrap();
        open(&mut d, Page::Fields(Field::Actuator(0, 1))).unwrap();
        assert_eq!(d.form.as_ref().unwrap().values.len(), 2);
        d.form.as_mut().unwrap().values = vec!["0.1".into(), "180".into()];
        apply_form(&mut d).unwrap();
        d.config.observations.last_mut().unwrap().enabled = false;
        let recovered: Draft = serde_json::from_str(&serde_json::to_string(&d).unwrap()).unwrap();
        assert_eq!(recovered.config.observation_size(), 55);
        assert_eq!(recovered.config.options["curriculum"], "hold");
        assert_eq!(recovered.config.actuators[0].kp, 180.);
        recovered.config.validate().unwrap();
        d.config.actuators[0].kd = 1.;
        assert!(d.config.validate().is_err());
    }
    fn store() -> Store {
        let mut s = Store::seed();
        s.sel.robot = Some(s.robots[0].id.clone());
        start(&mut s).unwrap();
        s
    }
    #[test]
    fn failed_field_edit_keeps_text_and_previous_valid_contract() {
        let mut s = store();
        option(&mut s, 1).unwrap();
        option(&mut s, 0).unwrap();
        let before = s.task_draft.as_ref().unwrap().config.clone();
        input(&mut s, 0, "not a number".into());
        assert!(accept(&mut s).is_err());
        let d = s.task_draft.as_ref().unwrap();
        assert_eq!(d.config, before);
        assert_eq!(d.form.as_ref().unwrap().values[0], "not a number");
        input(&mut s, 0, "4".into());
        input(&mut s, 1, "2".into());
        input(&mut s, 2, "8".into());
        accept(&mut s).unwrap();
        assert_eq!(s.task_draft.as_ref().unwrap().config.ppo.num_envs, 4);
        assert!(!s.robots[0].use_task);
        accept(&mut s).unwrap();
        assert!(s.robots[0].use_task);
        assert_eq!(s.robots[0].task.as_ref().unwrap().ppo.steps_per_env, 8);
    }
    #[test]
    fn saved_run_uses_its_original_recipe_after_library_edits() {
        let mut s = store();
        accept(&mut s).unwrap();
        let original = s.robots[0].task.clone().unwrap();
        let mut run = s.runs[0].clone();
        run.snapshot.backend = Some("external".into());
        run.snapshot.robot = Some(original.robot.clone());
        run.snapshot.training_profile = Some(
            serde_json::json!({"robot":original.robot,"task_config":original,
            "recipe":{"task":"captured task","iterations":3,"num_envs":4,"steps_per_env":8}}),
        );
        let snapshot = run.snapshot.training_profile.clone();
        let mut edited = original.clone();
        edited.rewards[0].weight = 123.;
        edited.ppo.iterations = 500;
        crate::nexus::configure_external_run(&mut run, Some(&edited)).unwrap();
        assert_eq!(run.snapshot.training_profile, snapshot);
        assert_eq!(run.iters_per, 3);
    }
    #[test]
    fn mapping_swap_disable_enable_and_last_action_guard() {
        let mut c = store().task_draft.unwrap().config;
        let n = c.actions();
        assert!(n > 2);
        c.set_action_slot(0, Some(1)).unwrap();
        assert_eq!(c.actuators[1].action_index, Some(0));
        c.set_action_slot(0, None).unwrap();
        assert_eq!(c.actions(), n - 1);
        c.validate().unwrap();
        c.set_action_slot(0, Some(0)).unwrap();
        assert_eq!(c.actions(), n);
        c.validate().unwrap();
        for i in 1..c.actuators.len() {
            c.set_action_slot(i, None).unwrap();
        }
        assert!(c.set_action_slot(0, None).is_err());
        c.validate().unwrap();
    }
    #[test]
    fn opening_another_robot_preserves_and_selects_existing_draft() {
        let mut s = store();
        let id = s.task_draft.as_ref().unwrap().robot_id.clone();
        let mut robot = s.robots[0].clone();
        robot.id = "another".into();
        s.robots.push(robot);
        s.sel.robot = Some("another".into());
        start(&mut s).unwrap();
        assert_eq!(s.sel.robot, Some(id));
        discard(&mut s);
        assert!(s.task_draft.is_some());
        discard(&mut s);
        assert!(s.task_draft.is_none());
    }
}
