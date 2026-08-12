//! Center stage (viewport, guide lines, banner), the per-mode timeline
//! panel, and the toast stack.

use super::*;
use crate::kit::*;
use makepad_urdf_player::robot_view::RobotViewWidgetRefExt;

pub fn sync_mode_frame(app: &mut App, cx: &mut Cx) {
    let home = app.st.mode == Mode::Home;
    let canvas = app.ui.view(cx, ids!(canvas));
    if !canvas.is_empty() {
        canvas.set_visible(cx, !home);
    }
    let hp = app.ui.view(cx, ids!(home));
    if !hp.is_empty() {
        hp.set_visible(cx, home);
    }
    let comp = app.ui.view(cx, ids!(composer));
    if !comp.is_empty() {
        comp.set_visible(cx, app.st.mode == Mode::Scenes);
    }
    let (wl, wr) = match app.st.mode {
        Mode::Scenes => (258.0, 312.0),
        Mode::Robots => (250.0, 320.0),
        Mode::Train => (196.0, 288.0),
        Mode::Inspect => (250.0, 288.0),
        Mode::Validate => (216.0, 272.0),
        Mode::Runs => (320.0, 300.0),
        Mode::Deploy => (252.0, 336.0),
        Mode::Home => (252.0, 336.0),
    };
    {
        let mut rl = app.ui.view(cx, ids!(rail_l));
        if !rl.is_empty() {
            script_apply_eval!(cx, rl, { width: #(wl) });
        }
        let mut rr2 = app.ui.view(cx, ids!(rail_r));
        if !rr2.is_empty() {
            script_apply_eval!(cx, rr2, { width: #(wr) });
        }
    }
    sync_composer(app, cx);
}

fn stance_frac(v: f64) -> f64 {
    // mock: bottom% = 26 + (v-0.45)/(0.84-0.45)*36  → top padding fraction
    let pct = 26.0 + (v - 0.45) / (0.84 - 0.45) * 36.0;
    1.0 - pct / 100.0
}

pub fn sync_stage_tl(app: &mut App, cx: &mut Cx) {
    let l = app.light();
    let lang = app.st.lang;
    let tt = |k: &str| tr(lang, k).to_string();
    let tf = |k: &str, a: &[&str]| trf(lang, k, a);
    let stage = app.ui.widget(cx, ids!(stage));
    if stage.is_empty() {
        return;
    }
    {
        let mut s = app.ui.view(cx, ids!(stage));
        script_apply_eval!(cx, s, { draw_bg +: { light: #(l as f64) } });
    }
    // 3D URDF view in Robots mode when the selected robot has a file
    let urdf_path = if app.st.mode == Mode::Robots {
        app.st.sel.robot.as_ref()
            .and_then(|id| app.st.robots.iter().find(|r| &r.id == id))
            .and_then(|r| r.urdf.clone())
    } else if app.replay.is_some() && matches!(app.st.mode, Mode::Scenes | Mode::Inspect) {
        // a real rollout drives the real robot
        app.st.robots.first().and_then(|r| r.urdf.clone())
    } else {
        None
    };
    let show_3d = urdf_path.is_some();
    {
        let wrapv = stage.view(cx, ids!(urdf_wrap));
        if !wrapv.is_empty() {
            wrapv.set_visible(cx, show_3d);
        }
        let figv = stage.view(cx, ids!(robot));
        if !figv.is_empty() {
            figv.set_visible(cx, !show_3d);
        }
        let glv = stage.view(cx, ids!(glines));
        if !glv.is_empty() {
            glv.set_visible(cx, !show_3d);
        }
    }
    if let Some(path) = &urdf_path {
        if app.loaded_urdf.as_deref() != Some(path.as_str()) {
            let assets = std::path::Path::new(path)
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();
            let rv = stage.robot_view(cx, ids!(urdf_wrap.rview));
            if !rv.is_empty() {
                match rv.load_robot(cx, path, &assets) {
                    Ok(()) => {
                        app.loaded_urdf = Some(path.clone());
                        rv.set_animating(cx, true);
                    }
                    Err(e) => {
                        app.loaded_urdf = Some(path.clone()); // don't retry every sync
                        app.st.toast(format!("URDF load failed: {e}"));
                    }
                }
            }
        }
    }
    // robot crouch follows the live height
    let sc = app.st.cur_scene();
    let crouch = ((0.82 - app.st.live.now) / (0.82 - 0.45)).clamp(0.0, 1.0);
    {
        let mut fig = stage.widget(cx, ids!(robot.fig));
        if !fig.is_empty() {
            script_apply_eval!(cx, fig, { draw_bg +: { light: #(l as f64) crouch: #(crouch) } });
        }
    }
    // guide lines: text + vertical position via top margin fraction of ~520px stage
    let tl_txt = tf("target {0}", &[&format!("{:.2}", sc.stance)]);
    let al_txt = tf("actual {0}", &[&format!("{:.3}", app.st.live.now)]);
    set_label(cx, &stage, ids!(glines.line_t.lbl_t), &tl_txt, l);
    set_label(cx, &stage, ids!(glines.line_a.lbl_a), &al_txt, l);
    let h = 520.0_f64;
    let mt = stance_frac(sc.stance) * h;
    let ma = (stance_frac(app.st.live.now) * h - mt).max(4.0);
    {
        let mut w = stage.widget(cx, ids!(glines.line_t));
        if !w.is_empty() {
            script_apply_eval!(cx, w, { margin +: { top: #(mt) } });
        }
        let mut w2 = stage.widget(cx, ids!(glines.line_a));
        if !w2.is_empty() {
            script_apply_eval!(cx, w2, { margin +: { top: #(ma) } });
        }
    }
    // stage info line
    let r = app.st.sel.run.as_ref().and_then(|id| app.st.run(id));
    let info = match app.st.mode {
        Mode::Scenes => format!("{} · {}{}", tt("scene"), sc.name, if app.st.dirty { format!("  {}", tt("● unsaved")) } else { String::new() }),
        Mode::Robots => format!("{} · {}", tt("robot"), app.st.sel.robot.as_ref().and_then(|id| app.st.robots.iter().find(|r| &r.id == id)).map(|r| r.name.clone()).unwrap_or_default()),
        Mode::Train => match r {
            Some(r) => format!("{} · {} · {} {} — {}", tt("run"), r.name, tt("stance"), r.stage + 1, r.stages.get(r.stage).cloned().unwrap_or_default()),
            None => String::new(),
        },
        Mode::Inspect => format!("{} · {} · {}", tt("probe"), app.st.sel.ck.clone().unwrap_or_else(|| "ck-2100k".into()), tt("deterministic")),
        Mode::Validate => format!("{} · {}", tt("sweep"), app.st.sweep_ckpt),
        Mode::Runs => match r {
            Some(r) => format!("{} · {} {}", tt("ledger"), if r.coll { tt("(kept checkpoints)") } else { r.name.clone() }, tt("(read-only)")),
            None => String::new(),
        },
        Mode::Deploy => {
            let lv = app.st.dg.armed.as_ref().and_then(|id| app.st.deploy(id));
            let tg_name = match &lv {
                Some(d) => d.target.clone(),
                None => app.st.dg.target.as_ref().and_then(|t| app.st.target(t)).map(|t| t.name.clone()).unwrap_or_else(|| "—".into()),
            };
            match lv {
                Some(d) => format!("{} · {} · {}", tt("deploy"), tg_name, tf("stage {0} · {1}% torque cap", &[&(d.stage + 1).to_string(), &[40, 70, 100][d.stage].to_string()])),
                None => format!("{} · {} · {}", tt("deploy"), tg_name, tt("candidate")),
            }
        }
        Mode::Home => String::new(),
    };
    set_label(cx, &stage, ids!(stage_info.info), &info, l);
    // banner (inspect · physics from sweep cell)
    let banner = stage.view(cx, ids!(banner));
    if !banner.is_empty() {
        let show = app.st.mode == Mode::Inspect && app.st.cell_phys.is_some();
        banner.set_visible(cx, show);
        if let (true, Some((f, g))) = (show, app.st.cell_phys) {
            let btxt = tf("physics from sweep cell — gain {0} · friction {1}", &[&format!("{g:.2}"), &format!("{f:.2}")]);
            set_label(cx, &stage, ids!(banner.bl.lbl), &btxt, l);
            set_btn(cx, &stage, ids!(banner.bl.clr), &tt("clear"), true, false, true, l);
        }
    }

    // ---------------------------------------------------------- timeline --
    let tlw = app.ui.widget(cx, ids!(tl));
    if tlw.is_empty() {
        return;
    }
    {
        let mut p = app.ui.view(cx, ids!(tl));
        script_apply_eval!(cx, p, { draw_bg +: { light: #(l as f64) } });
    }
    let mode = app.st.mode;
    let frames_v = matches!(mode, Mode::Scenes | Mode::Inspect);
    let run_v = matches!(mode, Mode::Train | Mode::Runs);
    let sweep_v = mode == Mode::Validate;
    let deploy_v = mode == Mode::Deploy;
    let note_v = mode == Mode::Robots;
    for (p, v) in [
        (ids!(sec_frames), frames_v),
        (ids!(sec_run), run_v),
        (ids!(sec_sweep), sweep_v),
        (ids!(sec_deploy), deploy_v),
        (ids!(sec_note), note_v),
    ] {
        let vv = tlw.view(cx, p);
        if !vv.is_empty() {
            vv.set_visible(cx, v);
        }
    }
    if frames_v {
        let frac = app.st.live.frame as f64 / app.st.live.frames.max(1) as f64;
        tlw.scrub(cx, ids!(sec_frames.scrub)).set(cx, frac, l);
        set_btn(cx, &tlw, ids!(sec_frames.row.b_start), "|◀", true, false, true, l);
        set_btn(cx, &tlw, ids!(sec_frames.row.b_back), "◀", true, false, true, l);
        set_btn(cx, &tlw, ids!(sec_frames.row.b_play), if app.st.live.playing { "▮▮" } else { "▶" }, true, true, true, l);
        set_btn(cx, &tlw, ids!(sec_frames.row.b_fwd), "▶", true, false, true, l);
        set_btn(cx, &tlw, ids!(sec_frames.row.b_resim), &tt("re-simulate"), true, false, true, l);
        set_btn(cx, &tlw, ids!(sec_frames.row.b_record), &tt("● record"), true, false, true, l);
        let fl = format!("{} / {}", app.st.live.frame, app.st.live.frames.saturating_sub(1));
        set_label(cx, &tlw, ids!(sec_frames.row.frame_lbl), &fl, l);
    }
    if run_v {
        if let Some(r) = app.st.sel.run.as_ref().and_then(|id| app.st.run(id)) {
            let mut segs: Vec<(f64, SegTone)> = vec![];
            let cur = r.iter as f64 / r.iters_per.max(1) as f64;
            for (i, _) in r.stages.iter().enumerate() {
                if i < r.stage {
                    segs.push((1.0, SegTone::Ok));
                } else if i == r.stage {
                    segs.push((cur.max(0.05), SegTone::Vio));
                    if cur < 1.0 {
                        segs.push((1.0 - cur, SegTone::Sink));
                    }
                } else {
                    segs.push((1.0, SegTone::Sink));
                }
            }
            tlw.segsbar(cx, ids!(sec_run.segs)).set(cx, segs, l);
            set_label(cx, &tlw, ids!(sec_run.ticks), &r.stages.join(" · "), l);
            let tiles = tlw.view(cx, ids!(sec_run.tiles));
            if !tiles.is_empty() {
                tiles.set_visible(cx, mode == Mode::Train);
            }
            if mode == Mode::Train {
                let vals = [
                    (tt("reward"), format!("{:.4}", app.st.live.reward)),
                    (tt("falls"), format!("{:.1}%", app.st.live.falls)),
                    ("steps/s".into(), "3,151".into()),
                    ("KL".into(), format!("{:.4}", app.st.live.kl)),
                    ("lr".into(), app.st.live.lr.clone()),
                    (tt("ep len"), "61".into()),
                ];
                let tpaths = [
                    ids!(sec_run.tiles.tile0), ids!(sec_run.tiles.tile1), ids!(sec_run.tiles.tile2),
                    ids!(sec_run.tiles.tile3), ids!(sec_run.tiles.tile4), ids!(sec_run.tiles.tile5),
                ];
                for (j, tp) in tpaths.iter().enumerate() {
                    let tile = tlw.widget(cx, *tp);
                    if tile.is_empty() {
                        continue;
                    }
                    set_label(cx, &tile, ids!(k), &caps(&vals[j].0), l);
                    set_label(cx, &tile, ids!(v), &vals[j].1, l);
                    let mut bg = tile.clone();
                    script_apply_eval!(cx, bg, { draw_bg +: { light: #(l as f64) } });
                }
            }
            let note = tlw.widget(cx, ids!(sec_run.ro_note));
            if !note.is_empty() {
                note.set_visible(cx, mode == Mode::Runs);
                if mode == Mode::Runs {
                    set_label(cx, &tlw, ids!(sec_run.ro_note), &tt("read-only — this run has ended"), l);
                }
            }
        }
    }
    if sweep_v {
        if let Some(g) = &app.real_grid {
            tlw.heat(cx, ids!(sec_sweep.heat)).set_dyn(cx, g, None, l);
        } else {
            tlw.heat(cx, ids!(sec_sweep.heat)).set(cx, app.st.sweep_grid.as_ref(), app.st.sweep_at, app.st.sel.cell, l, false);
        }
        let rc = &RECIPES[app.st.sel.recipe];
        let ax = format!(
            "Y {} — {}   ·   X {} — {}",
            rc.fr.iter().map(|v| format!("{v:.2}")).collect::<Vec<_>>().join(" "),
            tt(rc.yl),
            rc.ga.iter().map(|v| format!("{v:.2}")).collect::<Vec<_>>().join(" "),
            tt(rc.xl)
        );
        set_label(cx, &tlw, ids!(sec_sweep.ax), &ax, l);
        let sttxt = if app.real_grid.is_some() {
            let rows = app.real_grid.as_ref().map(|g| g.len()).unwrap_or(0);
            format!("{} · {rows} rows", tt("real sweep"))
        } else { match app.st.sweep_state {
            SweepState::Idle => tt("No surface yet — run the sweep from the left rail. 40 cells · 12 episodes each."),
            SweepState::Running => tf("sweeping {0}/40", &[&app.st.sweep_at.to_string()]),
            SweepState::Aborted => tf("aborted at {0}/40 — partial kept", &[&app.st.sweep_at.to_string()]),
            SweepState::Complete => tt("complete"),
        }};
        set_chip(cx, &tlw, ids!(sec_sweep.chips.st_chip), &sttxt, None, false, l);
        let pass = tf("{0}% of measured cells pass", &[&app.st.sweep_pass().to_string()]);
        set_chip(cx, &tlw, ids!(sec_sweep.chips.pass_chip), &pass, None, false, l);
        let has_cell = app.st.sel.cell.is_some() && app.st.sweep_grid.is_some();
        let cc = tlw.view(cx, ids!(sec_sweep.chips.cell_chip));
        if !cc.is_empty() {
            cc.set_visible(cx, has_cell);
        }
        if let (Some((ri, ci)), Some(grid)) = (app.st.sel.cell, app.st.sweep_grid.as_ref()) {
            let v = grid[ri][ci];
            let vtxt = v.map(|v| format!("{v:.2}")).unwrap_or_else(|| tt("unmeasured"));
            let ctxt = tf("gain {0} · friction {1} → {2}", &[&format!("{:.2}", rc.ga[ci]), &format!("{:.2}", rc.fr[ri]), &vtxt]);
            set_chip(cx, &tlw, ids!(sec_sweep.chips.cell_chip), &ctxt, None, false, l);
        }
        set_btn(cx, &tlw, ids!(sec_sweep.chips.b_cell_inspect), &tt("open in Inspect"), has_cell, false, true, l);
        set_btn(cx, &tlw, ids!(sec_sweep.chips.b_cell_scene), &tt("save as scene"), has_cell, false, true, l);
        set_label(cx, &tlw, ids!(sec_sweep.chips.grey_note), &tt("grey = unmeasured, not zero"), l);
    }
    if deploy_v {
        let g = &app.st.dg;
        let gtone = |gate: &Gate| match gate {
            Gate::NotRun => SegTone::Sink,
            Gate::Running(_) => SegTone::Cy,
            Gate::Pass { .. } => SegTone::Ok,
            Gate::Fail { .. } => SegTone::Hot,
        };
        let live = g.armed.as_ref().and_then(|id| app.st.deploy(id));
        let mut segs = vec![
            (1.0, gtone(&g.export)),
            (1.0, gtone(&g.compat)),
            (1.0, gtone(&g.sim2sim)),
            (1.0, gtone(&g.dryrun)),
            (1.0, if g.chk_ok() { SegTone::Ok } else { SegTone::Sink }),
        ];
        segs.push((1.0, match live {
            Some(d) if d.stage == 2 => SegTone::Ok,
            Some(_) => SegTone::Vio,
            None => SegTone::Sink,
        }));
        tlw.segsbar(cx, ids!(sec_deploy.gsegs)).set(cx, segs, l);
        let labels = [tt("export"), tt("compat"), tt("sim-to-sim"), tt("dry-run"), tt("checklist"), tt("rollout")].join(" · ");
        set_label(cx, &tlw, ids!(sec_deploy.glabels), &labels, l);
        let note = match live {
            Some(d) => tf("live on {0} at {1}% torque cap — E-STOP is in the right rail", &[&d.target, &[40, 70, 100][d.stage].to_string()]),
            None => tt("grey = not run · cyan = running · green = passed — a gate certifies this exact pair"),
        };
        set_label(cx, &tlw, ids!(sec_deploy.gnote), &note, l);
    }
    if note_v {
        set_label(cx, &tlw, ids!(sec_note.nl), &tt("Import runs a six-step gate: parse, assets, structure, rehearse, map, commit. A URDF that loads is not a URDF that trains."), l);
    }
}

pub fn route_stage_tl(app: &mut App, cx: &mut Cx, actions: &Actions) {
    let stage = app.ui.widget(cx, ids!(stage));
    if !stage.is_empty() && stage.button(cx, ids!(banner.bl.clr)).clicked(actions) {
        app.dispatch_named(cx, "clear-cell", "");
        return;
    }
    let tlw = app.ui.widget(cx, ids!(tl));
    if tlw.is_empty() {
        return;
    }
    if let Some(f) = tlw.scrub(cx, ids!(sec_frames.scrub)).seeked(actions) {
        app.st.live.frame = (f * (app.st.live.frames.saturating_sub(1)) as f64).round() as u32;
        app.st.live.playing = false;
        sync_stage_tl(app, cx);
        app.redraw_ui(cx);
        return;
    }
    if tlw.button(cx, ids!(sec_frames.row.b_start)).clicked(actions) {
        app.st.live.frame = 0;
        app.st.live.playing = true;
        app.sync_all(cx);
        return;
    }
    if tlw.button(cx, ids!(sec_frames.row.b_back)).clicked(actions) {
        app.st.live.playing = false;
        app.st.live.frame = app.st.live.frame.saturating_sub(1);
        app.sync_all(cx);
        return;
    }
    if tlw.button(cx, ids!(sec_frames.row.b_play)).clicked(actions) {
        app.st.live.playing = !app.st.live.playing;
        app.sync_all(cx);
        return;
    }
    if tlw.button(cx, ids!(sec_frames.row.b_fwd)).clicked(actions) {
        app.st.live.playing = false;
        app.st.live.frame = (app.st.live.frame + 1).min(app.st.live.frames.saturating_sub(1));
        app.sync_all(cx);
        return;
    }
    if tlw.button(cx, ids!(sec_frames.row.b_resim)).clicked(actions) {
        app.st.resim();
        app.sync_all(cx);
        return;
    }
    if tlw.button(cx, ids!(sec_frames.row.b_record)).clicked(actions) {
        app.dispatch_named(cx, "record-probe", "");
        return;
    }
    // sweep heat cell
    let heat = tlw.heat(cx, ids!(sec_sweep.heat));
    if !heat.is_empty() {
        if let Some(item) = actions.find_widget_action(heat.widget_uid()) {
            if let HeatAction::Cell(ri, ci) = item.cast::<HeatAction>() {
                let idx = ri * 8 + ci;
                if app.st.sweep_grid.is_some() && idx < app.st.sweep_at {
                    app.st.sel.cell = Some((ri, ci));
                    app.sync_all(cx);
                    return;
                }
            }
        }
    }
    if tlw.button(cx, ids!(sec_sweep.chips.b_cell_inspect)).clicked(actions) {
        app.dispatch_named(cx, "cell-inspect", "");
        return;
    }
    if tlw.button(cx, ids!(sec_sweep.chips.b_cell_scene)).clicked(actions) {
        app.dispatch_named(cx, "cell-scene", "");
    }
}

// ------------------------------------------------------------- composer --

pub fn sync_composer(app: &mut App, cx: &mut Cx) {
    if app.st.mode != Mode::Scenes {
        return;
    }
    let l = app.light();
    let lang = app.st.lang;
    let tt = |k: &str| tr(lang, k).to_string();
    let comp = app.ui.widget(cx, ids!(composer));
    if comp.is_empty() {
        return;
    }
    {
        let mut p = app.ui.view(cx, ids!(composer));
        script_apply_eval!(cx, p, { draw_bg +: { light: #(l as f64) } });
    }
    let Some(set) = app.st.sel.set.as_ref().and_then(|id| app.st.set(id)).cloned() else { return };
    set_label(cx, &comp, ids!(cap), &format!("{} · {}", caps(&tt("Set")), set.name), l);
    let rpaths = [ids!(rung0), ids!(rung1), ids!(rung2), ids!(rung3), ids!(rung4), ids!(rung5)];
    for (i, rp) in rpaths.iter().enumerate() {
        let rung = comp.widget(cx, *rp);
        if rung.is_empty() {
            continue;
        }
        match set.members.get(i) {
            Some(mid) => {
                rung.set_visible(cx, true);
                match app.st.scene(mid) {
                    Some(sc) => {
                        set_label(cx, &rung, ids!(n), &sc.name, l);
                        set_label(cx, &rung, ids!(h), &format!("{:.2}", sc.stance), l);
                    }
                    None => {
                        set_label(cx, &rung, ids!(n), &tt("deleted"), l);
                        set_label(cx, &rung, ids!(h), "—", l);
                    }
                }
                for bp in [ids!(l), ids!(r), ids!(x)] {
                    let b = rung.button(cx, bp);
                    if !b.is_empty() {
                        b.set_visible(cx, true);
                    }
                }
            }
            None => rung.set_visible(cx, false),
        }
    }
    set_btn(cx, &comp, ids!(b_add), &tt("+ add selected"), true, false, true, l);
    set_btn(cx, &comp, ids!(b_rename), &tt("rename"), true, false, true, l);
    set_btn(cx, &comp, ids!(b_dup), &tt("duplicate set"), true, false, true, l);
    set_chip(cx, &comp, ids!(budget), &tt("400 it × 6 rounds"), None, false, l);
    set_btn(cx, &comp, ids!(b_train), &tt("Train this set →"), true, true, true, l);
}

pub fn route_composer(app: &mut App, cx: &mut Cx, actions: &Actions) {
    let comp = app.ui.widget(cx, ids!(composer));
    if comp.is_empty() || app.st.mode != Mode::Scenes {
        return;
    }
    if comp.button(cx, ids!(b_add)).clicked(actions) {
        app.st.comp_add();
        app.sync_all(cx);
        return;
    }
    if comp.button(cx, ids!(b_rename)).clicked(actions) {
        app.dispatch_named(cx, "set-rename", "");
        return;
    }
    if comp.button(cx, ids!(b_dup)).clicked(actions) {
        app.st.set_dup();
        app.sync_all(cx);
        return;
    }
    if comp.button(cx, ids!(b_train)).clicked(actions) {
        app.dispatch_named(cx, "train-set", "");
        return;
    }
    let rpaths = [ids!(rung0), ids!(rung1), ids!(rung2), ids!(rung3), ids!(rung4), ids!(rung5)];
    for (i, rp) in rpaths.iter().enumerate() {
        let rung = comp.widget(cx, *rp);
        if rung.is_empty() {
            continue;
        }
        if rung.button(cx, ids!(l)).clicked(actions) {
            app.st.comp_move(i, false);
            app.sync_all(cx);
            return;
        }
        if rung.button(cx, ids!(r)).clicked(actions) {
            app.st.comp_move(i, true);
            app.sync_all(cx);
            return;
        }
        if rung.button(cx, ids!(x)).clicked(actions) {
            app.st.comp_rm(i);
            app.sync_all(cx);
            return;
        }
    }
}

/// 120ms path: update only the scrub fill + frame counter — no VM applies.
pub fn sync_transport_fast(app: &mut App, cx: &mut Cx) {
    if !matches!(app.st.mode, Mode::Scenes | Mode::Inspect) {
        return;
    }
    let tlw = app.ui.widget(cx, ids!(tl));
    if tlw.is_empty() {
        return;
    }
    let l = app.light();
    let frac = app.st.live.frame as f64 / app.st.live.frames.max(1) as f64;
    tlw.scrub(cx, ids!(sec_frames.scrub)).set(cx, frac, l);
    let fl = format!("{} / {}", app.st.live.frame, app.st.live.frames.saturating_sub(1));
    let lb = tlw.label(cx, ids!(sec_frames.row.frame_lbl));
    if !lb.is_empty() {
        lb.set_text(cx, &fl);
    }
}

/// 120ms path during a running sweep: heat cells + the two status chips.
pub fn sync_sweep_fast(app: &mut App, cx: &mut Cx) {
    if app.st.mode != Mode::Validate {
        return;
    }
    let tlw = app.ui.widget(cx, ids!(tl));
    if tlw.is_empty() {
        return;
    }
    let l = app.light();
    let lang = app.st.lang;
    tlw.heat(cx, ids!(sec_sweep.heat)).set(cx, app.st.sweep_grid.as_ref(), app.st.sweep_at, app.st.sel.cell, l, false);
    let sttxt = match app.st.sweep_state {
        SweepState::Running => trf(lang, "sweeping {0}/40", &[&app.st.sweep_at.to_string()]),
        SweepState::Complete => tr(lang, "complete").to_string(),
        SweepState::Aborted => trf(lang, "aborted at {0}/40 — partial kept", &[&app.st.sweep_at.to_string()]),
        SweepState::Idle => return,
    };
    set_chip(cx, &tlw, ids!(sec_sweep.chips.st_chip), &sttxt, None, false, l);
    let pass = trf(lang, "{0}% of measured cells pass", &[&app.st.sweep_pass().to_string()]);
    set_chip(cx, &tlw, ids!(sec_sweep.chips.pass_chip), &pass, None, false, l);
}

// --------------------------------------------------------------- toasts --

pub fn sync_toasts(app: &mut App, cx: &mut Cx) {
    let l = app.light();
    let paths = [ids!(toasts.t0), ids!(toasts.t1), ids!(toasts.t2)];
    let n = app.st.toasts.len();
    let show: Vec<&Toast> = app.st.toasts.iter().skip(n.saturating_sub(3)).collect();
    for (i, p) in paths.iter().enumerate() {
        let tv = app.ui.widget(cx, *p);
        if tv.is_empty() {
            continue;
        }
        match show.get(i) {
            Some(toast) => {
                tv.set_visible(cx, true);
                set_label(cx, &tv, ids!(lbl), &toast.text, l);
                let undo_lbl = if app.st.lang == 1 { "撤销" } else { "Undo" };
                set_btn(cx, &tv, ids!(undo), undo_lbl, toast.undo.is_some(), false, true, l);
                let mut bg = tv.clone();
                script_apply_eval!(cx, bg, { draw_bg +: { light: #(l as f64) } });
            }
            None => tv.set_visible(cx, false),
        }
    }
}

pub fn route_toasts(app: &mut App, cx: &mut Cx, actions: &Actions) {
    let paths = [ids!(toasts.t0), ids!(toasts.t1), ids!(toasts.t2)];
    let n = app.st.toasts.len();
    let base = n.saturating_sub(3);
    for (i, p) in paths.iter().enumerate() {
        let tv = app.ui.widget(cx, *p);
        if tv.is_empty() {
            continue;
        }
        if tv.button(cx, ids!(undo)).clicked(actions) {
            let idx = base + i;
            if idx < app.st.toasts.len() {
                let toast = app.st.toasts.remove(idx);
                if let Some(u) = toast.undo {
                    app.st.apply_undo(u);
                }
                app.sync_all(cx);
            }
            return;
        }
    }
}
