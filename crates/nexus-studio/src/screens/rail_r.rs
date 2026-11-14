//! Right rail: per-mode content built as a list of RSpec lines rendered into
//! reusable RLine slots.

use super::*;
use crate::kit::*;

#[derive(Clone, Debug)]
pub enum RKind {
    Cap {
        text: String,
        btn: Option<(String, String)>,
    }, // caption + optional (label, act)
    Tiles(Vec<(String, String, u8)>), // (k, v, accent 0/1/2)
    Chk {
        st: f64,
        nm: String,
        ds: String,
        act: Option<(String, String, bool, String)>,
        cancel: Option<String>,
    },
    Sld {
        lb: String,
        field: String,
        min: f64,
        max: f64,
        val: f64,
        fmt: String,
    },
    Txt(String),
    Mono(String),
    Ver {
        tone: f64,
        text: String,
    },
    Row {
        nm: String,
        tg: String,
        pill: Option<(String, f64)>,
        on: bool,
        click: String,
    },
    Btns(Vec<(String, String, bool, bool)>), // label, act, enabled, hot
    Cbx {
        lb: String,
        idx: usize,
        val: bool,
    },
    SparkLine {
        data: Vec<f64>,
        goal: Option<f64>,
        tone: u8,
    },
    Segs(Vec<(f64, SegTone)>),
    Estop {
        adv: Option<(String, String, bool)>,
        rollback_en: bool,
    },
}

#[derive(Clone, Debug)]
pub struct RSpec {
    pub kind: RKind,
}

impl Default for RSpec {
    fn default() -> Self {
        RSpec {
            kind: RKind::Txt(String::new()),
        }
    }
}

fn cap(text: String) -> RSpec {
    RSpec {
        kind: RKind::Cap { text, btn: None },
    }
}
fn capb(text: String, blabel: String, act: &str) -> RSpec {
    RSpec {
        kind: RKind::Cap {
            text,
            btn: Some((blabel, act.into())),
        },
    }
}
fn txt(t: String) -> RSpec {
    RSpec {
        kind: RKind::Txt(t),
    }
}
fn mono(t: String) -> RSpec {
    RSpec {
        kind: RKind::Mono(t),
    }
}
fn ver(tone: f64, text: String) -> RSpec {
    RSpec {
        kind: RKind::Ver { tone, text },
    }
}
fn btns(b: Vec<(String, String, bool, bool)>) -> RSpec {
    RSpec {
        kind: RKind::Btns(b),
    }
}

fn gate_line(
    st: &Store,
    id: GateId,
    label: String,
    detail: String,
    dis: bool,
    reason: String,
) -> RSpec {
    let g = st.dg.gate(id);
    let lang = st.lang;
    let (gst, ds, act, cancel) = match g {
        Gate::NotRun => (
            3.0,
            format!("{detail} · {}", tr(lang, "not run")),
            Some((
                tr(lang, "run").to_string(),
                gate_act(id, false),
                !dis,
                reason,
            )),
            None,
        ),
        Gate::Running(p) => (
            1.0,
            running_detail(st, id, *p),
            None,
            Some(gate_act(id, true)),
        ),
        Gate::Pass { .. } => (0.0, pass_detail(st, id), None, None),
        Gate::Fail { why } => (
            2.0,
            trf(
                lang,
                why.k,
                &why.a.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
            ),
            Some((
                tr(lang, "re-run").to_string(),
                gate_act(id, false),
                !dis,
                reason,
            )),
            None,
        ),
    };
    RSpec {
        kind: RKind::Chk {
            st: gst,
            nm: label,
            ds,
            act,
            cancel,
        },
    }
}

fn gate_act(id: GateId, cancel: bool) -> String {
    let base = match id {
        GateId::Export => "export",
        GateId::Compat => "compat",
        GateId::Sim2sim => "sim2sim",
        GateId::Dryrun => "dryrun",
    };
    if cancel {
        format!("cancel-{base}")
    } else {
        format!("gate-{base}")
    }
}

fn running_detail(st: &Store, id: GateId, p: u32) -> String {
    let lang = st.lang;
    match id {
        GateId::Export => tr(lang, "exporting…").to_string(),
        GateId::Compat => trf(lang, "checking {0}/5…", &[&(p / 2).min(5).to_string()]),
        GateId::Sim2sim => trf(lang, "episode {0}/24…", &[&p.to_string()]),
        GateId::Dryrun => tr(lang, "shadow running…").to_string(),
    }
}

fn pass_detail(st: &Store, id: GateId) -> String {
    let lang = st.lang;
    match id {
        GateId::Export => {
            if let Gate::Pass { hash, kb, .. } = &st.dg.export {
                trf(lang, "g1_policy.onnx · opset 17 · {0} KB · id {1} · fnv-32 demo fingerprint ✓", &[&kb.to_string(), hash])
            } else {
                String::new()
            }
        }
        GateId::Compat => tr(lang, "obs 480 (96×5) ✓ · act 29 ✓ · 50 Hz policy / 1 kHz command ✓ · position limits clipped to profile (note) · sdk 2.0 ✓").to_string(),
        GateId::Sim2sim => tr(lang, "24 episodes · survival 96% ≥ 95% ✓ · drift 6.8% ≤ 8% ✓").to_string(),
        GateId::Dryrun => tr(lang, "damped robot · policy shadowed 30 s · saturation 9% · jerk p99 0.4 ✓").to_string(),
    }
}

fn fmt_field(v: f64, k: &str, lang: usize) -> String {
    match k {
        "stance" => format!("{v:.2} m"),
        "dur" => format!("{v:.0} s"),
        "slope" => format!("{v:.0}°"),
        "push" => {
            if v <= 0.0 {
                tr(lang, "off").to_string()
            } else {
                format!("{v:.2}")
            }
        }
        "vx" => format!("{v:.2}"),
        _ => format!("{v:.2}×"),
    }
}

