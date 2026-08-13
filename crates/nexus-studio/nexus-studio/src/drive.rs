//! UI soak-test driver: a file-based command channel that injects the same
//! actions the pointer would, plus a state-dump ack so an external harness
//! can assert and screenshot deterministically.
//!
//! Protocol: append lines to /tmp/nexus_cmd.txt. Each fast tick the app
//! consumes the whole file, executes every line, then writes
//! /tmp/nexus_state.json containing an `ack` counter and a state summary.
//! The harness waits for `ack` to advance, then captures the window.
//!
//! Commands:
//!   mode <home|scenes|robots|train|inspect|validate|runs|deploy>
//!   act <name>            — dispatcher action ("target-ping", "sweep-run", …)
//!   arg <name:arg>        — composite action ("ck-promote:c4", "sel-dep:d1")
//!   lrow <i>              — click left-rail slot i's row
//!   lact <i> <j>          — click left-rail slot i's action j
//!   slider <field> <val>  — commit a scene slider
//!   chk <i> <0|1>         — deploy checklist box
//!   opt <i> | ok | cancel | danger | back — modal interactions
//!   frame <n> | play | pause | lang | theme | esc

use crate::state::*;
use crate::App;
use makepad_widgets::*;

pub const CMD_PATH: &str = "/tmp/nexus_cmd.txt";
pub const STATE_PATH: &str = "/tmp/nexus_state.json";

pub fn poll(app: &mut App, cx: &mut Cx) {
    let Ok(text) = std::fs::read_to_string(CMD_PATH) else { return };
    if text.trim().is_empty() {
        return;
    }
    let _ = std::fs::write(CMD_PATH, "");
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        exec(app, cx, line);
    }
    app.drive_ack += 1;
    dump_state(app);
    app.sync_all(cx);
}

fn exec(app: &mut App, cx: &mut Cx, line: &str) {
    let mut it = line.splitn(3, ' ');
    let cmd = it.next().unwrap_or("");
    let a1 = it.next().unwrap_or("");
    let a2 = it.next().unwrap_or("");
    match cmd {
        "mode" => {
            let m = match a1 {
                "home" => Mode::Home,
                "scenes" => Mode::Scenes,
                "robots" => Mode::Robots,
                "train" => Mode::Train,
                "inspect" => Mode::Inspect,
                "validate" => Mode::Validate,
                "runs" => Mode::Runs,
                "deploy" => Mode::Deploy,
                _ => return,
            };
            app.st.mode = m;
            app.sync_all(cx);
        }
        "act" => app.dispatch_named(cx, a1, a2),
        "arg" => app.dispatch_arg(cx, a1),
        "lrow" => {
            if let Ok(i) = a1.parse::<usize>() {
                if let Some(spec) = app.lmap.get(i) {
                    let click = spec.click.clone();
                    app.dispatch_l(cx, &click);
                }
            }
        }
        "lact" => {
            if let (Ok(i), Ok(j)) = (a1.parse::<usize>(), a2.parse::<usize>()) {
                if let Some(spec) = app.lmap.get(i) {
                    if let Some(act) = spec.act_ids.get(j) {
                        let act = act.to_string();
                        let arg = match &spec.click {
                            crate::screens::LAct::Replay(id) => id.clone(),
                            _ => String::new(),
                        };
                        app.dispatch_named(cx, &act, &arg);
                    }
                }
            }
        }
        "slider" => {
            if let Ok(v) = a2.parse::<f64>() {
                let field = a1.to_string();
                app.slider_committed(cx, &field, v);
            }
        }
        "chk" => {
            if let (Ok(i), Ok(v)) = (a1.parse::<usize>(), a2.parse::<u8>()) {
                app.st.dep_chk(i, v == 1);
                app.sync_all(cx);
            }
        }
        "opt" => {
            if let Ok(i) = a1.parse::<usize>() {
                crate::screens::modal_opt(app, cx, i);
            }
        }
        "ok" => crate::screens::modal_ok(app, cx),
        "cancel" => {
            app.st.modal = AppModal::None;
            app.sync_all(cx);
        }
        "danger" => crate::screens::modal_danger(app, cx),
        "back" => crate::screens::modal_back(app, cx),
        "esc" => {
            app.st.modal = AppModal::None;
            app.sync_all(cx);
        }
        "frame" => {
            if let Ok(n) = a1.parse::<u32>() {
                app.st.live.frame = n.min(app.st.live.frames.saturating_sub(1));
                app.st.live.playing = false;
            }
        }
        "play" => app.st.live.playing = true,
        "pause" => app.st.live.playing = false,
        "lang" => {
            app.st.lang = 1 - app.st.lang;
            app.sync_all(cx);
        }
        "theme" => {
            app.st.theme = match app.st.theme {
                Theme::Auto => Theme::Light,
                Theme::Light => Theme::Dark,
                Theme::Dark => Theme::Auto,
            };
            app.sync_all(cx);
        }
        _ => {}
    }
}

