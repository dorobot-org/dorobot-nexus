//! Right rail: per-mode content built as a list of RSpec lines rendered into
//! the 28 generic RLine slots, mirroring the mockup's renderR exactly.

use super::*;
use crate::kit::*;

#[derive(Clone, Debug)]
pub enum RKind {
    Cap { text: String, btn: Option<(String, String)> }, // caption + optional (label, act)
    Tiles(Vec<(String, String, u8)>),                    // (k, v, accent 0/1/2)
    Chk { st: f64, nm: String, ds: String, act: Option<(String, String, bool, String)>, cancel: Option<String> },
    Sld { lb: String, field: String, min: f64, max: f64, val: f64, fmt: String },
    Txt(String),
    Mono(String),
    Ver { tone: f64, text: String },
    Row { nm: String, tg: String, pill: Option<(String, f64)>, on: bool, click: String },
    Btns(Vec<(String, String, bool, bool)>), // label, act, enabled, hot
    Cbx { lb: String, idx: usize, val: bool },
    SparkLine { data: Vec<f64>, goal: Option<f64>, tone: u8 },
    Segs(Vec<(f64, SegTone)>),
    Estop { adv: Option<(String, String, bool)>, rollback_en: bool },
}

#[derive(Clone, Debug)]
pub struct RSpec {
    pub kind: RKind,
}

impl Default for RSpec {
    fn default() -> Self {
        RSpec { kind: RKind::Txt(String::new()) }
    }
}

fn cap(text: String) -> RSpec {
    RSpec { kind: RKind::Cap { text, btn: None } }
}
fn capb(text: String, blabel: String, act: &str) -> RSpec {
    RSpec { kind: RKind::Cap { text, btn: Some((blabel, act.into())) } }
}
fn txt(t: String) -> RSpec {
    RSpec { kind: RKind::Txt(t) }
}
fn mono(t: String) -> RSpec {
    RSpec { kind: RKind::Mono(t) }
}
fn ver(tone: f64, text: String) -> RSpec {
    RSpec { kind: RKind::Ver { tone, text } }
}
fn btns(b: Vec<(String, String, bool, bool)>) -> RSpec {
    RSpec { kind: RKind::Btns(b) }
}