pub fn build_right_specs(app: &App) -> Vec<RSpec> {
    let st = &app.st;
    let lang = st.lang;
    let tt = |k: &str| tr(lang, k).to_string();
    let tf = |k: &str, a: &[&str]| trf(lang, k, a);
    let mut out: Vec<RSpec> = vec![];
    match st.mode {
        Mode::Scenes => {
            let sc = st.cur_scene();
            let dirty_chip = if st.dirty {
                format!(" · {}", tt("● unsaved"))
            } else {
                String::new()
            };
            out.push(cap(format!(
                "{} · {}{}",
                caps(&tt("Editor")),
                sc.name,
                dirty_chip
            )));
            out.push(btns(vec![
                (tt("Revert"), "revert".into(), st.dirty, false),
                (tt("Save"), "save".into(), st.dirty, false),
                (tt("Save as…"), "saveas".into(), true, false),
            ]));
            out.push(cap(caps(&tt("Terrain"))));
            let fams = ["", "boxes", "rough", "wave", "step"];
            out.push(btns(
                fams.iter()
                    .map(|f| {
                        let label = if f.is_empty() { tt("flat") } else { tt(f) };
                        (label, format!("fam:{f}"), true, false)
                    })
                    .collect(),
            ));
            out.push(RSpec {
                kind: RKind::Sld {
                    lb: tt("relief"),
                    field: "amp".into(),
                    min: 0.25,
                    max: 3.0,
                    val: sc.amp,
                    fmt: fmt_field(sc.amp, "amp", lang),
                },
            });
            out.push(RSpec {
                kind: RKind::Sld {
                    lb: tt("grade"),
                    field: "slope".into(),
                    min: 0.0,
                    max: 30.0,
                    val: sc.slope,
                    fmt: fmt_field(sc.slope, "slope", lang),
                },
            });
            out.push(mono(format!("{} {}", tt("seed"), sc.seed)));
            out.push(cap(caps(&tt("Goal & command"))));
            out.push(RSpec {
                kind: RKind::Sld {
                    lb: tt("stance"),
                    field: "stance".into(),
                    min: 0.45,
                    max: 0.84,
                    val: sc.stance,
                    fmt: fmt_field(sc.stance, "stance", lang),
                },
            });
            out.push(RSpec {
                kind: RKind::Sld {
                    lb: "vx".into(),
                    field: "vx".into(),
                    min: -0.6,
                    max: 0.6,
                    val: sc.vx,
                    fmt: fmt_field(sc.vx, "vx", lang),
                },
            });
            out.push(RSpec {
                kind: RKind::Sld {
                    lb: tt("length"),
                    field: "dur".into(),
                    min: 2.0,
                    max: 20.0,
                    val: sc.dur,
                    fmt: fmt_field(sc.dur, "dur", lang),
                },
            });
            out.push(cap(caps(&tt("Fixed for the whole run"))));
            out.push(RSpec {
                kind: RKind::Sld {
                    lb: tt("friction"),
                    field: "friction".into(),
                    min: 0.15,
                    max: 1.5,
                    val: sc.friction,
                    fmt: fmt_field(sc.friction, "friction", lang),
                },
            });
            out.push(RSpec {
                kind: RKind::Sld {
                    lb: tt("PD gain"),
                    field: "kp".into(),
                    min: 0.4,
                    max: 1.6,
                    val: sc.kp,
                    fmt: fmt_field(sc.kp, "kp", lang),
                },
            });
            out.push(RSpec {
                kind: RKind::Sld {
                    lb: tt("mass"),
                    field: "mass".into(),
                    min: 0.5,
                    max: 2.0,
                    val: sc.mass,
                    fmt: fmt_field(sc.mass, "mass", lang),
                },
            });
            out.push(RSpec {
                kind: RKind::Sld {
                    lb: tt("push"),
                    field: "push".into(),
                    min: 0.0,
                    max: 1.5,
                    val: sc.push,
                    fmt: fmt_field(sc.push, "push", lang),
                },
            });
            out.push(cap(caps(&tt("Varied per environment · by the trainer"))));
            out.push(btns(vec![
                (tt("force"), "force-fam".into(), true, sc.force_fam),
                (tt("all 4"), "force-fam".into(), true, !sc.force_fam),
                (
                    format!(
                        "{}: {}",
                        tt("spawn state"),
                        if sc.spawn_dr { tt("on") } else { tt("off") }
                    ),
                    "spawn".into(),
                    true,
                    sc.spawn_dr,
                ),
            ]));
            out.push(txt(tt("difficulty")
                + " · "
                + &tt("adaptive · +1 after 4 wins")));
            out.push(txt(tt("Up to 4096 envs in parallel (trainer capacity; budgets use this machine’s measured 512-env throughput). These vary between them — you set whether, not what.")));
        }
        Mode::Robots => {
            out.push(btns(vec![
                ("Import bundle…".into(), "import-robot".into(), true, false),
                (
                    "Discover assets".into(),
                    "robots-discover".into(),
                    true,
                    false,
                ),
            ]));
            out.push(btns(vec![(
                format!("Workspace trash ({})", st.trash.len()),
                "artifacts-trash".into(),
                true,
                false,
            )]));
            match st
                .sel
                .robot
                .as_ref()
                .and_then(|id| st.robots.iter().find(|r| &r.id == id))
            {
                None => out.push(txt(tt("none"))),
                Some(r) => {
                    out.push(cap(r.name.to_uppercase()));
                    out.push(RSpec {
                        kind: RKind::Tiles(vec![
                            (tt("links"), r.links.to_string(), 0),
                            (tt("joints"), r.joints.to_string(), 0),
                            (tt("movable"), r.movable.to_string(), 0),
                        ]),
                    });
                    out.push(RSpec {
                        kind: RKind::Tiles(vec![
                            (tt("mapped"), r.mapped.to_string(), 0),
                            ("assets".into(), r.meshes.to_string(), 0),
                        ]),
                    });
                    out.push(capb(
                        "Training".into(),
                        "Backend…".into(),
                        "backend-profile",
                    ));
                    match crate::nexus::robot_training_task(r) {
                        Ok(Some(task)) => {
                            out.push(txt(task));
                            out.push(btns(vec![(
                                if app.real_proc.is_some() {
                                    "Queue training"
                                } else {
                                    "Start training"
                                }
                                .into(),
                                "real-train".into(),
                                true,
                                true,
                            )]));
                        }
                        Err(error) => out.push(txt(error)),
                        Ok(None) => {}
                    }
                    if ["Open Duck Mini v2", "Microduck"].contains(&r.model_name()) {
                        out.push(btns(vec![(
                            if r.model_name() == "Microduck" {
                                "Edit official task"
                            } else {
                                "Edit joystick task"
                            }
                            .into(),
                            "task-specialized".into(),
                            true,
                            false,
                        )]));
                    }
                    out.push(btns(vec![(
                        if st.task_draft.is_some() {
                            "Resume task draft"
                        } else if r.task.is_some() {
                            "Edit task"
                        } else {
                            "Create URDF task"
                        }
                        .into(),
                        "task-edit".into(),
                        true,
                        r.use_task,
                    )]));
                    if r.task.as_ref().is_some_and(|c| c.kind != "generic_mujoco") {
                        out.push(btns(vec![(
                            "Create URDF task".into(),
                            "task-generic".into(),
                            true,
                            false,
                        )]));
                    }
                    if r.use_task {
                        if let Some(c) = &r.task {
                            out.push(txt(format!("Revision {} · {} observations / {} actions · {} environments × {} iterations · {} learner",c.revision,c.observation_size(),c.actions(),c.ppo.num_envs,c.ppo.iterations,c.ppo.device)));
                        }
                        out.push(btns(vec![(
                            "Use trainer profile".into(),
                            "task-profile".into(),
                            true,
                            false,
                        )]));
                    }
                    out.push(btns(vec![(
                        "Unitree G1".into(),
                        "backend-native".into(),
                        true,
                        false,
                    )]));
                    out.push(cap(caps(&tt("Validation"))));
                    out.push(txt("URDF structure and referenced assets inspected. Training compatibility is defined by the selected backend.".into()));
                    out.push(cap(caps(&tt("Used by"))));
                    let references = st.robot_references(&r.id);
                    out.push(txt(if references.is_empty() {
                        "No run or recording references".into()
                    } else {
                        format!(
                            "{} run/recording references; removal lists the affected records.",
                            references.len()
                        )
                    }));
                    out.push(cap(caps(&tt("Actions"))));
                    out.push(btns(vec![(
                        "Export robot bundle…".into(),
                        "robot-export".into(),
                        r.urdf.is_some(),
                        false,
                    )]));
                    out.push(btns(vec![
                        (tt("rename"), "robot-rename".into(), true, false),
                        (tt("Edit actuators"), "robot-remap".into(), true, false),
                        (tt("remove"), "robot-remove".into(), true, false),
                    ]));
                    out.push(txt("Task settings capture actuator calibration for future runs. Importing another URDF creates a separate robot entry.".into()));
                }
            }
        }
        Mode::Train => {
            out.push(cap("Training queue".into()));
            out.push(btns(vec![
                (
                    "Launch settings…".into(),
                    "queue-settings".into(),
                    true,
                    false,
                ),
                (
                    if st.queue_enabled {
                        "Hold queue"
                    } else {
                        "Run queue"
                    }
                    .into(),
                    "queue-toggle".into(),
                    true,
                    false,
                ),
            ]));
            out.push(txt(format!(
                "{} launch · {} learner(s) · {} waiting",
                st.job_settings.transport,
                st.job_settings.processes,
                st.runs
                    .iter()
                    .filter(|r| r.state == RunState::Queued)
                    .count()
            )));
            for queued in st
                .runs
                .iter()
                .filter(|r| r.state == RunState::Queued)
                .take(8)
            {
                out.push(txt(format!(
                    "{} · {}",
                    queued.name,
                    queued
                        .job
                        .as_ref()
                        .map(|j| j.settings.transport.as_str())
                        .unwrap_or("local")
                )));
                out.push(btns(vec![
                    (
                        "Move first".into(),
                        format!("queue-first:{}", queued.id),
                        true,
                        false,
                    ),
                    (
                        "Run now".into(),
                        format!("queue-now:{}", queued.id),
                        true,
                        false,
                    ),
                    (
                        "Cancel".into(),
                        format!("queue-cancel:{}", queued.id),
                        true,
                        true,
                    ),
                ]));
            }
            let Some(r) = st.sel.run.as_ref().and_then(|id| st.run(id)) else {
                out.push(txt(tt("none")));
                app_rmap_done(&mut out);
                return out;
            };
            out.push(cap(caps(&tt("Diagnosis"))));
            let (tone, vtext) = match r.state {
                RunState::Failed => (
                    2.0,
                    tf("Failed. {0}", &[&tt(r.diagnosis.as_deref().unwrap_or(""))]),
                ),
                RunState::Completed => (
                    0.0,
                    tt("Completed. Budget reached; the record is immutable."),
                ),
                RunState::Stopped => (
                    1.0,
                    tt("Stopped early. Partial checkpoints kept; the record is immutable."),
                ),
                RunState::Paused => (
                    1.0,
                    "Worker paused with optimizer and environment state retained in memory.".into(),
                ),
                RunState::Queued => (1.0, "Waiting in the saved training queue".into()),
                RunState::Starting => (1.0, "Connecting to the job supervisor".into()),
                RunState::Stopping => {
                    (1.0, "Stop requested; waiting for the worker to exit".into())
                }
                RunState::Unknown => (
                    2.0,
                    "Worker state is unknown; automatic relaunch is blocked".into(),
                ),
                RunState::Running => (
                    0.0,
                    "Training in progress. Assess the measured reward and fall rate below.".into(),
                ),
            };
            let vtext = if r.archived {
                format!("{vtext} · {}", tt("archived"))
            } else {
                vtext
            };
            out.push(ver(tone, vtext));
            if let Some(job) = &r.job {
                out.push(txt(format!(
                    "{} · queued {} · last contact {}",
                    job.settings.transport,
                    job.queued_at,
                    job.last_contact
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "not contacted".into())
                )));
                if let Some(error) = &job.connection_error {
                    out.push(ver(
                        2.,
                        format!("Connection unavailable: {error}. Worker termination is unproven."),
                    ));
                }
                if let Some(error) = &r.diagnosis {
                    out.push(txt(error.clone()));
                }
            }
            if app.external_robot_run().is_some() {
                out.push(cap("Viewport".into()));
                out.push(txt(app.external_motion.label.clone()));
                out.push(txt("One training environment, including sampled actions, falls and resets. Motion runs at the trainer's speed.".into()));
                out.push(btns(vec![(
                    "Checkpoint validation".into(),
                    "external-validation".into(),
                    true,
                    false,
                )]));
            }
            if let Some(job) = &app.export_job {
                out.push(txt(format!("Exporting to {}", job.output.display())));
                out.push(btns(vec![(
                    "Cancel export".into(),
                    "export-cancel".into(),
                    true,
                    true,
                )]));
            }
            out.push(cap(caps(&tt("Checkpoints"))));
            out.push(btns(vec![
                (
                    "Refresh files".into(),
                    "artifacts-refresh".into(),
                    true,
                    false,
                ),
                (
                    "Cleanup…".into(),
                    "artifacts-cleanup".into(),
                    !matches!(r.state, RunState::Running | RunState::Paused),
                    false,
                ),
            ]));
            out.push(txt(tt(if r.set == "robot training task" {
                "Periodic checkpoints and the latest policy are saved in this run's directory."
            } else {
                "Cleanup previews file changes; deleted files can be restored from Runs → Trash."
            })));
            let latest = app
                .real_proc
                .as_ref()
                .filter(|p| p.run_id.as_deref() == Some(r.id.as_str()))
                .and_then(|p| p.artifact.as_ref())
                .filter(|p| std::path::Path::new(p).is_file());
            if let Some(path) = latest {
                out.push(mono(format!("Latest saved: {}", path)));
            } else if r.ckpts.is_empty() {
                out.push(txt(tt("none yet")));
            }
            let disk = if r.snapshot.robot.as_deref() == Some("Unitree G1-29dof") {
                crate::nexus::real_ckpts()
            } else {
                Vec::new()
            };
            if !disk.is_empty() {
                out.push(cap(format!("{} · {}", caps(&tt("On disk")), disk.len())));
                for c in disk.iter().take(5) {
                    out.push(mono(format!("{} · {} KB", c.label, c.kb)));
                }
            }
            if !app.real_lines.is_empty()
                && app
                    .real_proc
                    .as_ref()
                    .is_some_and(|p| p.run_id == st.sel.run)
            {
                out.push(cap(caps(&tt("Real run · disk"))));
                for line in app.real_lines.iter().rev().take(4).rev() {
                    out.push(mono(line.clone()));
                }
            }
            let page = app.checkpoint_page.min(r.ckpts.len().saturating_sub(1) / 2);
            out.push(btns(vec![
                ("← Previous".into(), "ck-page-prev".into(), page > 0, false),
                (
                    format!("Next → ({}/{})", page + 1, r.ckpts.len().div_ceil(2).max(1)),
                    "ck-page-next".into(),
                    (page + 1) * 2 < r.ckpts.len(),
                    false,
                ),
            ]));
            for c in r.ckpts.iter().skip(page * 2).take(2) {
                let pill = match c.st {
                    CkSt::Best => Some((tt("best"), 1.0)),
                    CkSt::Prom => Some((format!("★ {}", c.pname.clone().unwrap_or_default()), 0.0)),
                    CkSt::Ord => None,
                };
                out.push(RSpec {
                    kind: RKind::Row {
                        nm: c.label.clone(),
                        tg: if c.score_known {
                            format!("{:.3}", c.score)
                        } else {
                            "not measured".into()
                        },
                        pill,
                        on: false,
                        click: String::new(),
                    },
                });
                let mut b = vec![];
                if c.st == CkSt::Prom {
                    b.push((tt("Demote"), format!("ck-demote:{}", c.id), true, false));
                    b.push((tt("Deploy →"), format!("ck-deploy:{}", c.id), true, false));
                } else {
                    b.push((tt("Promote"), format!("ck-promote:{}", c.id), true, false));
                }
                b.push((
                    tt("Export checkpoint"),
                    format!("ck-export:{}", c.id),
                    true,
                    false,
                ));
                b.push((
                    "Export ONNX…".into(),
                    format!("ck-onnx:{}", c.id),
                    app.export_job.is_none(),
                    false,
                ));
                b.push((tt("Inspect"), format!("ck-inspect:{}", c.id), true, false));
                if c.st != CkSt::Prom {
                    b.push((tt("Delete"), format!("ck-del:{}", c.id), true, true));
                }
                for row in b.chunks(2) {
                    out.push(btns(row.to_vec()));
                }
            }
        }
        Mode::Inspect => {
            out.push(cap("Live policy simulation".into()));
            out.push(btns(vec![
                (
                    "Start selected policy".into(),
                    "sim-start".into(),
                    crate::nexus::selected_artifact(st).is_some(),
                    false,
                ),
                ("Recover…".into(), "sim-import".into(), true, false),
            ]));
            if let Some(session) = &app.simulation {
                let status = &session.status;
                let ready = matches!(session.state(), "running" | "paused");
                let terminal = status["terminal"].as_bool().unwrap_or(false);
                out.push(txt(format!(
                    "{} · {} control steps · reset {}",
                    session.state(),
                    status["sequence"],
                    status["reset_count"].as_u64().unwrap_or(0)
                )));
                out.push(txt(format!(
                    "Velocity x / y / yaw:\n{} (m/s, m/s, rad/s)\nWorld force: {} N",
                    status["command"], status["force_world"]
                )));
                if let Some(reason) = status["pose"]["terminal_reason"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                {
                    out.push(txt(format!(
                        "Episode ended: {reason}. Restart to continue."
                    )));
                }
                if let Some(error) = status["error"].as_str().filter(|s| !s.is_empty()) {
                    out.push(txt(error.into()));
                }
                out.push(btns(vec![
                    ("Velocity…".into(), "sim-command".into(), ready, false),
                    ("Push…".into(), "sim-push".into(), ready && !terminal, false),
                ]));
                out.push(btns(vec![
                    (
                        "Resume".into(),
                        "sim-resume".into(),
                        ready && !terminal,
                        false,
                    ),
                    ("Pause".into(), "sim-pause".into(), ready, false),
                    ("Step".into(), "sim-step".into(), ready && !terminal, false),
                ]));
                out.push(btns(vec![
                    ("Restart".into(), "sim-restart".into(), ready, false),
                    ("Stop".into(), "sim-stop".into(), ready, false),
                ]));
                out.push(btns(vec![(
                    "Save and review".into(),
                    "sim-save".into(),
                    status["sequence"].as_u64().unwrap_or(0) > 0,
                    false,
                )]));
                out.push(txt("Flat floor · restart seed 42\nPauses at a fall or episode limit.\nRecords every physics control step.\nSave to review commands and forces.".into()));
                return out;
            }
            out.push(cap("Recording review".into()));
            out.push(btns(vec![(
                "Import training stream".into(),
                "review-training".into(),
                st.sel.run.is_some(),
                false,
            )]));
            if let Some(replay) = &app.replay {
                out.push(txt(format!(
                    "{} · {} frames · {:.1} Hz",
                    replay.name,
                    replay.joints.len(),
                    1. / replay.dt
                )));
            }
            if let Some(doc) = &app.review.document {
                let frame = (st.live.frame as usize).min(doc.replay.joints.len().saturating_sub(1));
                out.push(txt(format!(
                    "Source playback · revision {} · {} kept / {} frames",
                    doc.revision,
                    doc.included().len(),
                    doc.replay.joints.len()
                )));
                if let Some(segment) = doc.segments.get(app.review.segment) {
                    out.push(cap(format!(
                        "Episode {} / {}",
                        app.review.segment + 1,
                        doc.segments.len()
                    )));
                    out.push(txt(format!(
                        "{} · source {}..{} · keep {}..{}{}",
                        segment.boundary,
                        segment.start,
                        segment.end,
                        segment.keep_start,
                        segment.keep_end,
                        if segment.excluded { " · EXCLUDED" } else { "" }
                    )));
                    if !segment.annotation.is_empty() {
                        out.push(txt(segment.annotation.clone()));
                    }
                    out.push(btns(vec![
                        (
                            "Previous".into(),
                            format!("review-episode:{}", app.review.segment.saturating_sub(1)),
                            app.review.segment > 0,
                            false,
                        ),
                        (
                            "Next".into(),
                            format!("review-episode:{}", app.review.segment + 1),
                            app.review.segment + 1 < doc.segments.len(),
                            false,
                        ),
                    ]));
                    out.push(btns(vec![
                        ("Trim / annotate…".into(), "review-edit".into(), true, false),
                        (
                            if segment.excluded {
                                "Include"
                            } else {
                                "Exclude"
                            }
                            .into(),
                            "review-exclude".into(),
                            true,
                            false,
                        ),
                    ]));
                    out.push(btns(vec![
                        (
                            "Split at playhead".into(),
                            "review-split".into(),
                            true,
                            false,
                        ),
                        ("Reset edits".into(), "review-reset".into(), true, false),
                    ]));
                }
                out.push(btns(vec![(
                    "Export edited dataset…".into(),
                    "review-export".into(),
                    !doc.included().is_empty(),
                    false,
                )]));
                let labels = [
                    "Pose",
                    "Observation",
                    "Action",
                    "Targets",
                    "Torques",
                    "Rewards",
                    "Command",
                    "World force",
                ];
                for row in (0..8).collect::<Vec<_>>().chunks(3) {
                    out.push(btns(
                        row.iter()
                            .map(|&i| {
                                (
                                    labels[i].into(),
                                    format!("review-channel:{i}"),
                                    app.review.channel != i,
                                    false,
                                )
                            })
                            .collect(),
                    ));
                }
                out.push(cap(format!(
                    "{} · frame {}",
                    labels[app.review.channel.min(7)],
                    frame
                )));
                if let Some(sample) = doc.samples.get(frame) {
                    out.push(mono(format!(
                        "step {} · sim {:.3}s{}",
                        sample.sequence,
                        sample.sim_time,
                        if sample.terminal { " · terminal" } else { "" }
                    )));
                    if !sample.sample_timing.is_empty() {
                        out.push(txt(sample.sample_timing.clone()));
                    }
                }
                let values: Vec<(String, f64)> = if app.review.channel == 0 {
                    let mut v = doc
                        .replay
                        .joint_names
                        .iter()
                        .cloned()
                        .zip(doc.replay.joints[frame].iter().map(|&v| v as f64))
                        .collect::<Vec<_>>();
                    if let Some(base) = doc.replay.base.get(frame) {
                        for (name, value) in ["base x (m)", "base y (m)", "base z (m)"]
                            .into_iter()
                            .zip(base)
                        {
                            v.insert(0, (name.into(), *value as f64));
                        }
                    }
                    v
                } else if let Some(sample) = doc.samples.get(frame) {
                    if app.review.channel == 5 {
                        sample
                            .reward_terms
                            .iter()
                            .map(|(n, &v)| (n.clone(), v))
                            .collect()
                    } else {
                        let vector = match app.review.channel {
                            1 => sample.observation.as_ref(),
                            2 => sample.action.as_ref(),
                            3 => sample.targets.as_ref(),
                            6 => sample.command.as_ref(),
                            7 => sample.force_world.as_ref(),
                            _ => sample.torques.as_ref(),
                        };
                        vector
                            .map(|v| {
                                v.iter()
                                    .enumerate()
                                    .map(|(i, &v)| {
                                        let name = match app.review.channel {
                                            1 => sample.observation_names.get(i),
                                            3 | 4 => sample.actuator_joints.get(i),
                                            _ => None,
                                        }
                                        .cloned()
                                        .unwrap_or_else(|| {
                                            format!("{}[{i}]", labels[app.review.channel])
                                        });
                                        (name, v)
                                    })
                                    .collect()
                            })
                            .unwrap_or_default()
                    }
                } else {
                    vec![]
                };
                if values.is_empty() {
                    out.push(txt(
                        "This source did not record this channel. No values are inferred.".into(),
                    ));
                } else {
                    let pages = values.len().div_ceil(8);
                    let page = app.review.page.min(pages - 1);
                    for (name, value) in values.iter().skip(page * 8).take(8) {
                        out.push(mono(format!("{name}: {value:.5}")));
                    }
                    out.push(btns(vec![
                        (
                            "◀ values".into(),
                            "review-prev-page".into(),
                            page > 0,
                            false,
                        ),
                        (
                            "values ▶".into(),
                            "review-next-page".into(),
                            page + 1 < pages,
                            false,
                        ),
                    ]));
                }
            } else if let Some(replay) = &app.replay {
                let frame = (st.live.frame as usize).min(replay.joints.len().saturating_sub(1));
                if let Some(row) = replay.joints.get(frame) {
                    for (name, value) in replay.joint_names.iter().zip(row).take(12) {
                        out.push(mono(format!("{name}: {value:.4}")));
                    }
                }
                out.push(txt("Save this replay to edit episodes. Import the training stream to inspect its recorded control telemetry.".into()));
            } else {
                out.push(txt("Select a recording on the left or a run with training-motion.jsonl. The training stream records environment 0; telemetry availability depends on the source.".into()));
            }
            out.push(btns(vec![(
                "Save replay copy".into(),
                "record-probe".into(),
                app.replay.is_some(),
                false,
            )]));
        }
        Mode::Validate => {
            out.push(cap("Policy evaluation and comparison".into()));
            out.push(btns(vec![
                (
                    "Evaluate / compare…".into(),
                    "eval-start".into(),
                    app.evaluation.job.is_none() && crate::nexus::selected_artifact(st).is_some(),
                    false,
                ),
                ("Latest report".into(), "eval-latest".into(), true, false),
            ]));
            if let Some(job) = &app.evaluation.job {
                out.push(txt(format!(
                    "{} · {} / {} trials\n{} · seed {}",
                    job.status["state"],
                    job.status["completed"],
                    job.status["total"],
                    job.status["case"],
                    job.status["seed"]
                )));
                out.push(btns(vec![(
                    "Cancel evaluation".into(),
                    "eval-cancel".into(),
                    true,
                    false,
                )]));
            }
            if app.evaluation_visible() {
                let report = app.evaluation.report.as_ref().unwrap();
                for (i, summary) in report["summaries"].as_array().unwrap().iter().enumerate() {
                    out.push(cap(format!(
                        "{} · {}",
                        if i == 0 { "A · left" } else { "B · right" },
                        report["checkpoints"][i]["checkpoint_sha256"]
                            .as_str()
                            .unwrap_or("")
                            .chars()
                            .take(10)
                            .collect::<String>()
                    )));
                    out.push(txt(format!("Survived {} / {}\nTracking passed {} / {}\nMean RMSE x / y / yaw:\n{}\nSurvival 95% interval:\n{}",summary["survived"],summary["total"],summary["tracking_passed"],summary["total"],crate::evaluation::metric_vector(&summary["mean_velocity_rmse"]),crate::evaluation::metric_vector(&summary["survival_ci95"]))));
                }
                if !report["comparison"].is_null() {
                    out.push(cap("Paired difference · B minus A".into()));
                    out.push(txt(format!(
                        "RMSE delta:\n{}\nNegative favors B.\nPaired 95% intervals are in report.",
                        crate::evaluation::metric_vector(&report["comparison"]["mean_rmse_delta"])
                    )));
                }
                let case = &report["results"][0][app.evaluation.case];
                out.push(cap(format!(
                    "Trial {} / {}",
                    app.evaluation.case + 1,
                    report["results"][0].as_array().unwrap().len()
                )));
                out.push(txt(format!(
                    "{} · seed {}\nCommand: {}\nPlayback holds terminal poses.",
                    case["case"].as_str().unwrap_or(""),
                    case["seed"],
                    crate::evaluation::metric_vector(&case["command"])
                )));
                out.push(btns(vec![
                    (
                        "Previous".into(),
                        "eval-prev".into(),
                        app.evaluation.case > 0,
                        false,
                    ),
                    (
                        "Next".into(),
                        "eval-next".into(),
                        app.evaluation.case + 1 < report["results"][0].as_array().unwrap().len(),
                        false,
                    ),
                ]));
                out.push(btns(vec![
                    ("Export report…".into(), "eval-export".into(), true, false),
                    ("Close report".into(), "eval-close".into(), true, false),
                ]));
                return out;
            }
            if app.external_robot_run().is_some() {
                out.push(cap("Checkpoint validation".into()));
                if let Some(v) = app.external_validation() {
                    out.push(ver(
                        if v.tracking_passed == v.total {
                            0.0
                        } else {
                            2.0
                        },
                        format!(
                            "Command tracking: {} / {} passed",
                            v.tracking_passed, v.total
                        ),
                    ));
                    out.push(txt(format!(
                        "Survival: {} / {} · checkpoint {}",
                        v.survived, v.total, v.iteration
                    )));
                    if let Some(case) = v.cases.get(app.external_motion.case) {
                        out.push(cap(format!("{} · seed {}", case.case, case.seed)));
                        out.push(mono(format!(
                            "Requested: vx {:.2} · vy {:.2} m/s\nyaw {:.2} rad/s",
                            case.command[0], case.command[1], case.command[2]
                        )));
                        out.push(mono(format!(
                            "Achieved: vx {:.4} · vy {:.4} m/s\nyaw {:.4} rad/s",
                            case.upright_mean_velocity[0],
                            case.upright_mean_velocity[1],
                            case.upright_mean_velocity[2]
                        )));
                        out.push(txt(format!(
                            "Maximum tilt: {:.1}° · {}",
                            case.max_tilt_deg,
                            if case.survived {
                                "stayed upright"
                            } else {
                                "survival failed"
                            }
                        )));
                    }
                    let controller = if app
                        .external_robot_run()
                        .is_some_and(|(_, r)| r == "Microduck")
                    {
                        "official BAM controller"
                    } else if app
                        .external_robot_run()
                        .is_some_and(|(_, r)| r == "Open Duck Mini v2")
                    {
                        "Open Duck MuJoCo position actuators"
                    } else {
                        "configured task controller"
                    };
                    out.push(txt(format!("Recorded deterministic policy test, using {controller}. Playback does not change training.")));
                    out.push(txt("Flat ground, 10-second trials. Tracking limits: 0.05 m/s and 0.20 rad/s RMSE; full survival required.".into()));
                    out.push(mono(v.report.to_string_lossy().into_owned()));
                } else {
                    out.push(txt(
                        "No completed validation is available for this run.".into()
                    ));
                }
                out.push(btns(vec![(
                    "Live training".into(),
                    "external-live".into(),
                    true,
                    false,
                )]));
                app_rmap_done(&mut out);
                return out;
            }
            // These four numbers were literals until this was wired: the panel
            // claimed a decimation comparison that nothing had computed, and
            // sat beside the real MuJoCo result under the same heading. An
            // unrun arm now says it has not run.
            out.push(cap(caps(&tt("Cross-sim · same physics"))));
            match app.cross.as_ref() {
                None => out.push(txt(tt("Not run. Start it from the left rail."))),
                Some(job) => {
                    let (label, done, gap, rows) = job.view();
                    out.push(mono(format!("{} · {}", tt("arm"), job.arm)));
                    if rows.is_empty() {
                        out.push(txt(tt("starting…")));
                    }
                    for (name, a, b, d) in &rows {
                        // Dashes while it runs: the report is zeroed until the
                        // arm returns, and 0.000 would read as a measurement.
                        if job.running() {
                            out.push(mono(format!(
                                "{}   {}: —   {}: —",
                                tt(name),
                                job.col_a,
                                job.col_b
                            )));
                        } else {
                            out.push(mono(format!(
                                "{}   {}: {a}   {}: {b}   Δ {d}",
                                tt(name),
                                job.col_a,
                                job.col_b
                            )));
                        }
                    }
                    if job.running() {
                        out.push(ver(
                            0.0,
                            tt("Running — both columns step the same equations."),
                        ));
                    } else if done {
                        out.push(ver(1.0, format!(
                            "{} {:.0}%. {}",
                            tt("Worst gap"),
                            gap * 100.0,
                            tt("Shares the dynamics function — catches integrator dependence, not modelling error.")
                        )));
                    } else {
                        out.push(ver(2.0, label));
                    }
                }
            }
            if st.demo {
                out.push(cap(caps(&tt("vs crouch-v2"))));
                if st.sweep_baseline {
                    let p = if st.sweep_grid.is_some() {
                        format!("{}%", st.sweep_pass())
                    } else {
                        "—".into()
                    };
                    out.push(mono(format!("{}   {p}   +9", tt("pass rate"))));
                    out.push(mono(format!("{}   .11   +.04", tt("low-friction edge"))));
                } else {
                    out.push(ver(2.0, tt("Baseline missing. The compared sweep was deleted — re-run crouch-v2 to compare.")));
                }
            }
        }
        Mode::Runs => {
            out.push(btns(vec![
                (
                    "Refresh files".into(),
                    "artifacts-refresh".into(),
                    true,
                    false,
                ),
                (
                    format!("Trash ({})", st.trash.len()),
                    "artifacts-trash".into(),
                    true,
                    false,
                ),
            ]));
            out.push(btns(vec![(
                "Discover saved runs".into(),
                "artifacts-import-runs".into(),
                true,
                false,
            )]));
            let Some(r) = st.sel.run.as_ref().and_then(|id| st.run(id)) else {
                out.push(txt(tt("none")));
                app_rmap_done(&mut out);
                return out;
            };
            if r.coll {
                out.push(cap("Retained policies".into()));
                out.push(txt("Protected checkpoints retain their original robot, training recipe and validation files.".into()));
                let page = app.checkpoint_page.min(r.ckpts.len().saturating_sub(1) / 2);
                out.push(btns(vec![
                    ("← Previous".into(), "ck-page-prev".into(), page > 0, false),
                    (
                        "Next →".into(),
                        "ck-page-next".into(),
                        (page + 1) * 2 < r.ckpts.len(),
                        false,
                    ),
                ]));
                for c in r.ckpts.iter().skip(page * 2).take(2) {
                    out.push(txt(format!(
                        "{} · {}",
                        c.label,
                        if c.st == CkSt::Prom {
                            "promoted"
                        } else {
                            "retained"
                        }
                    )));
                    out.push(btns(vec![
                        (
                            "Export ONNX…".into(),
                            format!("ck-onnx:{}", c.id),
                            app.export_job.is_none(),
                            false,
                        ),
                        (
                            "Inspect".into(),
                            format!("ck-inspect:{}", c.id),
                            true,
                            false,
                        ),
                    ]));
                    out.push(btns(vec![if c.st == CkSt::Prom {
                        ("Demote".into(), format!("ck-demote:{}", c.id), true, false)
                    } else {
                        (
                            "Delete checkpoint…".into(),
                            format!("ck-del:{}", c.id),
                            true,
                            true,
                        )
                    }]));
                }
                out.push(btns(vec![(
                    "Continue checkpoint…".into(),
                    "continue-run".into(),
                    app.real_proc.is_none() && crate::nexus::selected_artifact(st).is_some(),
                    false,
                )]));
                out.push(btns(vec![(
                    "Delete collection…".into(),
                    "run-delete".into(),
                    true,
                    true,
                )]));
                app_rmap_done(&mut out);
                return out;
            }
            out.push(cap(r.name.to_uppercase()));
            let best = r
                .best
                .map(|b| format!("{b:.3}"))
                .unwrap_or_else(|| "—".into());
            out.push(RSpec {
                kind: RKind::Tiles(vec![
                    (tt("set"), r.set.clone(), 0),
                    (tt("steps"), format!("{:.2}M", r.steps), 0),
                    (tt("best"), best, 0),
                ]),
            });
            if let Some(d) = &r.diagnosis {
                out.push(ver(2.0, format!("{}. {}", tt("Diagnosis"), tt(d))));
            }
            out.push(cap(caps(&tt("Snapshot"))));
            out.push(txt(tf(
                "{0} scenes · seed {1} — embedded at launch; edits to the library never touch this record.",
                &[&r.snapshot.scenes.to_string(), &r.snapshot.seed],
            )));
            if let Some(w) = &r.snapshot.warm {
                out.push(txt(format!("{}: {}", tt("Warm start"), tt(w))));
            }
            if !r.snapshot.cfg.is_empty() {
                out.push(mono(
                    r.snapshot
                        .cfg
                        .iter()
                        .map(|c| format!("{} {:.2}", c.name, c.stance))
                        .collect::<Vec<_>>()
                        .join(" · "),
                ));
            }
            out.push(cap(caps(&tt("Disposition"))));
            let mut b = vec![
                (
                    tt("Continue checkpoint…"),
                    "continue-run".into(),
                    app.real_proc.is_none() && crate::nexus::selected_artifact(st).is_some(),
                    false,
                ),
                (
                    tt("↻ re-run"),
                    "rerun".into(),
                    app.real_proc.is_none(),
                    false,
                ),
                (tt("open in Train"), "open-run".into(), true, false),
            ];
            if !r.archived {
                b.push((tt("archive"), "run-archive".into(), true, false));
            }
            b.push((tt("delete"), "run-delete".into(), true, true));
            for row in b.chunks(2) {
                out.push(btns(row.to_vec()));
            }
            out.push(btns(vec![(
                "Cleanup files…".into(),
                "artifacts-cleanup".into(),
                !matches!(r.state, RunState::Running | RunState::Paused),
                false,
            )]));
        }
        Mode::Deploy => {
            build_deploy_specs(app, &mut out);
        }
        Mode::Home => {}
    }
    app_rmap_done(&mut out);
    out
}