fn esc_json(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', " ")
}

pub fn dump_state(app: &App) {
    let st = &app.st;
    let gate = |g: &Gate| match g {
        Gate::NotRun => "not-run".to_string(),
        Gate::Running(p) => format!("running:{p}"),
        Gate::Pass { hash, .. } => format!("pass:{hash}"),
        Gate::Fail { .. } => "fail".to_string(),
    };
    let toast = st.toasts.last().map(|t| t.text.clone()).unwrap_or_default();
    let json = format!(
        concat!(
            "{{\n",
            "  \"ack\": {},\n",
            "  \"mode\": \"{:?}\",\n",
            "  \"modal\": \"{}\",\n",
            "  \"lang\": {},\n",
            "  \"theme\": \"{:?}\",\n",
            "  \"dirty\": {},\n",
            "  \"scenes\": {},\n",
            "  \"runs\": {},\n",
            "  \"recordings\": {},\n",
            "  \"deploys\": {},\n",
            "  \"sel_scene\": \"{}\",\n",
            "  \"sel_run\": \"{}\",\n",
            "  \"sel_dep\": \"{}\",\n",
            "  \"frame\": {},\n",
            "  \"frames\": {},\n",
            "  \"gates\": [\"{}\", \"{}\", \"{}\", \"{}\"],\n",
            "  \"chk\": [{}, {}, {}, {}],\n",
            "  \"armed\": \"{}\",\n",
            "  \"dwell\": {},\n",
            "  \"sweep_state\": \"{:?}\",\n",
            "  \"sweep_at\": {},\n",
            "  \"real_proc\": {},\n",
            "  \"real_lines\": {},\n",
            "  \"replay\": {},\n",
            "  \"last_toast\": \"{}\"\n",
            "}}\n"
        ),
        app.drive_ack,
        st.mode,
        esc_json(&format!("{:?}", st.modal)),
        st.lang,
        st.theme,
        st.dirty,
        st.scenes.len(),
        st.runs.len(),
        st.recordings.len(),
        st.deploys.len(),
        st.sel.scene.clone().unwrap_or_default(),
        st.sel.run.clone().unwrap_or_default(),
        st.sel.dep.clone().unwrap_or_default(),
        st.live.frame,
        st.live.frames,
        gate(&st.dg.export),
        gate(&st.dg.compat),
        gate(&st.dg.sim2sim),
        gate(&st.dg.dryrun),
        st.dg.chk[0], st.dg.chk[1], st.dg.chk[2], st.dg.chk[3],
        st.dg.armed.clone().unwrap_or_default(),
        st.dg_live.dwell,
        st.sweep_state,
        st.sweep_at,
        app.real_proc.is_some(),
        app.real_lines.len(),
        app.replay.is_some(),
        esc_json(&toast),
    );
    let _ = std::fs::write(STATE_PATH, json);
}
