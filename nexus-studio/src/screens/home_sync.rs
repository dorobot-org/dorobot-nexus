//! Home dashboard: stat row, live-session hero, and the five cards.

use super::*;
use crate::kit::*;

pub fn sync_home(app: &mut App, cx: &mut Cx) {
    if app.st.mode != Mode::Home {
        return;
    }
    let l = app.light();
    let lang = app.st.lang;
    let tt = |k: &str| tr(lang, k).to_string();
    let tf = |k: &str, a: &[&str]| trf(lang, k, a);
    let hp = app.ui.widget(cx, ids!(home));
    if hp.is_empty() {
        return;
    }
    // ------------------------------------------------------------ statrow --
    let st = &app.st;
    let total_steps: f64 = st.runs.iter().map(|r| r.steps).sum();
    let ck_n: usize = st.runs.iter().map(|r| r.ckpts.len()).sum();
    let best = st.runs.iter().filter_map(|r| r.best).fold(f64::MIN, f64::max);
    let live_run = st.runs.iter().any(|r| r.state == RunState::Running);
    let live_deps = st.deploys.iter().filter(|d| d.state == DepState::Live).count();
    let stats: Vec<(String, String, String)> = vec![
        (tt("runs"), st.runs.len().to_string(), if live_run { tt("1 live") } else { tt("none live") }),
        (tt("env-steps"), format!("{total_steps:.2}M"), String::new()),
        (tt("checkpoints"), ck_n.to_string(), String::new()),
        (tt("best reward"), if best > f64::MIN { format!("{best:.3}") } else { "—".into() }, String::new()),
        (tt("scenes"), st.scenes.len().to_string(), String::new()),
        (tt("sets"), st.sets.len().to_string(), String::new()),
        (tt("robots"), st.robots.len().to_string(), String::new()),
        (tt("recordings"), st.recordings.len().to_string(), String::new()),
        (tt("deploys"), st.deploys.len().to_string(), if live_deps > 0 { tf("{0} live", &[&live_deps.to_string()]) } else { tt("none live") }),
    ];
    let spaths = [
        ids!(statrow.s0), ids!(statrow.s1), ids!(statrow.s2), ids!(statrow.s3), ids!(statrow.s4),
        ids!(statrow.s5), ids!(statrow.s6), ids!(statrow.s7), ids!(statrow.s8),
    ];
    for (i, sp) in spaths.iter().enumerate() {
        let tile = hp.widget(cx, *sp);
        if tile.is_empty() {
            continue;
        }
        let (k, v, sub) = &stats[i];
        let kk = if sub.is_empty() { caps(k) } else { format!("{} · {}", caps(k), sub) };
        set_label(cx, &tile, ids!(k), &kk, l);
        set_label(cx, &tile, ids!(v), v, l);
        let mut bg = tile.clone();
        script_apply_eval!(cx, bg, { draw_bg +: { light: #(l as f64) } });
    }
    // --------------------------------------------------------------- hero --
    let hero = hp.widget(cx, ids!(hero));
    if !hero.is_empty() {
        let live = st.runs.iter().find(|r| r.state == RunState::Running).or_else(|| st.runs.iter().find(|r| r.state == RunState::Paused));
        set_label(cx, &hero, ids!(head.cap), &caps(&tt("Training session")), l);
        set_btn(cx, &hero, ids!(head.open), &tt("open →"), true, false, true, l);
        match live {
            Some(r) => {
                let goal = r.snapshot.cfg.get(r.stage).map(|c| c.stance).unwrap_or(0.62);
                let gap = st.live.now - goal;
                let bits: [(String, String); 4] = [
                    (tt("policy"), r.name.clone()),
                    (tt("goal"), format!("{goal:.2} m")),
                    (tt("now"), format!("{:.3} m", st.live.now)),
                    (tt("gap"), format!("{}{:.3}", if gap >= 0.0 { "+" } else { "−" }, gap.abs())),
                ];
                for (i, bp) in [ids!(bits.b0), ids!(bits.b1), ids!(bits.b2), ids!(bits.b3)].iter().enumerate() {
                    let b = hero.widget(cx, *bp);
                    if b.is_empty() {
                        continue;
                    }
                    set_label(cx, &b, ids!(k), &caps(&bits[i].0), l);
                    set_label(cx, &b, ids!(v), &bits[i].1, l);
                }
                hero.spark(cx, ids!(goal_chart)).set(cx, &st.live.now_hist, Some(goal), 1, l);
                set_label(cx, &hero, ids!(goal_note), &tt("stance · actual chasing target"), l);
                let mut segs: Vec<(f64, SegTone)> = vec![];
                for i in 0..r.stages.len() {
                    segs.push((1.0, if i < r.stage { SegTone::Ok } else if i == r.stage { SegTone::Vio } else { SegTone::Sink }));
                }
                hero.segsbar(cx, ids!(hsegs)).set(cx, segs, l);
                let seg_note = tf("stage {0}/{1} · round {2}/6 · iter {3}/{4}", &[
                    &(r.stage + 1).to_string(),
                    &r.stages.len().to_string(),
                    &r.round.to_string(),
                    &r.iter.to_string(),
                    &r.iters_per.to_string(),
                ]);
                set_label(cx, &hero, ids!(hseg_note), &seg_note, l);
                hero.spark(cx, ids!(rew_spark)).set(cx, &st.live.hist, None, 0, l);
                set_label(cx, &hero, ids!(rew_note), &tt("reward · last 90 intervals"), l);
                let hvals = [
                    (tt("reward"), format!("{:.4}", st.live.reward)),
                    (tt("falls"), format!("{:.1}%", st.live.falls)),
                    ("steps/s".to_string(), "3,151".to_string()),
                ];
                for (i, tp) in [ids!(htiles.h0), ids!(htiles.h1), ids!(htiles.h2)].iter().enumerate() {
                    let tile = hero.widget(cx, *tp);
                    if tile.is_empty() {
                        continue;
                    }
                    set_label(cx, &tile, ids!(k), &caps(&hvals[i].0), l);
                    set_label(cx, &tile, ids!(v), &hvals[i].1, l);
                    let mut bg = tile.clone();
                    script_apply_eval!(cx, bg, { draw_bg +: { light: #(l as f64) } });
                }
            }
            None => {
                set_label(cx, &hero, ids!(goal_note), &tt("Nothing running. Launch from a scene set — Scenes → Train this set."), l);
            }
        }
        let mut bg = hero.clone();
        script_apply_eval!(cx, bg, { draw_bg +: { light: #(l as f64) } });
    }
    // -------------------------------------------------------------- cards --
    sync_card_rows(app, cx, ids!(cards.colA.card_runs), &tt("Recent runs"), "runs", l);
    sync_card_rows(app, cx, ids!(cards.colA.card_sets), &tt("Scene sets"), "sets", l);
    sync_card_rows(app, cx, ids!(cards.colB.card_robots), &tt("Robots"), "robots", l);
    sync_card_rows(app, cx, ids!(cards.colB.card_val), &tt("Validation"), "val", l);
    sync_card_rows(app, cx, ids!(cards.colC.card_deps), &tt("Deployments"), "deps", l);
}

fn sync_card_rows(app: &mut App, cx: &mut Cx, path: &[LiveId], title: &str, kind: &str, l: f32) {
    let lang = app.st.lang;
    let tt = |k: &str| tr(lang, k).to_string();
    let tf = |k: &str, a: &[&str]| trf(lang, k, a);
    let hp = app.ui.widget(cx, ids!(home));
    let card = hp.widget(cx, path);
    if card.is_empty() {
        return;
    }
    set_label(cx, &card, ids!(head.cap), &caps(title), l);
    set_btn(cx, &card, ids!(head.open), &tt("open →"), true, false, true, l);
    let rpaths = [ids!(row0), ids!(row1), ids!(row2), ids!(row3)];
    let mut rows: Vec<(String, String, Option<(String, f64)>, bool)> = vec![];
    let mut fracs: Vec<f64> = vec![];
    let mut extra: Option<String> = None;
    let mut heat = false;
    let st = &app.st;
    match kind {
        "runs" => {
            let mx = st.runs.iter().map(|r| r.steps).fold(0.01, f64::max);
            for r in st.runs.iter().filter(|r| !r.archived).take(4) {
                let tone = match r.state {
                    RunState::Running => 0.0,
                    RunState::Completed => 1.0,
                    RunState::Failed => 3.0,
                    _ => 2.0,
                };
                fracs.push(r.steps / mx);
                rows.push((r.name.clone(), format!("{:.2}M", r.steps), Some((tt(r.state.key()), tone)), false));
            }
            extra = Some(tt("bar = env-steps relative to the largest run"));
        }
        "sets" => {
            for s in st.sets.iter().take(4) {
                let members = s
                    .members
                    .iter()
                    .take(4)
                    .map(|id| st.scene(id).map(|sc| format!("{} {:.2}", sc.name, sc.stance)).unwrap_or_else(|| tt("deleted")))
                    .collect::<Vec<_>>()
                    .join(" · ");
                rows.push((s.name.clone(), tf("{0} scenes", &[&s.members.len().to_string()]), None, false));
                if extra.is_none() {
                    extra = Some(members);
                }
            }
        }
        "robots" => {
            for r in st.robots.iter().take(4) {
                rows.push((r.name.clone(), tf("{0} dof", &[&r.movable.to_string()]), Some((tt("validated"), 1.0)), false));
            }
            extra = Some(tt("import gate: parse · assets · structure · rehearse · map"));
        }
        "val" => {
            let s = match st.sweep_state {
                SweepState::Idle => tt("no sweep yet"),
                SweepState::Running => tf("sweeping {0}/40", &[&st.sweep_at.to_string()]),
                SweepState::Aborted => tf("aborted {0}/40", &[&st.sweep_at.to_string()]),
                SweepState::Complete => tf("{0}% of cells pass", &[&st.sweep_pass().to_string()]),
            };
            extra = Some(format!("{s} · {}", tt("sim-to-sim gap 11%")));
            heat = st.sweep_grid.is_some();
        }
        "deps" => {
            for d in st.deploys.iter().take(4) {
                let tone = match d.state {
                    DepState::Live => 0.0,
                    DepState::Aborted => 3.0,
                    DepState::RolledBack => 2.0,
                    _ => 4.0,
                };
                rows.push((d.name.clone(), format!("{} → {}", d.pname, d.target), Some((tt(d.state.key()), tone)), false));
            }
            if rows.is_empty() {
                extra = Some(tt("no deployments yet — the Deploy tab gates the path to hardware"));
            }
        }
        _ => {}
    }
    for (i, rp) in rpaths.iter().enumerate() {
        let row = card.widget(cx, *rp);
        if row.is_empty() {
            continue;
        }
        match rows.get(i) {
            Some((nm, tg, pill, on)) => {
                row.set_visible(cx, true);
                set_label(cx, &row, ids!(nm), nm, l);
                set_label(cx, &row, ids!(tg), tg, l);
                let onf = if *on { 1.0 } else { 0.0 };
                let fr = fracs.get(i).copied().unwrap_or(0.0);
                let mut rw = card.widget(cx, *rp);
                script_apply_eval!(cx, rw, { draw_bg +: { on: #(onf) light: #(l as f64) frac: #(fr) } });
                match pill {
                    Some((pt, tone)) => set_pill(cx, &row, pt, *tone, l),
                    None => {
                        let pw = row.widget(cx, ids!(pill));
                        if !pw.is_empty() {
                            pw.set_visible(cx, false);
                        }
                    }
                }
            }
            None => row.set_visible(cx, false),
        }
    }
    let ex = card.widget(cx, ids!(extra));
    if !ex.is_empty() {
        ex.set_visible(cx, extra.is_some());
        if let Some(e) = &extra {
            set_label(cx, &card, ids!(extra), e, l);
        }
    }
    let hw = card.view(cx, ids!(mini_heatw));
    if !hw.is_empty() {
        hw.set_visible(cx, heat);
        if heat {
            card.heat(cx, ids!(mini_heatw.mini_heat)).set(cx, app.st.sweep_grid.as_ref(), app.st.sweep_at, None, l, true);
        }
    }
    let mut bg = card.clone();
    script_apply_eval!(cx, bg, { draw_bg +: { light: #(l as f64) } });
}

pub fn route_home(app: &mut App, cx: &mut Cx, actions: &Actions) {
    if app.st.mode != Mode::Home {
        return;
    }
    let hp = app.ui.widget(cx, ids!(home));
    if hp.is_empty() {
        return;
    }
    if hp.widget(cx, ids!(hero)).button(cx, ids!(head.open)).clicked(actions) {
        app.st.mode = Mode::Train;
        app.sync_all(cx);
        return;
    }
    let cards: [(&[LiveId], Mode, &str); 5] = [
        (ids!(cards.colA.card_runs), Mode::Runs, "runs"),
        (ids!(cards.colA.card_sets), Mode::Scenes, "sets"),
        (ids!(cards.colB.card_robots), Mode::Robots, "robots"),
        (ids!(cards.colB.card_val), Mode::Validate, "val"),
        (ids!(cards.colC.card_deps), Mode::Deploy, "deps"),
    ];
    for (path, mode, kind) in cards {
        let card = hp.widget(cx, path);
        if card.is_empty() {
            continue;
        }
        if card.button(cx, ids!(head.open)).clicked(actions) {
            app.st.mode = mode;
            app.sync_all(cx);
            return;
        }
        for (i, rp) in [ids!(row0), ids!(row1), ids!(row2), ids!(row3)].iter().enumerate() {
            let uid = card.widget(cx, *rp).widget_uid();
            if view_clicked(actions, uid) {
                match kind {
                    "runs" => {
                        if let Some(r) = app.st.runs.iter().filter(|r| !r.archived).nth(i) {
                            app.st.sel.run = Some(r.id.clone());
                            app.st.mode = Mode::Runs;
                        }
                    }
                    "sets" => {
                        if let Some(s) = app.st.sets.get(i) {
                            app.st.sel.set = Some(s.id.clone());
                            if let Some(first) = s.members.iter().find(|m| app.st.scene(m).is_some()) {
                                app.st.sel.scene = Some(first.clone());
                            }
                            app.st.mode = Mode::Scenes;
                        }
                    }
                    "robots" => {
                        if let Some(r) = app.st.robots.get(i) {
                            app.st.sel.robot = Some(r.id.clone());
                            app.st.mode = Mode::Robots;
                        }
                    }
                    "deps" => {
                        if let Some(d) = app.st.deploys.get(i) {
                            app.st.sel.dep = Some(d.id.clone());
                            app.st.mode = Mode::Deploy;
                        }
                    }
                    _ => {}
                }
                app.sync_all(cx);
                return;
            }
        }
    }
}