fn gate_line(st: &Store, id: GateId, label: String, detail: String, dis: bool, reason: String) -> RSpec {
    let g = st.dg.gate(id);
    let lang = st.lang;
    let (gst, ds, act, cancel) = match g {
        Gate::NotRun => (
            3.0,
            format!("{detail} · {}", tr(lang, "not run")),
            Some((tr(lang, "run").to_string(), gate_act(id, false), !dis, reason)),
            None,
        ),
        Gate::Running(p) => (1.0, running_detail(st, id, *p), None, Some(gate_act(id, true))),
        Gate::Pass { .. } => (0.0, pass_detail(st, id), None, None),
        Gate::Fail { why } => (
            2.0,
            trf(lang, why.k, &why.a.iter().map(|s| s.as_str()).collect::<Vec<_>>()),
            Some((tr(lang, "re-run").to_string(), gate_act(id, false), !dis, reason)),
            None,
        ),
    };
    RSpec { kind: RKind::Chk { st: gst, nm: label, ds, act, cancel } }
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
            let dirty_chip = if st.dirty { format!(" · {}", tt("● unsaved")) } else { String::new() };
            out.push(cap(format!("{} · {}{}", caps(&tt("Editor")), sc.name, dirty_chip)));
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
            out.push(RSpec { kind: RKind::Sld { lb: tt("relief"), field: "amp".into(), min: 0.25, max: 3.0, val: sc.amp, fmt: fmt_field(sc.amp, "amp", lang) } });
            out.push(RSpec { kind: RKind::Sld { lb: tt("grade"), field: "slope".into(), min: 0.0, max: 30.0, val: sc.slope, fmt: fmt_field(sc.slope, "slope", lang) } });
            out.push(mono(format!("{} {}", tt("seed"), sc.seed)));
            out.push(cap(caps(&tt("Goal & command"))));
            out.push(RSpec { kind: RKind::Sld { lb: tt("stance"), field: "stance".into(), min: 0.45, max: 0.84, val: sc.stance, fmt: fmt_field(sc.stance, "stance", lang) } });
            out.push(RSpec { kind: RKind::Sld { lb: "vx".into(), field: "vx".into(), min: -0.6, max: 0.6, val: sc.vx, fmt: fmt_field(sc.vx, "vx", lang) } });
            out.push(RSpec { kind: RKind::Sld { lb: tt("length"), field: "dur".into(), min: 2.0, max: 20.0, val: sc.dur, fmt: fmt_field(sc.dur, "dur", lang) } });
            out.push(cap(caps(&tt("Fixed for the whole run"))));
            out.push(RSpec { kind: RKind::Sld { lb: tt("friction"), field: "friction".into(), min: 0.15, max: 1.5, val: sc.friction, fmt: fmt_field(sc.friction, "friction", lang) } });
            out.push(RSpec { kind: RKind::Sld { lb: tt("PD gain"), field: "kp".into(), min: 0.4, max: 1.6, val: sc.kp, fmt: fmt_field(sc.kp, "kp", lang) } });
            out.push(RSpec { kind: RKind::Sld { lb: tt("mass"), field: "mass".into(), min: 0.5, max: 2.0, val: sc.mass, fmt: fmt_field(sc.mass, "mass", lang) } });
            out.push(RSpec { kind: RKind::Sld { lb: tt("push"), field: "push".into(), min: 0.0, max: 1.5, val: sc.push, fmt: fmt_field(sc.push, "push", lang) } });
            out.push(cap(caps(&tt("Varied per environment · by the trainer"))));
            out.push(btns(vec![
                (tt("force"), "force-fam".into(), true, sc.force_fam),
                (tt("all 4"), "force-fam".into(), true, !sc.force_fam),
                (format!("{}: {}", tt("spawn state"), if sc.spawn_dr { tt("on") } else { tt("off") }), "spawn".into(), true, sc.spawn_dr),
            ]));
            out.push(txt(tt("difficulty") + " · " + &tt("adaptive · +1 after 4 wins")));
            out.push(txt(tt("Up to 4096 envs in parallel (trainer capacity; budgets use this machine’s measured 512-env throughput). These vary between them — you set whether, not what.")));
        }
        Mode::Robots => {
            match st.sel.robot.as_ref().and_then(|id| st.robots.iter().find(|r| &r.id == id)) {
                None => out.push(txt(tt("none"))),
                Some(r) => {
                    out.push(cap(r.name.to_uppercase()));
                    out.push(RSpec { kind: RKind::Tiles(vec![
                        (tt("links"), r.links.to_string(), 0),
                        (tt("joints"), r.joints.to_string(), 0),
                        (tt("movable"), r.movable.to_string(), 0),
                    ]) });
                    out.push(RSpec { kind: RKind::Tiles(vec![
                        (tt("mapped"), r.mapped.to_string(), 0),
                        (tt("meshes"), r.meshes.to_string(), 0),
                    ]) });
                    out.push(cap(caps(&tt("Validation"))));
                    out.push(RSpec { kind: RKind::Chk { st: 0.0, nm: tt("Gate passed on import"), ds: tt("1 note: floating_base_joint commented out — excluded, as the parser does"), act: None, cancel: None } });
                    out.push(cap(caps(&tt("Used by"))));
                    out.push(txt(r.used_by.iter().map(|u| tt(u)).collect::<Vec<_>>().join(", ")));
                    out.push(cap(caps(&tt("Actions"))));
                    out.push(btns(vec![
                        (tt("rename"), "robot-rename".into(), true, false),
                        (tt("remap joints"), "robot-remap".into(), true, false),
                        (tt("remove"), "robot-remove".into(), true, false),
                    ]));
                    out.push(txt(tt("The URDF itself is not editable here — it is an upstream artifact. Re-import supersedes.")));
                }
            }
        }
        Mode::Train => {
            let Some(r) = st.sel.run.as_ref().and_then(|id| st.run(id)) else {
                out.push(txt(tt("none")));
                app_rmap_done(&mut out);
                return out;
            };
            out.push(cap(caps(&tt("Diagnosis"))));
            let (tone, vtext) = match r.state {
                RunState::Failed => (2.0, tf("Failed. {0}", &[&tt(r.diagnosis.as_deref().unwrap_or(""))])),
                RunState::Completed => (0.0, tt("Completed. Budget reached; the record is immutable.")),
                RunState::Stopped => (1.0, tt("Stopped early. Partial checkpoints kept; the record is immutable.")),
                RunState::Paused => (1.0, tt("Paused. Resuming re-does up to 50 iterations from the last checkpoint.")),
                RunState::Running => (0.0, tt("Learning. Total falling while 4 of 5 behaviour terms improve — the drop is the termination penalty, not a regression.")),
            };
            let vtext = if r.archived { format!("{vtext} · {}", tt("archived")) } else { vtext };
            out.push(ver(tone, vtext));
            out.push(cap(caps(&tt("Checkpoints"))));
            out.push(txt(tt("retention: keep last 4 + best + promoted · prune shows its list first")));
            if r.ckpts.is_empty() {
                out.push(txt(tt("none yet")));
            }
            let disk = crate::nexus::real_ckpts();
            if !disk.is_empty() {
                out.push(cap(format!("{} · {}", caps(&tt("On disk")), disk.len())));
                for c in disk.iter().take(5) {
                    out.push(mono(format!("{} · {} KB", c.label, c.kb)));
                }
            }
            if !app.real_lines.is_empty() {
                out.push(cap(caps(&tt("Real run · disk"))));
                for line in app.real_lines.iter().rev().take(4).rev() {
                    out.push(mono(line.clone()));
                }
            }
            for c in &r.ckpts {
                let pill = match c.st {
                    CkSt::Best => Some((tt("best"), 1.0)),
                    CkSt::Prom => Some((format!("★ {}", c.pname.clone().unwrap_or_default()), 0.0)),
                    CkSt::Ord => None,
                };
                out.push(RSpec { kind: RKind::Row { nm: c.label.clone(), tg: format!("{:.3}", c.score), pill, on: false, click: String::new() } });
                let mut b = vec![];
                if c.st == CkSt::Prom {
                    b.push((tt("Demote"), format!("ck-demote:{}", c.id), true, false));
                    b.push((tt("Deploy →"), format!("ck-deploy:{}", c.id), true, false));
                } else {
                    b.push((tt("Promote"), format!("ck-promote:{}", c.id), true, false));
                }
                b.push((tt("Export ONNX"), format!("ck-export:{}", c.id), true, false));
                b.push((tt("Inspect"), format!("ck-inspect:{}", c.label), true, false));
                if c.st != CkSt::Prom {
                    b.push((tt("Delete"), format!("ck-del:{}", c.id), true, true));
                }
                out.push(btns(b));
            }
        }
        Mode::Inspect => {
            out.push(cap(caps(&tt("Observation · 480-dim (96 × 5-step history)"))));
            out.push(RSpec { kind: RKind::Tiles(vec![("base_height".into(), format!("{:.3}", st.live.now), 2)]) });
            out.push(RSpec { kind: RKind::Tiles(vec![
                ("roll".into(), "−0.021".into(), 0),
                ("pitch".into(), "0.084".into(), 0),
                ("vx".into(), "0.31".into(), 0),
            ]) });
            out.push(cap(caps(&tt("Torque · fraction of limit"))));
            for (n, v) in [("hip", 0.42_f64), ("knee", 0.71), ("ankle", 0.55)] {
                out.push(RSpec { kind: RKind::Segs(vec![(v, SegTone::Vio), (1.0 - v, SegTone::Sink)]) });
                out.push(mono(format!("{} {:.0}%", tt(n), v * 100.0)));
            }
            if let Some((f, g)) = st.cell_phys {
                out.push(ver(1.0, tf("physics from sweep cell — gain {0} · friction {1}", &[&format!("{g:.2}"), &format!("{f:.2}")])));
                out.push(btns(vec![(tt("clear"), "clear-cell".into(), true, false)]));
            }
            out.push(btns(vec![(tt("● record"), "record-probe".into(), true, false)]));
        }
        Mode::Validate => {
            out.push(cap(caps(&tt("Sim-to-sim"))));
            out.push(mono(format!("{}   dec 4: 0.84   dec 1: 0.79   Δ −6%", tt("survival"))));
            out.push(mono(format!("{}   dec 4: 0.71   dec 1: 0.68   Δ −4%", tt("tracking"))));
            out.push(mono(format!("{}   dec 4: 0.55   dec 1: 0.49   Δ −11%", tt("reward"))));
            out.push(ver(1.0, tt("Worst gap 11%. Shares the dynamics function — catches integrator dependence, not modelling error.")));
            out.push(cap(caps(&tt("vs crouch-v2"))));
            if st.sweep_baseline {
                let p = if st.sweep_grid.is_some() { format!("{}%", st.sweep_pass()) } else { "—".into() };
                out.push(mono(format!("{}   {p}   +9", tt("pass rate"))));
                out.push(mono(format!("{}   .11   +.04", tt("low-friction edge"))));
            } else {
                out.push(ver(2.0, tt("Baseline missing. The compared sweep was deleted — re-run crouch-v2 to compare.")));
            }
        }
        Mode::Runs => {
            let Some(r) = st.sel.run.as_ref().and_then(|id| st.run(id)) else {
                out.push(txt(tt("none")));
                app_rmap_done(&mut out);
                return out;
            };
            if r.coll {
                out.push(cap(caps(&tt("(kept checkpoints)"))));
                out.push(txt(tt("Artifact collection — promoted checkpoints kept from deleted runs. Not re-runnable.")));
                for c in &r.ckpts {
                    out.push(RSpec { kind: RKind::Row { nm: c.label.clone(), tg: format!("{:.3}", c.score), pill: Some((format!("★ {}", c.pname.clone().unwrap_or_default()), 0.0)), on: false, click: String::new() } });
                }
                out.push(cap(caps(&tt("Disposition"))));
                out.push(btns(vec![(tt("delete"), "run-delete".into(), true, true)]));
                app_rmap_done(&mut out);
                return out;
            }
            out.push(cap(r.name.to_uppercase()));
            let best = r.best.map(|b| format!("{b:.3}")).unwrap_or_else(|| "—".into());
            out.push(RSpec { kind: RKind::Tiles(vec![
                (tt("set"), r.set.clone(), 0),
                (tt("steps"), format!("{:.2}M", r.steps), 0),
                (tt("best"), best, 0),
            ]) });
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
                    r.snapshot.cfg.iter().map(|c| format!("{} {:.2}", c.name, c.stance)).collect::<Vec<_>>().join(" · "),
                ));
            }
            out.push(cap(caps(&tt("Disposition"))));
            let mut b = vec![
                (tt("↻ re-run"), "rerun".into(), true, false),
                (tt("open in Train"), "open-run".into(), true, false),
            ];
            if !r.archived {
                b.push((tt("archive"), "run-archive".into(), true, false));
            }
            b.push((tt("delete"), "run-delete".into(), true, true));
            out.push(btns(b));
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
    let line_txt = |l: &Line| trf(lang, l.k, &l.a.iter().map(|s| s.as_str()).collect::<Vec<_>>());
    // Record view
    if let Some(dep) = st.sel.dep.as_ref().and_then(|id| st.deploy(id)) {
        out.push(capb(caps(&tt("Deployment record")), tt("← back"), "dep-back"));
        let tone = match dep.state {
            DepState::Live => 0.0,
            DepState::Aborted => 3.0,
            DepState::RolledBack => 2.0,
            _ => 4.0,
        };
        out.push(RSpec { kind: RKind::Row { nm: dep.name.clone(), tg: String::new(), pill: Some((tt(dep.state.key()), tone)), on: false, click: String::new() } });
        out.push(RSpec { kind: RKind::Tiles(vec![
            (tt("policy"), dep.pname.clone(), 0),
            (tt("target"), dep.target.clone(), 0),
        ]) });
        out.push(RSpec { kind: RKind::Tiles(vec![
            (tt("artifact"), dep.hash.clone(), 0),
            (tt("reached"), format!("{}%", [40, 70, 100][dep.stage]), 0),
        ]) });
        out.push(mono(tf("target rev {0}", &[&dep.tg_rev.to_string()])));
        if let Some(n) = &dep.note {
            out.push(ver(1.0, line_txt(n)));
        }
        if let Some(tg) = &dep.tg_snap {
            out.push(cap(caps(&tt("Target snapshot"))));
            out.push(mono(format!("{} · {} · {} · {} · {} {}%", tg.model, tg.ip, tg.iface, tg.sdk, tt("cap"), tg.cap)));
        }
        out.push(cap(caps(&tt("Gate report"))));
        for g in &dep.gate_rep {
            out.push(mono(format!("✓ {}", line_txt(g))));
        }
        out.push(cap(caps(&tt("Checklist"))));
        let labels = ["Area clear of people", "Harness attached", "E-stop within reach", "Battery above 30%"];
        for (i, lb) in labels.iter().enumerate() {
            out.push(mono(format!("{} {}", if dep.chk[i] { "✓" } else { "✕" }, tt(lb))));
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
            out.push(txt(tt("software stop — the pendant E-stop stays authoritative")));
        }
        return;
    }
    // Rollout console
    if let Some(live) = st.dg.armed.as_ref().and_then(|id| st.deploy(id)) {
        out.push(cap(format!("{} · {}", caps(&tt("Rollout")), live.name)));
        let mut segs = vec![];
        for i in 0..3 {
            segs.push((1.0, if i < live.stage { SegTone::Ok } else if i == live.stage { SegTone::Vio } else { SegTone::Sink }));
        }
        out.push(RSpec { kind: RKind::Segs(segs) });
        out.push(mono("40% · 70% · 100%".into()));
        let cap_v = [40, 70, 100][live.stage];
        out.push(RSpec { kind: RKind::Tiles(vec![
            (tt("torque cap"), format!("{cap_v}%"), 1),
            (tt("peak torque"), format!("{}%", st.dg_live.torq), 2),
            (tt("height err"), format!("{:.3}", st.dg_live.herr), 0),
        ]) });
        out.push(ver(0.0, tt("Watching. Advance only after the peak torque settles well under the cap — headroom is the safety margin.")));
        let adv = if live.stage < 2 {
            Some((
                tf("advance to {0}%", &[&[40, 70, 100][live.stage + 1].to_string()]),
                "stage-adv".to_string(),
                st.dg_live.unlocked,
            ))
        } else {
            Some((tt("rollout complete — keep live"), "dep-done".to_string(), true))
        };
        let rollback_en = st.deploys.iter().any(|d| d.state == DepState::Superseded && d.tg_id == live.tg_id);
        out.push(RSpec { kind: RKind::Estop { adv, rollback_en } });
        if !st.dg_live.unlocked && live.stage < 2 {
            out.push(mono(tf("settling · {0}/4 healthy s", &[&st.dg_live.dwell.min(4).to_string()])));
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
    out.push(RSpec { kind: RKind::Tiles(vec![
        (tt("policy"), policy, 0),
        (tt("target"), tg.map(|t| t.name.clone()).unwrap_or_else(|| "—".into()), 0),
    ]) });
    out.push(capb(caps(&tt("Gates · in order")), tt("simulated evidence"), ""));
    let ping_ok = tg.map(|t| matches!(t.ping, Ping::Ok(_))).unwrap_or(false);
    out.push(gate_line(st, GateId::Export, tt("1 · Export"), tt("ONNX for the sdk2 controller — TorchScript available"), st.dg.ck.is_none(), tt("pick a policy first")));
    out.push(gate_line(st, GateId::Compat, tt("2 · Compatibility"), tf("five checks against {0}", &[&tg.map(|t| t.model.clone()).unwrap_or_default()]), !st.dg.export.ok(), tt("export first")));
    out.push(gate_line(st, GateId::Sim2sim, tt("3 · Sim-to-sim"), tt("MuJoCo replica — catches export drift, not modelling error"), !st.dg.compat.ok(), tt("compatibility first")));
    let dr_reason = if !st.dg.sim2sim.ok() { tt("sim-to-sim first") } else { tt("target unreachable — ping it first") };
    out.push(gate_line(st, GateId::Dryrun, tt("4 · Dry-run"), tt("actions computed on the real robot, not applied — needs a reachable target"), !st.dg.sim2sim.ok() || !ping_ok, dr_reason));
    out.push(cap(caps(&tt("Safety checklist · a human signs"))));
    let labels = ["Area clear of people", "Harness attached", "E-stop within reach", "Battery above 30%"];
    for (i, lb) in labels.iter().enumerate() {
        out.push(RSpec { kind: RKind::Cbx { lb: tt(lb), idx: i, val: st.dg.chk[i] } });
    }
    let missing = st.arm_missing();
    out.push(btns(vec![(tt("Arm rollout →"), "dep-arm".into(), missing.is_none(), false)]));
    if let Some(m) = missing {
        out.push(mono(format!("{}", tf("blocked: {0}", &[&tt(m)]))));
    }
    out.push(cap(format!("{} · {}", caps(&tt("History")), st.deploys.len())));
    for d in &st.deploys {
        let tone = match d.state {
            DepState::Live => 0.0,
            DepState::Aborted => 3.0,
            DepState::RolledBack => 2.0,
            _ => 4.0,
        };
        out.push(RSpec { kind: RKind::Row {
            nm: d.name.clone(),
            tg: format!("{} → {}", d.pname, d.target),
            pill: Some((tt(d.state.key()), tone)),
            on: false,
            click: format!("sel-dep:{}", d.id),
        } });
    }
}

fn rline_paths() -> [&'static [LiveId]; 28] {
    [
        ids!(rr.r0), ids!(rr.r1), ids!(rr.r2), ids!(rr.r3), ids!(rr.r4), ids!(rr.r5), ids!(rr.r6),
        ids!(rr.r7), ids!(rr.r8), ids!(rr.r9), ids!(rr.r10), ids!(rr.r11), ids!(rr.r12), ids!(rr.r13),
        ids!(rr.r14), ids!(rr.r15), ids!(rr.r16), ids!(rr.r17), ids!(rr.r18), ids!(rr.r19), ids!(rr.r20),
        ids!(rr.r21), ids!(rr.r22), ids!(rr.r23), ids!(rr.r24), ids!(rr.r25), ids!(rr.r26), ids!(rr.r27),
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
        for vp in [ids!(tiles), ids!(chk), ids!(sld), ids!(ver), ids!(row), ids!(btns), ids!(cap), ids!(cbx), ids!(sparkw), ids!(segw), ids!(hot)] {
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
            RKind::Chk { st, nm, ds, act, cancel } => {
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
                    Some(_) => set_btn(cx, &line, ids!(chk.act2), &t(app.st.lang, "cancel"), true, false, true, l),
                    None => {
                        let b = line.button(cx, ids!(chk.act2));
                        if !b.is_empty() {
                            b.set_visible(cx, false);
                        }
                    }
                }
            }
            RKind::Sld { lb, val, min, max, fmt, .. } => {
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
            RKind::Row { nm, tg, pill, on, .. } => {
                let rv = line.view(cx, ids!(row));
                rv.set_visible(cx, true);
                set_label(cx, &line, ids!(row.nm), nm, l);
                set_label(cx, &line, ids!(row.tg), tg, l);
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
                let mut cold: Vec<&(String, String, bool, bool)> = bs.iter().filter(|b| !b.3).collect();
                let hot: Option<&(String, String, bool, bool)> = bs.iter().find(|b| b.3);
                cold.truncate(3);
                for (j, bp) in bpaths.iter().enumerate().take(3) {
                    match cold.get(j) {
                        Some((label, _act, enabled, _)) => set_btn(cx, &line, *bp, label, true, false, *enabled, l),
                        None => {
                            let b = line.button(cx, *bp);
                            if !b.is_empty() {
                                b.set_visible(cx, false);
                            }
                        }
                    }
                }
                match hot {
                    Some((label, _act, enabled, _)) => set_btn(cx, &line, ids!(btns.b3), label, true, false, *enabled, l),
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
                    Some((label, _act, enabled)) => set_btn(cx, &line, ids!(hot.h0), label, true, false, *enabled, l),
                    None => {
                        let b = line.button(cx, ids!(hot.h0));
                        if !b.is_empty() {
                            b.set_visible(cx, false);
                        }
                    }
                }
                set_btn(cx, &line, ids!(hot.h1), &t(app.st.lang, "E-STOP"), true, false, true, l);
                set_btn(cx, &line, ids!(hot.h2), &t(app.st.lang, "rollback"), true, false, *rollback_en, l);
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
            RKind::Cap { btn: Some((_, act)), .. } => {
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
            RKind::Sld { field, min, max, .. } => {
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