fn app_rmap_done(_out: &mut [RSpec]) {}

fn build_deploy_specs(app: &App, out: &mut Vec<RSpec>) {
    let st = &app.st;
    let lang = st.lang;
    let tt = |k: &str| tr(lang, k).to_string();
    let tf = |k: &str, a: &[&str]| trf(lang, k, a);
    let line_txt = |l: &Line| {
        trf(
            lang,
            l.k,
            &l.a.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
        )
    };
    if !st.demo {
        out.push(cap("Robot agent".into()));
        if let Some(target) = st.dg.target.as_ref().and_then(|id| st.target(id)) {
            out.push(txt(format!(
                "{}\n{}\n{}",
                target.name, target.model, target.ip
            )));
        } else {
            out.push(txt("Add a target for the selected robot. Local simulator targets run the full agent protocol with real physics.".into()));
        }
        out.push(btns(vec![
            (
                "Add target…".into(),
                "target-new".into(),
                app.deployment.session.is_none(),
                false,
            ),
            (
                "Evidence files".into(),
                "agent-evidence".into(),
                true,
                false,
            ),
        ]));
        let Some(session) = &app.deployment.session else {
            out.push(btns(vec![(
                "Connect agent".into(),
                "agent-connect".into(),
                st.dg.target.is_some(),
                false,
            )]));
            out.push(txt("Remote: run tools/deploy/agent.py on the target and forward its localhost port through SSH. Set the endpoint and token file in the target editor.".into()));
            return;
        };
        let status = &session.status;
        let agent = &status["agent"];
        out.push(ver(
            if agent["simulation"] == true {
                1.0
            } else {
                2.0
            },
            if agent["simulation"] == true {
                "SIMULATOR · no hardware connected".into()
            } else {
                "ROBOT AGENT · physical validation unverified".into()
            },
        ));
        out.push(txt(format!(
            "Connection: {}\nTarget: {}",
            status["state"].as_str().unwrap_or("connecting"),
            session.agent_state()
        )));
        if let Some(error) = status["error"].as_str().or_else(|| agent["error"].as_str()) {
            out.push(ver(2.0, error.into()));
        }
        if let Some(error) = status["stop_error"].as_str() {
            out.push(ver(2.0, format!("Stop unconfirmed: {error}")));
        }
        let connected = status["state"] == "connected";
        let active = session.agent_state() == "armed";
        let ready = connected && !session.busy();
        out.push(btns(vec![
            ("Stop target".into(), "agent-stop".into(), true, true),
            ("Disconnect".into(), "agent-disconnect".into(), true, false),
        ]));
        out.push(btns(vec![(
            "Prepare checkpoint".into(),
            "agent-prepare".into(),
            ready && !active && crate::nexus::selected_artifact(st).is_some(),
            false,
        )]));
        if let Some(candidate) = agent["candidate"].as_object() {
            out.push(cap("Captured candidate".into()));
            let hash = candidate
                .get("checkpoint_sha256")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            out.push(txt(format!(
                "SHA-256 {}…\n{} observations · {} actions",
                hash.chars().take(16).collect::<String>(),
                agent["candidate"]["observations"],
                agent["candidate"]["actions"]
            )));
            out.push(btns(vec![
                (
                    "Dry-run".into(),
                    "agent-dry-run".into(),
                    ready && !active,
                    false,
                ),
                (
                    "Arm…".into(),
                    "agent-arm".into(),
                    ready && session.agent_state() == "verified",
                    true,
                ),
            ]));
        }
        if let Some(source) = agent["dry_run"]["source"].as_str() {
            out.push(txt(format!("Dry-run: {source}")));
        }
        if active {
            out.push(cap(format!(
                "Stage {} / 3 · {:.0}% command limits",
                agent["stage"],
                agent["command_fraction"].as_f64().unwrap_or(0.) * 100.
            )));
            out.push(txt(format!(
                "{:.1}s · {} control steps\nCommand x / y / yaw:\n{}\nWatchdog: {:.1}s",
                agent["dwell_seconds"].as_f64().unwrap_or(0.),
                agent["steps"],
                crate::evaluation::metric_vector(&agent["command"]),
                agent["watchdog_seconds"].as_f64().unwrap_or(0.)
            )));
            out.push(btns(vec![
                (
                    "Next stage".into(),
                    "agent-advance".into(),
                    ready
                        && agent["stage"].as_u64().unwrap_or(3) < 3
                        && agent["dwell_seconds"].as_f64().unwrap_or(0.) >= 3.
                        && agent["steps"].as_u64().unwrap_or(0) >= 50,
                    false,
                ),
                ("Velocity…".into(), "agent-command".into(), ready, false),
            ]));
        }
        out.push(btns(vec![(
            "Roll back (disarmed)".into(),
            "agent-rollback".into(),
            ready,
            false,
        )]));
        out.push(txt("Arming, commands, stop reasons and measured telemetry are retained in the session evidence folder. Command stages do not change motor torque calibration.".into()));
        return;
    }
    // Record view
    if let Some(dep) = st.sel.dep.as_ref().and_then(|id| st.deploy(id)) {
        out.push(capb(
            caps(&tt("Deployment record")),
            tt("← back"),
            "dep-back",
        ));
        let tone = match dep.state {
            DepState::Live => 0.0,
            DepState::Aborted => 3.0,
            DepState::RolledBack => 2.0,
            _ => 4.0,
        };
        out.push(RSpec {
            kind: RKind::Row {
                nm: dep.name.clone(),
                tg: String::new(),
                pill: Some((tt(dep.state.key()), tone)),
                on: false,
                click: String::new(),
            },
        });
        out.push(RSpec {
            kind: RKind::Tiles(vec![
                (tt("policy"), dep.pname.clone(), 0),
                (tt("target"), dep.target.clone(), 0),
            ]),
        });
        out.push(RSpec {
            kind: RKind::Tiles(vec![
                (tt("artifact"), dep.hash.clone(), 0),
                (tt("reached"), format!("{}%", [40, 70, 100][dep.stage]), 0),
            ]),
        });
        out.push(mono(tf("target rev {0}", &[&dep.tg_rev.to_string()])));
        if let Some(n) = &dep.note {
            out.push(ver(1.0, line_txt(n)));
        }
        if let Some(tg) = &dep.tg_snap {
            out.push(cap(caps(&tt("Target snapshot"))));
            out.push(mono(format!(
                "{} · {} · {} · {} · {} {}%",
                tg.model,
                tg.ip,
                tg.iface,
                tg.sdk,
                tt("cap"),
                tg.cap
            )));
        }
        out.push(cap(caps(&tt("Gate report"))));
        for g in &dep.gate_rep {
            out.push(mono(format!("✓ {}", line_txt(g))));
        }
        out.push(cap(caps(&tt("Checklist"))));
        let labels = [
            "Area clear of people",
            "Harness attached",
            "E-stop within reach",
            "Battery above 30%",
        ];
        for (i, lb) in labels.iter().enumerate() {
            out.push(mono(format!(
                "{} {}",
                if dep.chk[i] { "✓" } else { "✕" },
                tt(lb)
            )));
        }
        out.push(mono(tf("signed · {0} · {1}", &[&dep.signer, &dep.ts])));
        out.push(cap(caps(&tt("Events"))));
        for e in &dep.events {
            out.push(mono(format!("· {}", line_txt(e))));
        }
        out.push(txt(tt("Artifact, gate report, checklist and target state were embedded at arm time — this record does not change when the library does.")));
        out.push(cap(caps(&tt("Disposition"))));
        let mut b = vec![];
        if dep.state == DepState::Live {
            b.push((tt("retire"), "dep-retire".into(), true, false));
            b.push((tt("E-STOP"), "dep-estop-rec".into(), true, true));
        }
        b.push((tt("load as candidate"), "dep-recert".into(), true, false));
        out.push(btns(b));
        if dep.state == DepState::Live {
            out.push(txt(tt(
                "software stop — the pendant E-stop stays authoritative",
            )));
        }
        return;
    }
    // Rollout console
    if let Some(live) = st.dg.armed.as_ref().and_then(|id| st.deploy(id)) {
        out.push(cap(format!("{} · {}", caps(&tt("Rollout")), live.name)));
        let mut segs = vec![];
        for i in 0..3 {
            segs.push((
                1.0,
                if i < live.stage {
                    SegTone::Ok
                } else if i == live.stage {
                    SegTone::Vio
                } else {
                    SegTone::Sink
                },
            ));
        }
        out.push(RSpec {
            kind: RKind::Segs(segs),
        });
        out.push(mono("40% · 70% · 100%".into()));
        let cap_v = [40, 70, 100][live.stage];
        out.push(RSpec {
            kind: RKind::Tiles(vec![
                (tt("torque cap"), format!("{cap_v}%"), 1),
                (tt("peak torque"), format!("{}%", st.dg_live.torq), 2),
                (tt("height err"), format!("{:.3}", st.dg_live.herr), 0),
            ]),
        });
        out.push(ver(0.0, tt("Watching. Advance only after the peak torque settles well under the cap — headroom is the safety margin.")));
        let adv = if live.stage < 2 {
            Some((
                tf(
                    "advance to {0}%",
                    &[&[40, 70, 100][live.stage + 1].to_string()],
                ),
                "stage-adv".to_string(),
                st.dg_live.unlocked,
            ))
        } else {
            Some((
                tt("rollout complete — keep live"),
                "dep-done".to_string(),
                true,
            ))
        };
        let rollback_en = st
            .deploys
            .iter()
            .any(|d| d.state == DepState::Superseded && d.tg_id == live.tg_id);
        out.push(RSpec {
            kind: RKind::Estop { adv, rollback_en },
        });
        if !st.dg_live.unlocked && live.stage < 2 {
            out.push(mono(tf(
                "settling · {0}/4 healthy s",
                &[&st.dg_live.dwell.min(4).to_string()],
            )));
        }
        out.push(txt(format!(
            "{} · {}",
            tt("E-stop drops the robot to damping — the record is kept as aborted, gates stay valid for re-arm."),
            tt("software stop — the pendant E-stop stays authoritative")
        )));
        return;
    }
    // Candidate + gates
    let tg = st.dg.target.as_ref().and_then(|t| st.target(t));
    out.push(capb(caps(&tt("Candidate")), tt("reset"), "dep-reset"));
    let policy = match (&st.dg.pname, &st.dg.ck) {
        (Some(p), Some(c)) => format!("{p} · {c}"),
        _ => tt("— pick from the left rail"),
    };
    out.push(RSpec {
        kind: RKind::Tiles(vec![
            (tt("policy"), policy, 0),
            (
                tt("target"),
                tg.map(|t| t.name.clone()).unwrap_or_else(|| "—".into()),
                0,
            ),
        ]),
    });
    out.push(capb(
        caps(&tt("Gates · in order")),
        tt("simulated evidence"),
        "",
    ));
    let ping_ok = tg.map(|t| matches!(t.ping, Ping::Ok(_))).unwrap_or(false);
    out.push(gate_line(
        st,
        GateId::Export,
        tt("1 · Export"),
        tt("ONNX for the sdk2 controller — TorchScript available"),
        st.dg.ck.is_none(),
        tt("pick a policy first"),
    ));
    out.push(gate_line(
        st,
        GateId::Compat,
        tt("2 · Compatibility"),
        tf(
            "five checks against {0}",
            &[&tg.map(|t| t.model.clone()).unwrap_or_default()],
        ),
        !st.dg.export.ok(),
        tt("export first"),
    ));
    out.push(gate_line(
        st,
        GateId::Sim2sim,
        tt("3 · Sim-to-sim"),
        tt("MuJoCo replica — catches export drift, not modelling error"),
        !st.dg.compat.ok(),
        tt("compatibility first"),
    ));
    let dr_reason = if !st.dg.sim2sim.ok() {
        tt("sim-to-sim first")
    } else {
        tt("target unreachable — ping it first")
    };
    out.push(gate_line(
        st,
        GateId::Dryrun,
        tt("4 · Dry-run"),
        tt("actions computed on the real robot, not applied — needs a reachable target"),
        !st.dg.sim2sim.ok() || !ping_ok,
        dr_reason,
    ));
    out.push(cap(caps(&tt("Safety checklist · a human signs"))));
    let labels = [
        "Area clear of people",
        "Harness attached",
        "E-stop within reach",
        "Battery above 30%",
    ];
    for (i, lb) in labels.iter().enumerate() {
        out.push(RSpec {
            kind: RKind::Cbx {
                lb: tt(lb),
                idx: i,
                val: st.dg.chk[i],
            },
        });
    }
    let missing = st.arm_missing();
    out.push(btns(vec![(
        tt("Arm rollout →"),
        "dep-arm".into(),
        missing.is_none(),
        false,
    )]));
    if let Some(m) = missing {
        out.push(mono(format!("{}", tf("blocked: {0}", &[&tt(m)]))));
    }
    out.push(cap(format!(
        "{} · {}",
        caps(&tt("History")),
        st.deploys.len()
    )));
    for d in &st.deploys {
        let tone = match d.state {
            DepState::Live => 0.0,
            DepState::Aborted => 3.0,
            DepState::RolledBack => 2.0,
            _ => 4.0,
        };
        out.push(RSpec {
            kind: RKind::Row {
                nm: d.name.clone(),
                tg: format!("{} → {}", d.pname, d.target),
                pill: Some((tt(d.state.key()), tone)),
                on: false,
                click: format!("sel-dep:{}", d.id),
            },
        });
    }
}

