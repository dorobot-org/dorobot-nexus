//! The modal host: one overlay panel whose generic children are arranged per
//! AppModal kind — every dialog, the six-step import wizard, and the tour.

use super::*;

fn modal_key(m: &AppModal) -> String {
    format!("{m:?}")
}

pub fn sync_modal(app: &mut App, cx: &mut Cx) {
    let l = app.light();
    let lang = app.st.lang;
    let tt = |k: &str| tr(lang, k).to_string();
    let tf = |k: &str, a: &[&str]| trf(lang, k, a);
    let host = app.ui.widget(cx, ids!(modal_host));
    if host.is_empty() {
        return;
    }
    let vis = app.st.modal != AppModal::None;
    app.ui.view(cx, ids!(modal_host)).set_visible(cx, vis);
    if !vis {
        app.modal_seen = String::new();
        return;
    }
    let fresh = app.modal_seen != modal_key(&app.st.modal);
    app.modal_seen = modal_key(&app.st.modal);
    {
        let mut p = host.widget(cx, ids!(panel));
        script_apply_eval!(cx, p, { draw_bg +: { light: #(l as f64) } });
    }
    // hide everything, then reveal per kind
    let hide_paths: [&[LiveId]; 26] = [
        ids!(panel.sub), ids!(panel.blast), ids!(panel.wsteps), ids!(panel.info),
        ids!(panel.in_cap0), ids!(panel.input0), ids!(panel.in_cap1), ids!(panel.input1),
        ids!(panel.in_cap2), ids!(panel.input2),
        ids!(panel.opt0), ids!(panel.opt1), ids!(panel.opt2), ids!(panel.opt3),
        ids!(panel.opt4), ids!(panel.opt5), ids!(panel.opt6), ids!(panel.opt7),
        ids!(panel.chk0), ids!(panel.chk1), ids!(panel.chk2), ids!(panel.chk3), ids!(panel.chk4),
        ids!(panel.foot.b_back), ids!(panel.foot.b_ok), ids!(panel.foot.b_danger),
    ];
    for p in hide_paths {
        let w = host.widget(cx, p);
        if !w.is_empty() {
            w.set_visible(cx, false);
        }
    }
    let title;
    let mut sub: Option<String> = None;
    let mut blast: Option<(f64, String)> = None;
    let mut wsteps: Option<String> = None;
    let mut info: Option<String> = None;
    let mut inputs: Vec<(String, String)> = vec![]; // (cap, default)
    let mut opts: Vec<(String, bool)> = vec![];
    let mut chks: Vec<(f64, String, String)> = vec![];
    let mut ok: Option<String> = None;
    let mut danger: Option<String> = None;
    let mut back = false;
    let st = &app.st;
    match &st.modal {
        AppModal::None => return,
        AppModal::Tour => {
            title = tt("Go through it");
            sub = Some(tt("Eight paths that exercise every lifecycle. Colour grammar: cyan asked · orange achieved · green done · amber strained · grey unmeasured."));
            opts = vec![
                (format!("0  {}", tt("Home — the landing view: live session, recent runs, sets, robots, the last sweep. Every card is a door and every number updates as training runs.")), false),
                (format!("1  {}", tt("Scenes — drag the cyan stance slider. The target line moves, the world re-simulates, the row goes ● unsaved. Save, Save as…, or Revert.")), false),
                (format!("2  {}", tt("Delete a scene. The dialog names its blast radius; the toast offers Undo; the set composer shows the broken slot.")), false),
                (format!("3  {}", tt("Train this set → pre-flight: warm start, honest time budget, Launch. Then pause — the tooltip states what pausing actually costs.")), false),
                (format!("4  {}", tt("Robots → + Import. Six gates; the structure step fails on collision geometry; the rehearsal sweeps each joint.")), false),
                (format!("5  {}", tt("Validate → run sweep. Click a red cell: open it in Inspect (physics pre-loaded) or save it as a scene — a weakness becomes training material in one verb.")), false),
                (format!("6  {}", tt("Runs — open the failed run to read its diagnosis; Re-run any run from its snapshot; delete one and answer the checkpoint question.")), false),
                (format!("7  {}", tt("Deploy — pick the promoted policy, run the gate ladder (export → compatibility → sim-to-sim → dry-run), sign the checklist, then stage torque 40 → 70 → 100% with E-STOP one click away.")), false),
            ];
            info = Some(tt("Deploy gate evidence is simulated and labelled as such — the real control plane (SDK2 agent, real gates) is the next milestone."));
            ok = Some(tt("Start exploring"));
        }
        AppModal::DeleteScene => {
            let name = st.sel.scene.as_ref().and_then(|id| st.scene(id)).map(|s| s.name.clone()).unwrap_or_default();
            title = tf("Delete {0}?", &[&name]);
            sub = Some(tt("Blast radius, before you decide:"));
            let sets = st.sel.scene.as_ref().map(|id| st.sets_of(id)).unwrap_or_default();
            let inset = if sets.is_empty() {
                tt("In no sets.")
            } else {
                tf("In set {0} — the set will show a broken slot until fixed.", &[&sets.iter().map(|s| s.name.clone()).collect::<Vec<_>>().join(", ")])
            };
            blast = Some((1.0, format!("{inset} {}", tt("Runs are unaffected — every run holds its own snapshot. Recordings keep their tags."))));
            danger = Some(tt("Delete"));
        }
        AppModal::LastSceneBlocked => {
            title = tt("Delete?");
            blast = Some((2.0, tt("Blocked: this is the last scene. An empty library has no baseline to simulate — duplicate first, then delete.")));
            ok = Some(tt("Understood"));
        }
        AppModal::AddToSet => {
            let name = st.sel.scene.as_ref().and_then(|id| st.scene(id)).map(|s| s.name.clone()).unwrap_or_default();
            title = tf("Add {0} to a set", &[&name]);
            sub = Some(tt("A scene can belong to several sets; a set is one ordering of scenes."));
            for s in &st.sets {
                opts.push((format!("{} · {}", s.name, tf("{0} scenes", &[&s.members.len().to_string()])), false));
            }
        }
        AppModal::RenameSet => {
            title = tt("Rename set");
            let cur = st.sel.set.as_ref().and_then(|id| st.set(id)).map(|s| s.name.clone()).unwrap_or_default();
            inputs.push((tt("Name"), cur));
            ok = Some(tt("Rename"));
        }
        AppModal::Preflight => {
            let set = st.sel.set.as_ref().and_then(|id| st.set(id));
            let set_name = set.map(|s| s.name.clone()).unwrap_or_default();
            let n = set.map(|s| s.members.len()).unwrap_or(0);
            let broken = set.map(|s| s.members.iter().any(|m| st.scene(m).is_none())).unwrap_or(false);
            title = tf("Pre-flight · {0}", &[&set_name]);
            sub = Some(tt("A run snapshots its inputs at launch. Edits after launch apply to the next run."));
            if broken {
                blast = Some((2.0, tt("This set has a broken slot (a deleted scene). Fix or remove it in the composer — training through a hole is refused.")));
            }
            inputs.push((tt("Run name"), "crouch-v4".into()));
            opts = vec![
                (tt("Resume latest — ck-2100k"), app.warm_sel == 0),
                (tt("From promoted — crouch-v2-final"), app.warm_sel == 1),
                (tt("Fresh"), app.warm_sel == 2),
            ];
            let iters = n * 400 * 6;
            info = Some(tf("Budget: {0} scenes × 400 it × 6 rounds ≈ {1} iterations ≈ {2} h at measured throughput (512 envs, this machine).", &[
                &n.to_string(),
                &iters.to_string(),
                &((iters as f64 * 3.9 / 3600.0).round() as u64).to_string(),
            ]));
            if !broken {
                ok = Some(tt("Launch"));
            }
        }
        AppModal::Rerun => {
            let r = st.sel.run.as_ref().and_then(|id| st.run(id));
            let name = r.map(|r| r.name.clone()).unwrap_or_default();
            title = tf("Re-run {0}", &[&name]);
            sub = Some(tt("Launches from this run’s snapshot — identical fields and seed, regardless of what the library has become."));
            if let Some(r) = r {
                info = Some(tf("Snapshot: {0} scenes · seed {1}", &[&r.snapshot.scenes.to_string(), &r.snapshot.seed]));
            }
            inputs.push((tt("New run name"), format!("{name}-repro")));
            ok = Some(tt("Launch"));
        }
        AppModal::DeleteRun => {
            let r = st.sel.run.as_ref().and_then(|id| st.run(id));
            let name = r.map(|r| if r.coll { tt("(kept checkpoints)") } else { r.name.clone() }).unwrap_or_default();
            title = tf("Delete run {0}?", &[&name]);
            if let Some(r) = r {
                let prom = if r.coll { 0 } else { r.ckpts.iter().filter(|c| c.st == CkSt::Prom).count() };
                let promtxt = if prom > 0 { tf(" — {0} promoted.", &[&prom.to_string()]) } else { ". ".into() };
                blast = Some((1.0, tf("This run owns {0} checkpoints{1} Recordings survive; they carry their own tags.", &[&r.ckpts.len().to_string(), &promtxt])));
                if prom > 0 {
                    opts = vec![
                        (tt("Keep the promoted checkpoints"), app.keep_sel == 0),
                        (tt("Delete everything"), app.keep_sel == 1),
                    ];
                }
            }
            danger = Some(tt("Delete run"));
        }
        AppModal::PromoteCk { .. } => {
            title = tt("Promote checkpoint");
            sub = Some(tt("A promoted checkpoint is exempt from pruning and can be deployed or used as a warm start."));
            inputs.push((tt("Name"), "crouch-v3-final".into()));
            ok = Some(tt("Promote"));
        }
        AppModal::RemoveRobotBlocked => {
            let r = st.sel.robot.as_ref().and_then(|id| st.robots.iter().find(|r| &r.id == id));
            title = tf("Remove {0}?", &[&r.map(|r| r.name.clone()).unwrap_or_default()]);
            let used = r.map(|r| r.used_by.iter().map(|u| tt(u)).collect::<Vec<_>>().join(", ")).unwrap_or_default();
            blast = Some((2.0, tf("Blocked: referenced by {0}. Point those scenes at another robot first — nothing is greyed out without its reason.", &[&used])));
            ok = Some(tt("Understood"));
        }
        AppModal::Wizard { step } => {
            title = tt("Import robot");
            let names = ["Source", "Assets", "Structure", "Rehearse", "Map", "Commit"];
            let marks: Vec<String> = names
                .iter()
                .enumerate()
                .map(|(i, n)| {
                    let i = i as u32 + 1;
                    if i < *step {
                        format!("✓ {}", tt(n))
                    } else if i == *step {
                        format!("[{}]", tt(n))
                    } else {
                        tt(n)
                    }
                })
                .collect();
            wsteps = Some(marks.join("  ·  "));
            match step {
                1 => {
                    sub = Some(tt("Pick the robot description. The wizard copies everything on commit."));
                    info = Some(match &st.wiz_file {
                        Some(p) => p.clone(),
                        None => tt("No file chosen yet — Choose file opens the system browser."),
                    });
                    ok = Some(if st.wiz_file.is_some() { tt("Continue") } else { tt("Choose file…") });
                }
                2 => {
                    sub = Some(tt("Resolving 28 mesh references against the chosen asset directory…"));
                    chks.push((0.0, tt("28 of 28 meshes found"), String::new()));
                    back = true;
                    ok = Some(tt("Continue"));
                }
                3 => {
                    sub = Some(tt("Structural checks — each one exists because its absence produced a real, silent failure."));
                    chks = vec![
                        (0.0, tt("XML parses"), tt("31 links · 30 joints")),
                        (1.0, tt("Commented-out joint excluded"), tt("counting it would shift every index by one")),
                        (0.0, tt("Inertias non-degenerate"), tt("a singular mass matrix means NaN on step one")),
                        (2.0, tt("Collision geometry"), tt("3 links are visual-only — they will not collide. Fix the URDF or accept.")),
                        (0.0, tt("Single physics step finite"), tt("the catch-all that cost days when absent")),
                    ];
                    back = true;
                    ok = Some(tt("Accept note & rehearse"));
                }
                4 => {
                    sub = Some(tt("Each joint driven min → max → min alone, then all together — watching for self-intersection, detached links, non-finite poses."));
                    chks = vec![
                        (0.0, "left_hip_pitch".into(), tt("swept ✓")),
                        (0.0, "left_knee".into(), tt("swept ✓")),
                        (1.0, "right_hip_pitch".into(), tt("sweeping…")),
                        (3.0, "right_knee".into(), tt("queued")),
                    ];
                    back = true;
                    ok = Some(tt("Rehearsal passed →"));
                }
                5 => {
                    sub = Some(tt("Bind URDF joints to the trainer’s actuated set. Shown, never inferred — a silent mismatch here is fatal."));
                    info = Some(format!(
                        "left_hip_pitch_joint → motor_0\nleft_knee_joint → motor_1\nright_hip_pitch_joint → motor_2\nright_knee_joint → motor_3\n{}",
                        tt("… 12 of 23 movable joints bound; the rest hold position")
                    ));
                    back = true;
                    ok = Some(tt("Mapping confirmed"));
                }
                _ => {
                    sub = Some(tt("Everything below is copied into the library. The source directory can vanish tomorrow."));
                    let fname = st
                        .wiz_file
                        .as_ref()
                        .and_then(|p| std::path::Path::new(p).file_name().map(|s| s.to_string_lossy().to_string()))
                        .unwrap_or_else(|| "h2_plus.urdf".into());
                    info = Some(format!("{fname} · {}", tt("28 meshes · joint map (12 bound) · validation report with 1 accepted note, 1 accepted failure")));
                    back = true;
                    ok = Some(tt("Commit robot"));
                }
            }
        }
        AppModal::TargetForm { edit } => {
            let editing = edit.as_ref().and_then(|id| st.target(id));
            title = if editing.is_some() { tt("Edit target") } else { tt("Add target") };
            if editing.is_none() {
                sub = Some(tt("A target is an authored endpoint — deployments reference it by snapshot."));
            }
            let (n, ip, capv) = match editing {
                Some(t) => (t.name.clone(), format!("{} · {}", t.ip, t.iface), t.cap.to_string()),
                None => ("g1-lab-C".into(), "192.168.123.162 · eth0".into(), "70".into()),
            };
            inputs.push((tt("Name"), n));
            inputs.push((tt("IP · interface"), ip));
            inputs.push((tt("Torque cap profile"), capv));
            ok = Some(if editing.is_some() { tt("Save") } else { tt("Add") });
        }
        AppModal::RemoveTargetBlocked { held } => {
            title = tt("Remove?");
            blast = Some((2.0, tf("Blocked: {0} is live on this target. Retire or roll back the deployment first — nothing is greyed out without its reason.", &[held])));
            ok = Some(tt("Understood"));
        }
        AppModal::Arm => {
            let tg = st.dg.target.as_ref().and_then(|t| st.target(t));
            title = tf("Arm rollout on {0}?", &[&tg.map(|t| t.name.clone()).unwrap_or_default()]);
            sub = Some(tt("Arming snapshots everything below into an immutable record."));
            if let (Some(tg), Gate::Pass { hash, .. }) = (tg, &st.dg.export) {
                info = Some(tf("{0} · {1} · id {2} → {3} ({4}) · torque staged 40 → 70 → 100% of the {5}% profile", &[
                    &st.dg.pname.clone().unwrap_or_default(),
                    &st.dg.ck.clone().unwrap_or_default(),
                    hash,
                    &tg.name,
                    &tg.ip,
                    &tg.cap.to_string(),
                ]));
            }
            ok = Some(tt("Arm at 40%"));
        }
    }
    set_label(cx, &host, ids!(panel.title), &title, l);
    if let Some(s) = &sub {
        host.widget(cx, ids!(panel.sub)).set_visible(cx, true);
        set_label(cx, &host, ids!(panel.sub), s, l);
    }
    if let Some((tone, text)) = &blast {
        host.widget(cx, ids!(panel.blast)).set_visible(cx, true);
        set_label(cx, &host, ids!(panel.blast.lbl), text, l);
        let mut b = host.widget(cx, ids!(panel.blast));
        let tf2 = *tone;
        script_apply_eval!(cx, b, { draw_bg +: { tone: #(tf2) light: #(l as f64) } });
    }
    if let Some(w) = &wsteps {
        host.widget(cx, ids!(panel.wsteps)).set_visible(cx, true);
        set_label(cx, &host, ids!(panel.wsteps), w, l);
    }
    if let Some(i) = &info {
        host.widget(cx, ids!(panel.info)).set_visible(cx, true);
        set_label(cx, &host, ids!(panel.info), i, l);
    }
    let icaps = [ids!(panel.in_cap0), ids!(panel.in_cap1), ids!(panel.in_cap2)];
    let ifields = [ids!(panel.input0), ids!(panel.input1), ids!(panel.input2)];
    for (i, (capt, default)) in inputs.iter().enumerate() {
        host.widget(cx, icaps[i]).set_visible(cx, true);
        set_label(cx, &host, icaps[i], &caps(capt), l);
        let ti = host.widget(cx, ifields[i]);
        ti.set_visible(cx, true);
        if fresh {
            ti.set_text(cx, default);
        }
    }
    let opaths = [
        ids!(panel.opt0), ids!(panel.opt1), ids!(panel.opt2), ids!(panel.opt3),
        ids!(panel.opt4), ids!(panel.opt5), ids!(panel.opt6), ids!(panel.opt7),
    ];
    for (i, (label, on)) in opts.iter().enumerate() {
        if i >= opaths.len() {
            break;
        }
        let row = host.widget(cx, opaths[i]);
        row.set_visible(cx, true);
        set_label(cx, &row, ids!(nm), label, l);
        set_label(cx, &row, ids!(tg), "", l);
        let onf = if *on { 1.0 } else { 0.0 };
        let mut rw = host.widget(cx, opaths[i]);
        script_apply_eval!(cx, rw, { draw_bg +: { on: #(onf) light: #(l as f64) } });
        let pw = row.widget(cx, ids!(pill));
        if !pw.is_empty() {
            pw.set_visible(cx, false);
        }
    }
    let cpaths = [ids!(panel.chk0), ids!(panel.chk1), ids!(panel.chk2), ids!(panel.chk3), ids!(panel.chk4)];
    for (i, (stv, nm, ds)) in chks.iter().enumerate() {
        if i >= cpaths.len() {
            break;
        }
        let row = host.widget(cx, cpaths[i]);
        row.set_visible(cx, true);
        let ic = match *stv as i32 {
            0 => "✓",
            1 => "!",
            2 => "✕",
            _ => "·",
        };
        set_label(cx, &row, ids!(ico), ic, l);
        {
            let mut iw = row.widget(cx, ids!(ico));
            let sv = *stv;
            script_apply_eval!(cx, iw, { draw_text +: { st: #(sv) } });
        }
        set_label(cx, &row, ids!(body.nm), nm, l);
        set_label(cx, &row, ids!(body.ds), ds, l);
        for bp in [ids!(act), ids!(act2)] {
            let b = row.button(cx, bp);
            if !b.is_empty() {
                b.set_visible(cx, false);
            }
        }
    }
    set_btn(cx, &host, ids!(panel.foot.b_cancel), &tt("Cancel"), true, false, true, l);
    if back {
        set_btn(cx, &host, ids!(panel.foot.b_back), &tt("Back"), true, false, true, l);
    }
    if let Some(o) = &ok {
        set_btn(cx, &host, ids!(panel.foot.b_ok), o, true, true, true, l);
    }
    if let Some(d) = &danger {
        set_btn(cx, &host, ids!(panel.foot.b_danger), d, true, false, true, l);
    }
}

/// Modal option-row activation (driver + click share this).
pub fn modal_opt(app: &mut App, cx: &mut Cx, i: usize) {
    match &app.st.modal {
        AppModal::AddToSet => {
            if let Some(s) = app.st.sets.get(i) {
                let id = s.id.clone();
                app.st.addto_set_yes(&id);
            }
        }
        AppModal::Preflight => app.warm_sel = i.min(2),
        AppModal::DeleteRun => app.keep_sel = i.min(1),
        _ => {}
    }
    app.sync_all(cx);
}

pub fn modal_back(app: &mut App, cx: &mut Cx) {
    if let AppModal::Wizard { step } = &app.st.modal {
        let s = (*step).max(2) - 1;
        app.st.modal = AppModal::Wizard { step: s };
        app.sync_all(cx);
    }
}

pub fn modal_danger(app: &mut App, cx: &mut Cx) {
    match &app.st.modal {
        AppModal::DeleteScene => app.st.del_scene_yes(),
        AppModal::DeleteRun => {
            let keep = app.keep_sel == 0;
            app.st.run_delete_yes(keep);
        }
        _ => {}
    }
    app.sync_all(cx);
}

pub fn modal_ok(app: &mut App, cx: &mut Cx) {
    let host = app.ui.widget(cx, ids!(modal_host));
    let modal = app.st.modal.clone();
    match modal {
        AppModal::Tour | AppModal::LastSceneBlocked | AppModal::RemoveRobotBlocked | AppModal::RemoveTargetBlocked { .. } => {
            app.st.modal = AppModal::None;
        }
        AppModal::RenameSet => {
            let name = host.widget(cx, ids!(panel.input0)).text();
            app.st.set_rename_yes(&name);
        }
        AppModal::Preflight => {
            let name = host.widget(cx, ids!(panel.input0)).text();
            let name = if name.is_empty() { "crouch-v4".into() } else { name };
            let warm = ["Resume latest — ck-2100k", "From promoted — crouch-v2-final", "Fresh"][app.warm_sel.min(2)];
            app.st.launch(&name, warm);
        }
        AppModal::Rerun => {
            let name = host.widget(cx, ids!(panel.input0)).text();
            app.st.rerun_yes(&name);
        }
        AppModal::PromoteCk { ck_id } => {
            let name = host.widget(cx, ids!(panel.input0)).text();
            app.st.ck_promote_yes(&ck_id, &name);
        }
        AppModal::Wizard { step } => {
            if step == 1 && app.st.wiz_file.is_none() {
                let picked = rfd::FileDialog::new()
                    .set_title("Pick a robot description")
                    .add_filter("Robot description (URDF/XML/MJCF)", &["urdf", "xml", "mjcf"])
                    .add_filter("All files", &["*"])
                    .set_directory("/Users/yuechen/home/makepad-urdf-viewer/data")
                    .pick_file();
                if let Some(p) = picked {
                    app.st.wiz_file = Some(p.to_string_lossy().to_string());
                    app.st.modal = AppModal::Wizard { step: 2 };
                }
            } else if step >= 6 {
                app.st.wiz_commit();
                app.st.wiz_file = None;
            } else {
                app.st.modal = AppModal::Wizard { step: step + 1 };
            }
        }
        AppModal::TargetForm { edit } => {
            let name = host.widget(cx, ids!(panel.input0)).text();
            let ipif = host.widget(cx, ids!(panel.input1)).text();
            let capv = host.widget(cx, ids!(panel.input2)).text().trim().parse::<u32>().unwrap_or(70);
            let (ip, iface) = match ipif.split_once('·') {
                Some((a, b)) => (a.trim().to_string(), b.trim().to_string()),
                None => (ipif.trim().to_string(), String::new()),
            };
            app.st.target_save(edit, &name, &ip, &iface, capv);
        }
        AppModal::Arm => {
            app.st.dep_arm_yes();
        }
        _ => {}
    }
    app.sync_all(cx);
}

pub fn route_modal(app: &mut App, cx: &mut Cx, actions: &Actions) {
    if app.st.modal == AppModal::None {
        return;
    }
    let host = app.ui.widget(cx, ids!(modal_host));
    if host.is_empty() {
        return;
    }
    if host.button(cx, ids!(panel.foot.b_cancel)).clicked(actions) {
        app.st.modal = AppModal::None;
        app.sync_all(cx);
        return;
    }
    if host.button(cx, ids!(panel.foot.b_back)).clicked(actions) {
        modal_back(app, cx);
        return;
    }
    // option rows
    let opaths = [
        ids!(panel.opt0), ids!(panel.opt1), ids!(panel.opt2), ids!(panel.opt3),
        ids!(panel.opt4), ids!(panel.opt5), ids!(panel.opt6), ids!(panel.opt7),
    ];
    for (i, op) in opaths.iter().enumerate() {
        let uid = host.widget(cx, *op).widget_uid();
        if view_clicked(actions, uid) {
            modal_opt(app, cx, i);
            return;
        }
    }
    if host.button(cx, ids!(panel.foot.b_danger)).clicked(actions) {
        modal_danger(app, cx);
        return;
    }
    if host.button(cx, ids!(panel.foot.b_ok)).clicked(actions) {
        modal_ok(app, cx);
    }
}