fn rline_paths() -> [&'static [LiveId]; 40] {
    [
        ids!(rr.r0),
        ids!(rr.r1),
        ids!(rr.r2),
        ids!(rr.r3),
        ids!(rr.r4),
        ids!(rr.r5),
        ids!(rr.r6),
        ids!(rr.r7),
        ids!(rr.r8),
        ids!(rr.r9),
        ids!(rr.r10),
        ids!(rr.r11),
        ids!(rr.r12),
        ids!(rr.r13),
        ids!(rr.r14),
        ids!(rr.r15),
        ids!(rr.r16),
        ids!(rr.r17),
        ids!(rr.r18),
        ids!(rr.r19),
        ids!(rr.r20),
        ids!(rr.r21),
        ids!(rr.r22),
        ids!(rr.r23),
        ids!(rr.r24),
        ids!(rr.r25),
        ids!(rr.r26),
        ids!(rr.r27),
        ids!(rr.r28),
        ids!(rr.r29),
        ids!(rr.r30),
        ids!(rr.r31),
        ids!(rr.r32),
        ids!(rr.r33),
        ids!(rr.r34),
        ids!(rr.r35),
        ids!(rr.r36),
        ids!(rr.r37),
        ids!(rr.r38),
        ids!(rr.r39),
    ]
}

pub fn sync_right_rail(app: &mut App, cx: &mut Cx) {
    let l = app.light();
    let specs = build_right_specs(app);
    app.rmap = specs.clone();
    let rail = app.ui.widget(cx, ids!(rail_r));
    if rail.is_empty() {
        return;
    }
    for (i, path) in rline_paths().iter().enumerate() {
        let line = rail.widget(cx, path);
        if line.is_empty() {
            continue;
        }
        let Some(spec) = specs.get(i) else {
            line.set_visible(cx, false);
            continue;
        };
        line.set_visible(cx, true);
        // hide all variants first
        for vp in [
            ids!(tiles),
            ids!(chk),
            ids!(sld),
            ids!(ver),
            ids!(row),
            ids!(btns),
            ids!(cap),
            ids!(cbx),
            ids!(sparkw),
            ids!(segw),
            ids!(hot),
        ] {
            let v = line.view(cx, vp);
            if !v.is_empty() {
                v.set_visible(cx, false);
            }
        }
        let txtl = line.label(cx, ids!(txt));
        if !txtl.is_empty() {
            let w = line.view(cx, ids!(txt));
            let _ = w;
        }
        line.widget(cx, ids!(txt)).set_visible(cx, false);
        match &spec.kind {
            RKind::Cap { text, btn } => {
                let capv = line.view(cx, ids!(cap));
                capv.set_visible(cx, true);
                set_label(cx, &line, ids!(cap.cl), text, l);
                match btn {
                    Some((bl, act)) => {
                        let enabled = !act.is_empty();
                        set_btn(cx, &line, ids!(cap.cb), bl, true, false, enabled, l);
                    }
                    None => {
                        let b = line.button(cx, ids!(cap.cb));
                        if !b.is_empty() {
                            b.set_visible(cx, false);
                        }
                    }
                }
            }
            RKind::Tiles(ts) => {
                let tv = line.view(cx, ids!(tiles));
                tv.set_visible(cx, true);
                let tpaths = [ids!(tiles.t0), ids!(tiles.t1), ids!(tiles.t2)];
                for (j, tp) in tpaths.iter().enumerate() {
                    let tile = line.widget(cx, *tp);
                    if tile.is_empty() {
                        continue;
                    }
                    match ts.get(j) {
                        Some((k, v, accent)) => {
                            tile.set_visible(cx, true);
                            set_label(cx, &tile, ids!(k), &caps(k), l);
                            set_label(cx, &tile, ids!(v), v, l);
                            let mut tw = tile.widget(cx, ids!(v));
                            let af = *accent as f64;
                            script_apply_eval!(cx, tw, { draw_text +: { accent: #(af) } });
                            let mut bg = tile.clone();
                            script_apply_eval!(cx, bg, { draw_bg +: { light: #(l as f64) } });
                        }
                        None => tile.set_visible(cx, false),
                    }
                }
            }
            RKind::Chk {
                st,
                nm,
                ds,
                act,
                cancel,
            } => {
                let cv = line.view(cx, ids!(chk));
                cv.set_visible(cx, true);
                let ic = match *st as i32 {
                    0 => "✓",
                    1 => "…",
                    2 => "✕",
                    _ => "·",
                };
                set_label(cx, &line, ids!(chk.ico), ic, l);
                {
                    let mut iw = line.widget(cx, ids!(chk.ico));
                    let stf = *st;
                    script_apply_eval!(cx, iw, { draw_text +: { st: #(stf) } });
                }
                set_label(cx, &line, ids!(chk.body.nm), nm, l);
                set_label(cx, &line, ids!(chk.body.ds), ds, l);
                match act {
                    Some((label, _actid, enabled, _reason)) => {
                        set_btn(cx, &line, ids!(chk.act), label, true, false, *enabled, l)
                    }
                    None => {
                        let b = line.button(cx, ids!(chk.act));
                        if !b.is_empty() {
                            b.set_visible(cx, false);
                        }
                    }
                }
                match cancel {
                    Some(_) => set_btn(
                        cx,
                        &line,
                        ids!(chk.act2),
                        &t(app.st.lang, "cancel"),
                        true,
                        false,
                        true,
                        l,
                    ),
                    None => {
                        let b = line.button(cx, ids!(chk.act2));
                        if !b.is_empty() {
                            b.set_visible(cx, false);
                        }
                    }
                }
            }
            RKind::Sld {
                lb,
                val,
                min,
                max,
                fmt,
                ..
            } => {
                let sv = line.view(cx, ids!(sld));
                sv.set_visible(cx, true);
                set_label(cx, &line, ids!(sld.lb), lb, l);
                set_label(cx, &line, ids!(sld.val), fmt, l);
                let s = line.slider(cx, ids!(sld.sl));
                if !s.is_empty() {
                    let norm = ((val - min) / (max - min)).clamp(0.0, 1.0);
                    s.set_value(cx, norm);
                }
            }
            RKind::Txt(tx) => {
                line.widget(cx, ids!(txt)).set_visible(cx, true);
                set_label(cx, &line, ids!(txt), tx, l);
            }
            RKind::Mono(tx) => {
                line.widget(cx, ids!(txt)).set_visible(cx, true);
                set_label(cx, &line, ids!(txt), tx, l);
            }
            RKind::Ver { tone, text } => {
                let vv = line.view(cx, ids!(ver));
                vv.set_visible(cx, true);
                set_label(cx, &line, ids!(ver.lbl), text, l);
                let mut vw = line.widget(cx, ids!(ver));
                let tf2 = *tone;
                script_apply_eval!(cx, vw, { draw_bg +: { tone: #(tf2) light: #(l as f64) } });
            }
            RKind::Row {
                nm, tg, pill, on, ..
            } => {
                let rv = line.view(cx, ids!(row));
                rv.set_visible(cx, true);
                set_label(cx, &line, ids!(row.nm), nm, l);
                set_label(cx, &line, ids!(row.tg), tg, l);
                line.widget(cx, ids!(row.tg))
                    .set_visible(cx, !tg.is_empty());
                let onf = if *on { 1.0 } else { 0.0 };
                let mut rw = line.widget(cx, ids!(row));
                script_apply_eval!(cx, rw, { draw_bg +: { on: #(onf) light: #(l as f64) } });
                let rowref = line.widget(cx, ids!(row));
                match pill {
                    Some((pt, tone)) => set_pill(cx, &rowref, pt, *tone, l),
                    None => {
                        let pw = rowref.widget(cx, ids!(pill));
                        if !pw.is_empty() {
                            pw.set_visible(cx, false);
                        }
                    }
                }
            }
            RKind::Btns(bs) => {
                let bv = line.view(cx, ids!(btns));
                bv.set_visible(cx, true);
                let bpaths = [ids!(btns.b0), ids!(btns.b1), ids!(btns.b2), ids!(btns.b3)];
                // hot buttons go to b3 slot when flagged
                let mut cold: Vec<&(String, String, bool, bool)> =
                    bs.iter().filter(|b| !b.3).collect();
                let hot: Option<&(String, String, bool, bool)> = bs.iter().find(|b| b.3);
                cold.truncate(3);
                for (j, bp) in bpaths.iter().enumerate().take(3) {
                    match cold.get(j) {
                        Some((label, _act, enabled, _)) => {
                            set_btn(cx, &line, *bp, label, true, false, *enabled, l)
                        }
                        None => {
                            let b = line.button(cx, *bp);
                            if !b.is_empty() {
                                b.set_visible(cx, false);
                            }
                        }
                    }
                }
                match hot {
                    Some((label, _act, enabled, _)) => {
                        set_btn(cx, &line, ids!(btns.b3), label, true, false, *enabled, l)
                    }
                    None => {
                        let b = line.button(cx, ids!(btns.b3));
                        if !b.is_empty() {
                            b.set_visible(cx, false);
                        }
                    }
                }
            }
            RKind::Cbx { lb, val, .. } => {
                let cv = line.view(cx, ids!(cbx));
                cv.set_visible(cx, true);
                set_label(cx, &line, ids!(cbx.cl2), lb, l);
                let cb = line.check_box(cx, ids!(cbx.cbox));
                if !cb.is_empty() {
                    cb.set_active(cx, *val, Animate::No);
                }
            }
            RKind::SparkLine { data, goal, tone } => {
                let sw = line.view(cx, ids!(sparkw));
                sw.set_visible(cx, true);
                let sp = line.spark(cx, ids!(sparkw.spark));
                sp.set(cx, data, *goal, *tone, l);
            }
            RKind::Segs(segs) => {
                let sw = line.view(cx, ids!(segw));
                sw.set_visible(cx, true);
                let sg = line.segsbar(cx, ids!(segw.seg));
                sg.set(cx, segs.clone(), l);
            }
            RKind::Estop { adv, rollback_en } => {
                let hv = line.view(cx, ids!(hot));
                hv.set_visible(cx, true);
                match adv {
                    Some((label, _act, enabled)) => {
                        set_btn(cx, &line, ids!(hot.h0), label, true, false, *enabled, l)
                    }
                    None => {
                        let b = line.button(cx, ids!(hot.h0));
                        if !b.is_empty() {
                            b.set_visible(cx, false);
                        }
                    }
                }
                set_btn(
                    cx,
                    &line,
                    ids!(hot.h1),
                    &t(app.st.lang, "E-STOP"),
                    true,
                    false,
                    true,
                    l,
                );
                set_btn(
                    cx,
                    &line,
                    ids!(hot.h2),
                    &t(app.st.lang, "rollback"),
                    true,
                    false,
                    *rollback_en,
                    l,
                );
            }
        }
    }
}

pub fn route_right_rail(app: &mut App, cx: &mut Cx, actions: &Actions) {
    let rail = app.ui.widget(cx, ids!(rail_r));
    if rail.is_empty() {
        return;
    }
    let specs = app.rmap.clone();
    for (i, path) in rline_paths().iter().enumerate() {
        let Some(spec) = specs.get(i) else { continue };
        let line = rail.widget(cx, path);
        if line.is_empty() {
            continue;
        }
        match &spec.kind {
            RKind::Cap {
                btn: Some((_, act)),
                ..
            } => {
                if !act.is_empty() && line.button(cx, ids!(cap.cb)).clicked(actions) {
                    app.dispatch_named(cx, act, "");
                    return;
                }
            }
            RKind::Chk { act, cancel, .. } => {
                if let Some((_, actid, _, _)) = act {
                    if line.button(cx, ids!(chk.act)).clicked(actions) {
                        app.dispatch_named(cx, actid, "");
                        return;
                    }
                }
                if let Some(c) = cancel {
                    if line.button(cx, ids!(chk.act2)).clicked(actions) {
                        app.dispatch_named(cx, c, "");
                        return;
                    }
                }
            }
            RKind::Sld {
                field, min, max, ..
            } => {
                let s = line.slider(cx, ids!(sld.sl));
                if s.is_empty() {
                    continue;
                }
                if let Some(norm) = s.slided(actions) {
                    let v = min + norm.clamp(0.0, 1.0) * (max - min);
                    app.slider_changed(cx, field, v);
                    return;
                }
                let uid = line.widget(cx, ids!(sld.sl)).widget_uid();
                for a in actions.filter_widget_actions(uid) {
                    if let SliderAction::EndSlide(norm) = a.cast::<SliderAction>() {
                        let v = min + norm.clamp(0.0, 1.0) * (max - min);
                        app.slider_committed(cx, field, v);
                        return;
                    }
                }
            }
            RKind::Row { click, .. } => {
                if !click.is_empty() {
                    let uid = line.widget(cx, ids!(row)).widget_uid();
                    if view_clicked(actions, uid) {
                        app.dispatch_arg(cx, click);
                        return;
                    }
                }
            }
            RKind::Btns(bs) => {
                let bpaths = [ids!(btns.b0), ids!(btns.b1), ids!(btns.b2)];
                let cold: Vec<&(String, String, bool, bool)> = bs.iter().filter(|b| !b.3).collect();
                let hot = bs.iter().find(|b| b.3);
                for (j, bp) in bpaths.iter().enumerate() {
                    if let Some((_, act, _, _)) = cold.get(j) {
                        if line.button(cx, *bp).clicked(actions) {
                            app.dispatch_arg(cx, act);
                            return;
                        }
                    }
                }
                if let Some((_, act, _, _)) = hot {
                    if line.button(cx, ids!(btns.b3)).clicked(actions) {
                        app.dispatch_arg(cx, act);
                        return;
                    }
                }
            }
            RKind::Cbx { idx, .. } => {
                let cb = line.check_box(cx, ids!(cbx.cbox));
                if !cb.is_empty() {
                    if let Some(v) = cb.changed(actions) {
                        app.st.dep_chk(*idx, v);
                        app.sync_all(cx);
                        return;
                    }
                }
            }
            RKind::Estop { adv, .. } => {
                if let Some((_, act, _)) = adv {
                    if line.button(cx, ids!(hot.h0)).clicked(actions) {
                        app.dispatch_named(cx, act, "");
                        return;
                    }
                }
                if line.button(cx, ids!(hot.h1)).clicked(actions) {
                    app.dispatch_named(cx, "dep-estop", "");
                    return;
                }
                if line.button(cx, ids!(hot.h2)).clicked(actions) {
                    app.dispatch_named(cx, "dep-rollback", "");
                    return;
                }
            }
            _ => {}
        }
    }
}
