//! nexus-studio — makepad port of the frozen dorobot-nexus web mockup.
//! Eight workspaces, factory-industrial theme, EN/中文, light/dark.

pub mod actions;
pub mod drive;
pub mod nexus;
pub mod i18n;
pub mod screens;
pub mod state;
/// Tokens and the widget kit moved to the shared `dorobot-ux` crate, which
/// dorobot-studio draws with too. Re-exported under their old paths so the
/// screens keep referring to `crate::tokens` / `crate::kit` unchanged.
pub use dorobot_ux::{kit, tokens};

use crate::i18n::{tr, trf};
use crate::screens::{LSpec, RSpec};
use crate::state::*;
use crate::tokens::pal;
use makepad_widgets::*;

/// makepad's `app_main!` generates the crate's `fn main`, and by the time that
/// reaches our code it has already written to **stdout** via `Cx::init_log` and
/// `init_websockets` — which would put framework chatter inside a JSON stream.
/// Invoking it inside a module demotes that `main` to `shell::main`, leaving
/// the real entry point below to dispatch the engine's headless flags first.
///
/// Both consoles do this, so `nexus-studio --sweep` and `dorobot-nexus --sweep`
/// are the same run: the surfaces live in the engine, not in either UI.
#[allow(dead_code)]
mod shell {
    use super::*;
    app_main!(App);
}

fn main() {
    // Never returns if a headless flag was given.
    nexus_engine::cli::maybe_headless();
    shell::app_main();
}

script_mod! {
    use mod.prelude.widgets.*
    use mod.widgets.*
    use mod.widgets.nx.*

    // A generic left-rail slot: group header, selectable row with an inline
    // action strip, or a note — visibility-picked from Rust each sync.
    let LSlot = View{
        width: Fill height: Fit
        flow: Down
        visible: false
        grp := mod.widgets.nx.Grp{ visible: false }
        row := mod.widgets.nx.NxRow{ visible: false }
        acts := View{
            width: Fill height: Fit
            visible: false
            flow: Right
            spacing: 4.0
            padding: Inset{left: 9. right: 7. top: 2. bottom: 4.}
            a0 := mod.widgets.nx.Mini{ visible: false }
            a1 := mod.widgets.nx.Mini{ visible: false }
            a2 := mod.widgets.nx.MiniHot{ visible: false }
        }
        note := View{
            width: Fill height: Fit
            visible: false
            padding: Inset{left: 9. right: 7. top: 4. bottom: 4.}
            lbl := mod.widgets.nx.BodyLbl{}
        }
    }

    let ModeTab = Button{
        width: Fit height: Fit
        padding: Inset{left: 12. right: 12. top: 8. bottom: 8.}
        text: "Mode"
        draw_text +: {
            light: instance(0.0)
            on: instance(0.0)
            text_style: mod.widgets.nx.T_BODY{}
            get_color: fn() {
                let idle = mix(#x948781, #x5E564E, self.light)
                let ink = mix(#xF2F0EC, #x161413, self.light)
                return mix(idle, ink, max(self.on, self.hover))
            }
        }
        draw_bg +: {
            light: instance(0.0)
            on: instance(0.0)
            pixel: fn() {
                let sdf = Sdf2d.viewport(self.pos * self.rect_size)
                let vio = mix(#xEF6F2E, #xD15010, self.light)
                sdf.rect(0.0, self.rect_size.y - 2.0, self.rect_size.x, 2.0)
                sdf.fill(mix(vec4(0.0,0.0,0.0,0.0), vio, self.on))
                return sdf.result
            }
        }
    }

    startup() do #(App::script_component(vm)){
        ui: Root{
            main_window := Window{
                window.title: "dorobot-nexus"
                window.inner_size: vec2(1500, 980)
                pass.clear_color: #x0A0908

                body +: {
                    width: Fill
                    height: Fill
                    flow: Overlay

                    root_bg := mod.widgets.nx.Ground{
                        flow: Down

                        // ------------------------------------------ chrome --
                        modes := View{
                            width: Fill height: Fit
                            flow: Right
                            align: Align{y: 0.5}
                            spacing: 2.0
                            padding: Inset{left: 10. right: 12. top: 2. bottom: 0.}
                            show_bg: true
                            draw_bg +: {
                                light: instance(0.0)
                                pixel: fn() {
                                    let base = mix(#x141312, #xFBFAF8, self.light)
                                    let edge = mix(#x2A2725, #xD8D4CF, self.light)
                                    let t = step(self.rect_size.y - 1.0, self.pos.y * self.rect_size.y)
                                    return mix(base, edge, t)
                                }
                            }
                            tab0 := ModeTab{}
                            tab1 := ModeTab{}
                            tab2 := ModeTab{}
                            tab3 := ModeTab{}
                            tab4 := ModeTab{}
                            tab5 := ModeTab{}
                            tab6 := ModeTab{}
                            tab7 := ModeTab{}
                            Filler{}
                            c_state := mod.widgets.nx.ChipV{}
                            c_steps := mod.widgets.nx.ChipV{}
                            c_dep := mod.widgets.nx.ChipV{ visible: false }
                            lang_btn := mod.widgets.nx.Mini{ text: "中文" }
                            theme_btn := mod.widgets.nx.Mini{ text: "◐" }
                            help_btn := mod.widgets.nx.Mini{ text: "?" }
                        }

                        // ------------------------------------------ canvas --
                        canvas := View{
                            width: Fill height: Fill
                            flow: Right
                            spacing: 8.0
                            padding: Inset{left: 8. right: 8. top: 8. bottom: 8.}

                            rail_l := mod.widgets.nx.Panel{
                                width: 252
                                scroll_bars: ScrollBars{show_scroll_x: false show_scroll_y: true}
                                cap_head := View{
                                    width: Fill height: Fit
                                    flow: Right
                                    align: Align{y: 0.5}
                                    padding: Inset{left: 9. right: 7. top: 7. bottom: 3.}
                                    cap := mod.widgets.nx.Cap{}
                                    Filler{}
                                    cap_btn := mod.widgets.nx.Mini{ visible: false }
                                }
                                filter_wrap := View{
                                    width: Fill height: Fit
                                    visible: false
                                    padding: Inset{left: 9. right: 7. top: 2. bottom: 2.}
                                    filter_in := TextInput{ width: Fill empty_text: "⌕ filter…" }
                                }
                                lslot0 := LSlot{} lslot1 := LSlot{} lslot2 := LSlot{} lslot3 := LSlot{}
                                lslot4 := LSlot{} lslot5 := LSlot{} lslot6 := LSlot{} lslot7 := LSlot{}
                                lslot8 := LSlot{} lslot9 := LSlot{} lslot10 := LSlot{} lslot11 := LSlot{}
                                lslot12 := LSlot{} lslot13 := LSlot{} lslot14 := LSlot{} lslot15 := LSlot{}
                                lslot16 := LSlot{} lslot17 := LSlot{} lslot18 := LSlot{} lslot19 := LSlot{}
                                lslot20 := LSlot{} lslot21 := LSlot{}
                                rail_pad := View{ width: Fill height: 8 }
                            }

                            center := View{
                                width: Fill height: Fill
                                flow: Down
                                spacing: 8.0
                                stage := mod.widgets.screens.Stage{}
                                tl := mod.widgets.screens.TimelinePanel{}
                            }

                            rail_r := mod.widgets.nx.Panel{
                                width: 336
                                scroll_bars: ScrollBars{show_scroll_x: false show_scroll_y: true}
                                rr := mod.widgets.screens.RailR{}
                            }
                        }

                        home := mod.widgets.screens.HomePage{ visible: false }
                        composer := mod.widgets.screens.ComposerStrip{ visible: false }
                    }

                    // -------------------------------------------- overlays --
                    toasts := View{
                        width: Fill height: Fill
                        flow: Down
                        align: Align{x: 0.5 y: 1.0}
                        padding: Inset{bottom: 18.}
                        spacing: 6.0
                        t0 := mod.widgets.screens.ToastV{ visible: false }
                        t1 := mod.widgets.screens.ToastV{ visible: false }
                        t2 := mod.widgets.screens.ToastV{ visible: false }
                    }

                    modal_host := mod.widgets.screens.ModalHost{ visible: false }
                }
            }
        }
    }
}

#[derive(Script, ScriptHook)]
pub struct App {
    #[live]
    ui: WidgetRef,
    #[rust]
    st: Store,
    #[rust]
    fast_timer: Timer,
    #[rust]
    slow_timer: Timer,
    /// Playhead for a loaded rollout, at the rate the data was written for.
    #[rust]
    replay_timer: Timer,
    #[rust]
    lmap: Vec<LSpec>,
    #[rust]
    lhead_act: Option<String>,
    #[rust]
    rmap: Vec<RSpec>,
    #[rust]
    warm_sel: usize,
    #[rust]
    keep_sel: usize,
    #[rust]
    modal_seen: String,
    #[rust]
    toast_seen: usize,
    #[rust]
    pub loaded_urdf: Option<String>,
    #[rust]
    pub real_proc: Option<crate::nexus::RealProc>,
    #[rust]
    pub real_lines: Vec<String>,
    /// MuJoCo sim-to-sim: the verdict, and the motion that produced it.
    #[rust]
    pub mj: Option<crate::nexus::MjRun>,
    /// Cross-sim: the same policy in two implementations of the same physics.
    #[rust]
    pub cross: Option<crate::nexus::CrossRun>,
    #[rust]
    pub replay: Option<crate::nexus::Replay>,
    #[rust]
    pub replay_map: Option<Vec<Option<usize>>>,
    #[rust]
    pub drive_ack: u64,
    #[rust]
    last_fast: Option<std::time::Instant>,
    /// Samples already folded into `st.live` from the real training handle.
    #[rust]
    train_seen: usize,
    /// Which terrain variant the 3D view currently shows (family|amp|slope|seed).
    #[rust]
    terrain_shown: String,
    /// NEXUS_MJCF: the MuJoCo model whose mjvScene the stage renders, kept
    /// loaded so a later step can drive its qpos.
    #[rust]
    pub mjcf: Option<nexus_mujoco::Model>,
    #[rust]
    mjcf_pushed: bool,
    /// Train-mode policy preview in flight: the run's checkpoint rolled out
    /// on the stage's scene, so training replays through the same path a
    /// Validate sim-to-sim does.
    #[rust]
    pub preview: Option<crate::nexus::PreviewJob>,
    /// Checkpoint + scene identity the current preview answers. A new
    /// checkpoint (path or mtime) or a scene edit invalidates it; an
    /// unchanged one does not re-drive.
    #[rust]
    preview_key: String,
}

impl AppMain for App {
    fn script_mod(vm: &mut ScriptVm) -> ScriptValue {
        makepad_widgets::script_mod(vm);
        makepad_urdf_player::makepad_xr::script_mod(vm);
        makepad_urdf_player::script_mod(vm);
        dorobot_ux::script_mod(vm);
        crate::screens::script_mod(vm);
        self::script_mod(vm)
    }

    fn handle_event(&mut self, cx: &mut Cx, event: &Event) {
        // Two wake sources share one cadence: the UI timer (foreground) and
        // the soak thread's SignalToUI (pierces App Nap while backgrounded).
        let timer_fired = self.fast_timer.is_event(event).is_some();
        let signal_fired = matches!(event, Event::Signal);
        if timer_fired || signal_fired {
            let due = self
                .last_fast
                .map(|t| t.elapsed().as_millis() >= 110)
                .unwrap_or(true);
            if due {
                self.last_fast = Some(std::time::Instant::now());
                self.st.fast_tick();
                crate::drive::poll(self, cx);
                self.sync_fast(cx);
            }
        }
        if self.slow_timer.is_event(event).is_some() {
            self.st.train_tick();
            self.sync_slow(cx);
        }
        // A real rollout owns its own playhead: one frame per 40 ms rather than
        // the mock timeline's 3 frames per 120 ms, which delivered recorded
        // motion as 8 visible updates a second.
        if self.replay_timer.is_event(event).is_some()
            && self.replay.is_some()
            && self.st.live.playing
            && matches!(self.st.mode, Mode::Scenes | Mode::Inspect | Mode::Validate | Mode::Train)
        {
            self.st.live.frame = (self.st.live.frame + 1) % self.st.live.frames.max(1);
            self.drive_replay(cx);
            self.ui.redraw(cx);
        }
        self.match_event(cx, event);
        self.ui.handle_event(cx, event, &mut Scope::empty());
    }
}

impl MatchEvent for App {
    fn handle_startup(&mut self, cx: &mut Cx) {
        self.fast_timer = cx.start_interval(0.12);
        self.slow_timer = cx.start_interval(1.1);
        // 25 Hz: the rate tools/lafan1_to_rollout.py resamples motion to, so a
        // recorded clip plays at 1.0x and one frame per tick keeps it smooth.
        self.replay_timer = cx.start_interval(0.04);
        self.st.merge_disk();
        // Opt out of App Nap: backgrounded, macOS otherwise coalesces our
        // timers AND throttles our own threads (the waker included), parking
        // the app solid. This app legitimately works in the background — it
        // monitors real training processes and the soak driver.
        unsafe {
            disable_app_nap();
        }
        // Soak keep-awake: while the harness marker exists, signal the UI
        // thread every 100ms so gates/ticks run at full cadence even when the
        // window is backgrounded (App Nap parks timers otherwise).
        std::thread::spawn(|| loop {
            std::thread::sleep(std::time::Duration::from_millis(100));
            let pending = std::fs::metadata(crate::drive::CMD_PATH).map(|m| m.len() > 0).unwrap_or(false);
            let soak = std::path::Path::new("/tmp/nexus_soak_on").exists();
            if pending || soak {
                SignalToUI::set_ui_signal();
                // The signal is only an AtomicBool — it is read when the loop
                // wakes, but wakes nothing. Post makepad's own "event loop
                // unblocker" (NSApplicationDefined) so a parked NSApp run
                // loop actually resumes; without this, App Nap freezes the
                // app solid while backgrounded.
                unsafe {
                    wake_nsapp();
                }
            }
        });
        self.st.modal = AppModal::Tour;
        self.sync_all(cx);
        // Launched as an observer: attach to the named run immediately and
        // land on Train, so the window opens onto the live curves instead of
        // a mock — no click required.
        if std::env::var("DOROBOT_ATTACH_LOG").is_ok() {
            self.st.modal = AppModal::None;
            self.st.mode = Mode::Train;
            self.start_real_train(cx);
        }
    }

    fn handle_actions(&mut self, cx: &mut Cx, actions: &Actions) {
        self.route_chrome(cx, actions);
        self.route_rails(cx, actions);
        self.route_modal(cx, actions);
        if self.st_dirty_check() {
            self.sync_all(cx);
        }
    }

    fn handle_key_down(&mut self, cx: &mut Cx, e: &KeyEvent) {
        match e.key_code {
            KeyCode::Escape => {
                if self.st.modal != AppModal::None {
                    self.st.modal = AppModal::None;
                    self.sync_all(cx);
                }
            }
            KeyCode::ArrowRight | KeyCode::ArrowLeft => {
                if self.st.modal != AppModal::None {
                    return;
                }
                let fwd = e.key_code == KeyCode::ArrowRight;
                if e.modifiers.control {
                    let i = MODES.iter().position(|m| *m == self.st.mode).unwrap_or(0);
                    let n = MODES.len();
                    let j = if fwd { (i + 1) % n } else { (i + n - 1) % n };
                    self.st.mode = MODES[j];
                    self.sync_all(cx);
                } else if matches!(self.st.mode, Mode::Scenes | Mode::Inspect) {
                    self.st.live.playing = false;
                    if fwd {
                        self.st.live.frame = (self.st.live.frame + 3).min(self.st.live.frames.saturating_sub(1));
                    } else {
                        self.st.live.frame = self.st.live.frame.saturating_sub(3);
                    }
                    crate::screens::sync_stage_tl(self, cx);
                    self.ui.redraw(cx);
                }
            }
            KeyCode::Home | KeyCode::End => {
                if self.st.modal == AppModal::None && matches!(self.st.mode, Mode::Scenes | Mode::Inspect) {
                    self.st.live.playing = false;
                    self.st.live.frame = if e.key_code == KeyCode::Home { 0 } else { self.st.live.frames.saturating_sub(1) };
                    crate::screens::sync_stage_tl(self, cx);
                    self.ui.redraw(cx);
                }
            }
            _ => {}
        }
    }
}

impl App {
    fn st_dirty_check(&mut self) -> bool {
        // Cheap: any handler that mutates sets this via toasts/mode/modal
        // changes; we just always re-sync after actions.
        true
    }

    pub fn light(&self) -> f32 {
        match self.st.theme {
            Theme::Dark => 0.0,
            Theme::Light => 1.0,
            Theme::Auto => 0.0, // desktop default dark; OS query TODO
        }
    }

    fn t(&self, k: &str) -> String {
        tr(self.st.lang, k).to_string()
    }
    fn tf(&self, k: &str, a: &[&str]) -> String {
        trf(self.st.lang, k, a)
    }

    // ------------------------------------------------------------- routing --

    fn route_chrome(&mut self, cx: &mut Cx, actions: &Actions) {
        for (i, id) in [ids!(tab0), ids!(tab1), ids!(tab2), ids!(tab3), ids!(tab4), ids!(tab5), ids!(tab6), ids!(tab7)]
            .into_iter()
            .enumerate()
        {
            if self.ui.button(cx, id).clicked(actions) {
                self.st.mode = MODES[i];
                self.sync_all(cx);
                return;
            }
        }
        if self.ui.button(cx, ids!(lang_btn)).clicked(actions) {
            self.st.lang = 1 - self.st.lang;
            self.sync_all(cx);
        }
        if self.ui.button(cx, ids!(theme_btn)).clicked(actions) {
            self.st.theme = match self.st.theme {
                Theme::Auto => Theme::Light,
                Theme::Light => Theme::Dark,
                Theme::Dark => Theme::Auto,
            };
            self.sync_all(cx);
        }
        if self.ui.button(cx, ids!(help_btn)).clicked(actions) {
            self.st.modal = AppModal::Tour;
            self.sync_all(cx);
        }
        if crate::screens::view_clicked(actions, self.ui.widget(cx, ids!(c_dep)).widget_uid()) {
            self.st.mode = Mode::Deploy;
            self.sync_all(cx);
        }
    }

    fn route_rails(&mut self, cx: &mut Cx, actions: &Actions) {
        crate::screens::route_left_rail(self, cx, actions);
        crate::screens::route_right_rail(self, cx, actions);
        crate::screens::route_stage_tl(self, cx, actions);
        crate::screens::route_composer(self, cx, actions);
        crate::screens::route_home(self, cx, actions);
        crate::screens::route_toasts(self, cx, actions);
    }

    fn route_modal(&mut self, cx: &mut Cx, actions: &Actions) {
        crate::screens::route_modal(self, cx, actions);
    }

    // -------------------------------------------------------------- syncing --

    pub fn sync_all(&mut self, cx: &mut Cx) {
        let l = self.light();
        self.sync_chrome(cx, l);
        crate::screens::sync_mode_frame(self, cx);
        crate::screens::sync_left_rail(self, cx);
        crate::screens::sync_right_rail(self, cx);
        crate::screens::sync_stage_tl(self, cx);
        crate::screens::sync_home(self, cx);
        crate::screens::sync_toasts(self, cx);
        crate::screens::sync_modal(self, cx);
        self.sync_terrain_view(cx);
        self.sync_mjcf_view(cx);
        self.ui.redraw(cx);
    }

    fn sync_fast(&mut self, cx: &mut Cx) {
        let mut dirty = false;
        // real process output
        if self.drain_real_proc(cx) {
            dirty = true;
        }
        // MuJoCo sim-to-sim result + the motion it simulated
        if self.drain_mujoco(cx) {
            dirty = true;
        }
        // Train shows the policy the run is producing: whenever the
        // checkpoint or the scene changes, roll it out in the background and
        // replay the result — training and validation render through one
        // path, one robot, one look.
        if self.tick_train_preview(cx) {
            dirty = true;
        }
        // real rollout replay drives the 3D robot. Validate is in this list
        // because sim-to-sim delivers a rollout there: the scores say whether
        // the policy transferred, and watching it is how you see what "it
        // didn't" actually looked like. Train is here for the policy preview.
        let plays = matches!(self.st.mode, Mode::Scenes | Mode::Inspect | Mode::Validate | Mode::Train);
        if self.replay.is_some() && self.st.live.playing && plays {
            self.drive_replay(cx);
            dirty = true;
        }
        // playback: two cheap value writes, no VM applies
        if self.st.live.playing && plays {
            crate::screens::sync_transport_fast(self, cx);
            dirty = true;
        }
        // sweep reveal
        if self.st.sweep_state == SweepState::Running {
            crate::screens::sync_sweep_fast(self, cx);
            dirty = true;
        }
        // gates/pings in flight → the deploy rail is genuinely changing
        let gates_busy = self.st.mode == Mode::Deploy
            && (GATES.iter().any(|g| self.st.dg.gate(*g).running())
                || self.st.targets.iter().any(|t| t.ping == Ping::Probing));
        if gates_busy {
            crate::screens::sync_right_rail(self, cx);
            crate::screens::sync_stage_tl(self, cx);
            dirty = true;
        }
        // toasts: only when the set actually changed (birth or expiry)
        if self.st.toasts.len() != self.toast_seen {
            self.toast_seen = self.st.toasts.len();
            crate::screens::sync_toasts(self, cx);
            dirty = true;
        }
        if dirty {
            self.ui.redraw(cx);
        }
    }

    fn sync_slow(&mut self, cx: &mut Cx) {
        let l = self.light();
        self.sync_chrome(cx, l);
        crate::screens::sync_stage_tl(self, cx);
        if self.st.mode == Mode::Home {
            crate::screens::sync_home(self, cx);
        }
        if matches!(self.st.mode, Mode::Train | Mode::Runs | Mode::Deploy) {
            crate::screens::sync_left_rail(self, cx);
            crate::screens::sync_right_rail(self, cx);
        }
        self.ui.redraw(cx);
    }

    fn sync_chrome(&mut self, cx: &mut Cx, l: f32) {
        let modes_bg = self.ui.view(cx, ids!(modes));
        if !modes_bg.is_empty() {
            let mut m = modes_bg.clone();
            script_apply_eval!(cx, m, { draw_bg +: { light: #(l as f64) } });
        }
        let mut root = self.ui.view(cx, ids!(root_bg));
        if !root.is_empty() {
            script_apply_eval!(cx, root, { draw_bg +: { light: #(l as f64) } });
        }
        for (i, id) in [ids!(tab0), ids!(tab1), ids!(tab2), ids!(tab3), ids!(tab4), ids!(tab5), ids!(tab6), ids!(tab7)]
            .into_iter()
            .enumerate()
        {
            let on = if MODES[i] == self.st.mode { 1.0 } else { 0.0 };
            let label = self.t(MODES[i].label());
            let mut b = self.ui.button(cx, id);
            if b.is_empty() {
                continue;
            }
            b.set_text(cx, &label);
            script_apply_eval!(cx, b, {
                draw_bg +: { on: #(on) light: #(l as f64) }
                draw_text +: { on: #(on) light: #(l as f64) }
            });
        }
        // chips
        let running = self.st.runs.iter().find(|r| r.state == RunState::Running);
        let (state_txt, tone, pulse) = match running {
            Some(r) => (
                self.tf("training · stage {0}/{1}", &[&(r.stage + 1).to_string(), &r.stages.len().to_string()]),
                0.0,
                true,
            ),
            None => {
                let paused = self.st.runs.iter().any(|r| r.state == RunState::Paused);
                if paused {
                    (self.t("paused"), 0.5, true)
                } else {
                    (self.t("stopped"), 0.5, false)
                }
            }
        };
        let steps = self
            .st
            .runs
            .iter()
            .find(|r| r.state == RunState::Running)
            .map(|r| r.steps)
            .unwrap_or(2.10);
        let steps_txt = self.tf("{0}M steps", &[&format!("{steps:.2}")]);
        crate::screens::set_chip(cx, &self.ui, ids!(c_state), &state_txt, Some(tone), pulse, l);
        crate::screens::set_chip(cx, &self.ui, ids!(c_steps), &steps_txt, None, false, l);
        let live_n = self.st.deploys.iter().filter(|d| d.state == DepState::Live).count();
        let cdep = self.ui.view(cx, ids!(c_dep));
        if !cdep.is_empty() {
            cdep.set_visible(cx, live_n > 0);
            if live_n > 0 {
                let txt = self.tf("{0} deployment live", &[&live_n.to_string()]);
                crate::screens::set_chip(cx, &self.ui, ids!(c_dep), &txt, Some(0.0), true, l);
            }
        }
        let mut lb = self.ui.button(cx, ids!(lang_btn));
        if !lb.is_empty() {
            lb.set_text(cx, if self.st.lang == 0 { "中文" } else { "EN" });
            script_apply_eval!(cx, lb, { draw_bg +: { light: #(l as f64) } draw_text +: { light: #(l as f64) } });
        }
        let mut tb = self.ui.button(cx, ids!(theme_btn));
        if !tb.is_empty() {
            tb.set_text(cx, match self.st.theme {
                Theme::Auto => "◐",
                Theme::Light => "☀",
                Theme::Dark => "☾",
            });
            script_apply_eval!(cx, tb, { draw_bg +: { light: #(l as f64) } draw_text +: { light: #(l as f64) } });
        }
        let mut hb = self.ui.button(cx, ids!(help_btn));
        if !hb.is_empty() {
            script_apply_eval!(cx, hb, { draw_bg +: { light: #(l as f64) } draw_text +: { light: #(l as f64) } });
        }
        // rails + window clear
        for id in [ids!(rail_l), ids!(rail_r)] {
            let mut p = self.ui.view(cx, id);
            if !p.is_empty() {
                script_apply_eval!(cx, p, { draw_bg +: { light: #(l as f64) } });
            }
        }
        let _ = pal::VOID_D;
    }
}

impl App {
    /// Row-click actions from the left rail.
    pub fn dispatch_l(&mut self, cx: &mut Cx, act: &crate::screens::LAct) {
        use crate::screens::LAct;
        match act {
            LAct::SelScene(id) => {
                self.st.sel_scene(id);
                self.replay = None;
                self.replay_map = None;
                self.st.replay_driven = false;
                self.st.resim();
            }
            LAct::SelRobot(id) => self.st.sel.robot = Some(id.clone()),
            LAct::SelRun(id) => self.st.sel.run = Some(id.clone()),
            LAct::PickCk(rn, l) => self.st.pick_ck(rn, l),
            LAct::DepPickCk(rn, l) => self.st.dep_pick_ck(rn, l),
            LAct::SelTarget(id) => self.st.sel_target(id),
            LAct::Replay(id) => {
                self.st.replay(id);
                self.replay = None;
                self.replay_map = None;
                self.st.replay_driven = false;
                if let Some(rec) = self.st.recordings.iter().find(|r| &r.id == id) {
                    if let Some(p) = rec.path.clone() {
                        match crate::nexus::load_rollout(std::path::Path::new(&p), &rec.name) {
                            Some(rp) => {
                                self.st.live.frames = rp.joints.len() as u32;
                                self.st.live.frame = 0;
                                self.replay = Some(rp);
                                self.st.replay_driven = true;
                                self.st.toast("real rollout loaded — driving the 3D robot".into());
                            }
                            None => self.st.toast("rollout file failed to parse".into()),
                        }
                    }
                }
            }
            LAct::SelRecipe(i) => self.st.sel_recipe(*i),
            LAct::None => return,
        }
        self.sync_all(cx);
    }

    pub fn dispatch(&mut self, cx: &mut Cx, act: &str, _idx: usize) {
        self.dispatch_named(cx, act, "");
    }

    /// Parse "action:arg" composite ids from spec-built buttons/rows.
    pub fn dispatch_arg(&mut self, cx: &mut Cx, composite: &str) {
        match composite.split_once(':') {
            Some((a, b)) => {
                let (a, b) = (a.to_string(), b.to_string());
                match a.as_str() {
                    "fam" => {
                        self.st.edit_terrain(&b);
                        self.sync_all(cx);
                    }
                    "ck-promote" => {
                        self.st.modal = AppModal::PromoteCk { ck_id: b };
                        self.sync_all(cx);
                    }
                    "ck-demote" => {
                        self.st.ck_demote(&b);
                        self.sync_all(cx);
                    }
                    "ck-export" => {
                        self.st.ck_export(&b);
                        self.sync_all(cx);
                    }
                    "ck-del" => {
                        self.st.ck_del(&b);
                        self.sync_all(cx);
                    }
                    "ck-inspect" => {
                        self.st.ck_inspect(&b);
                        self.sync_all(cx);
                    }
                    "ck-deploy" => {
                        self.st.ck_deploy(&b);
                        self.sync_all(cx);
                    }
                    "sel-dep" => {
                        self.st.sel.dep = Some(b);
                        self.sync_all(cx);
                    }
                    _ => self.dispatch_named(cx, &a, &b),
                }
            }
            None => self.dispatch_named(cx, composite, ""),
        }
    }

    pub fn redraw_ui(&mut self, cx: &mut Cx) {
        self.ui.redraw(cx);
    }

    /// Start real training — or, with `DOROBOT_ATTACH_LOG` set, attach to a
    /// run already going — and adopt it as the selected Run row so every
    /// screen shows its numbers.
    fn start_real_train(&mut self, cx: &mut Cx) {
        let attached = std::env::var("DOROBOT_ATTACH_LOG").is_ok();
        match crate::nexus::run_train(2_000_000) {
            Ok(p) => {
                self.real_proc = Some(p);
                self.train_seen = 0;
                let iters: u32 = std::env::var("DOROBOT_ATTACH_ITERS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(2_000);
                let stage = {
                    let fam = std::env::var("BIPED_TERRAIN_FAMILY").unwrap_or_default();
                    let slope = std::env::var("BIPED_TERRAIN_SLOPE_DEG").unwrap_or_default();
                    match (fam.is_empty(), slope.is_empty()) {
                        (true, _) => "flat".to_string(),
                        (false, true) => fam,
                        (false, false) => format!("{fam} · {slope}°"),
                    }
                };
                let name = if attached { "attached run" } else { "studio run" };
                self.st.adopt_real_run(name, &stage, iters);
                self.st.toast(if attached {
                    "attached to the running trainer — its curves are live".into()
                } else {
                    "real training started — 2M env-steps on the linked engine".into()
                });
            }
            Err(e) => self.st.toast(format!("could not start real training: {e}")),
        }
        self.sync_all(cx);
    }

    /// Start the engine behind a run the user just configured and launched.
    ///
    /// Distinct from [`Self::start_real_train`] only in who owns the run row:
    /// there, the trainer is found first and a row is adopted for it; here,
    /// `Store::launch` has already inserted the row with its snapshot — the
    /// set, the scenes, the warm start — and that provenance is the point of
    /// the pre-flight, so it is kept and the engine started underneath it.
    ///
    /// Before this, Launch inserted the row and started nothing: the timeline
    /// that followed was a demo curve, and the only way to train from the
    /// console was a second control labelled "real train".
    fn begin_launched_run(&mut self, cx: &mut Cx) {
        match crate::nexus::run_train(2_000_000) {
            Ok(p) => {
                self.real_proc = Some(p);
                self.train_seen = 0;
                self.st.live.real = true;
                self.st.live.hist.clear();
                self.st.live.now_hist.clear();
                self.st.live.hist_full.clear();
            }
            // The row stays. It records what was asked for, which is still
            // true; the toast says the engine did not start.
            Err(e) => {
                self.st.live.real = false;
                self.st.toast(format!("launched, but training did not start: {e}"));
            }
        }
        self.sync_all(cx);
    }

    /// Drain progress from the running engine job; harvest the sweep surface.
    fn drain_real_proc(&mut self, cx: &mut Cx) -> bool {
        let Some(proc_) = &mut self.real_proc else { return false };
        let mut got = false;
        while let Ok(line) = proc_.rx.try_recv() {
            got = true;
            self.real_lines.push(line);
            if self.real_lines.len() > 200 {
                self.real_lines.remove(0);
            }
        }
        // A Train job's numbers come off the metric stream itself, not the
        // printed lines: reward + fall-rate curves into `st.live`, cumulative
        // steps into the adopted Run row.
        if proc_.kind == crate::nexus::ProcKind::Train {
            if let Some(h) = &proc_.train {
                if let Ok(g) = h.shared.lock() {
                    let from = self.train_seen.min(g.samples.len());
                    // The stream self-trims its history; renumber against what
                    // remains rather than sticking to absolute indices.
                    if self.train_seen > g.samples.len() {
                        self.train_seen = g.samples.len();
                    }
                    let fresh = &g.samples[from..];
                    if !fresh.is_empty() {
                        got = true;
                        for s in fresh {
                            self.st.live.hist.push(s.reward as f64);
                            self.st.live.hist_full.push(s.reward as f64);
                        }
                        // The full-run curve stays bounded by decimation, not
                        // truncation — the shape of the whole run survives.
                        if self.st.live.hist_full.len() > 1200 {
                            let dec: Vec<f64> = self
                                .st
                                .live
                                .hist_full
                                .chunks(2)
                                .map(|c| c.iter().sum::<f64>() / c.len() as f64)
                                .collect();
                            self.st.live.hist_full = dec;
                        }
                        let keep = self.st.live.hist.len().saturating_sub(120);
                        if keep > 0 {
                            self.st.live.hist.drain(0..keep);
                        }
                        let last = &fresh[fresh.len() - 1];
                        self.st.live.reward = last.reward as f64;
                        self.st.live.falls = last.fall_rate as f64 * 100.0;
                        self.st.live.sps = last.steps_per_sec;
                        let envs = g.envs.max(1) as u64;
                        let iter = last.step / (envs * 24);
                        let steps_m = last.step as f64 / 1.0e6;
                        let best = self.st.live.hist.iter().cloned().fold(f64::MIN, f64::max);
                        if let Some(r) = self
                            .st
                            .sel
                            .run
                            .clone()
                            .and_then(|id| self.st.run_mut(&id))
                        {
                            r.iter = (iter as u32).min(r.iters_per);
                            r.steps = steps_m;
                            r.best = Some(best);
                        }
                    }
                    self.train_seen = g.samples.len();
                }
            }
        }
        // Read the surface itself rather than reconstructing it from printed
        // rows: the engine is in this process, so the grid is already here.
        // `sweep_at` is the engine's own measured count, which is what the
        // pass percentage takes its denominator from.
        if got && proc_.kind == crate::nexus::ProcKind::Sweep {
            if let Some((g, done)) = proc_.grid() {
                self.st.sweep_grid = Some(g);
                self.st.sweep_at = done;
            }
        }
        if proc_.finished() {
            let kind = proc_.kind;
            // Take the completed surface before dropping the job.
            if kind == crate::nexus::ProcKind::Sweep {
                if let Some((g, done)) = proc_.grid() {
                    self.st.sweep_grid = Some(g);
                    self.st.sweep_at = done;
                }
                // Aborted runs keep the partial surface and say so; only a run
                // that reached the end is complete.
                if self.st.sweep_state == SweepState::Running {
                    self.st.sweep_state = SweepState::Complete;
                    let p = self.st.sweep_pass();
                    let m = crate::i18n::trf(
                        self.st.lang,
                        "Sweep complete — {0}% of cells pass",
                        &[&p.to_string()],
                    );
                    self.st.toast(m);
                }
            }
            if kind == crate::nexus::ProcKind::Train {
                self.st.live.real = false;
                if let Some(r) = self.st.sel.run.clone().and_then(|id| self.st.run_mut(&id)) {
                    if r.state == RunState::Running {
                        r.state = RunState::Completed;
                    }
                }
            }
            self.drive_ack += 1;
            self.real_proc = None;
            let msg = match kind {
                crate::nexus::ProcKind::Train => "real training finished".to_string(),
                crate::nexus::ProcKind::Sweep => "real sweep finished".to_string(),
            };
            if kind == crate::nexus::ProcKind::Train {
                self.st.toast(msg);
            }
            self.st.merge_disk(); // new checkpoints/recordings may exist now
            crate::drive::dump_state(self);
            self.sync_all(cx);
            return true;
        }
        if got {
            crate::drive::dump_state(self);
        }
        if got && matches!(self.st.mode, Mode::Train | Mode::Validate) {
            crate::screens::sync_right_rail(self, cx);
            crate::screens::sync_stage_tl(self, cx);
        }
        got
    }

    /// Watch a MuJoCo sim-to-sim run: publish its verdict, and hand the motion
    /// it simulated to the 3D robot once it arrives.
    ///
    /// The two land at the same moment — the engine fills both from one harness
    /// run — but they are taken separately here because the rollout is the
    /// weaker product: a run can score and still fail to dump a trajectory, and
    /// that is a result worth showing, not a failure worth discarding.
    fn drain_mujoco(&mut self, cx: &mut Cx) -> bool {
        let Some(job) = &self.mj else { return false };
        let running = job.running();
        if !running && !job.replayed {
            let motion = job.rollout.snapshot();
            if let Some(job) = &mut self.mj {
                job.replayed = true;
            }
            match motion {
                Some(r) if !r.is_empty() => {
                    let rp = crate::nexus::replay_from(&r, "mujoco-sim2sim");
                    self.st.live.frames = rp.joints.len() as u32;
                    self.st.live.frame = 0;
                    self.st.live.playing = true;
                    self.replay = Some(rp);
                    self.replay_map = None;
                    self.st.replay_driven = true;
                    let m = format!("MuJoCo rollout loaded — {} frames on the 3D robot", r.len());
                    self.st.toast(m);
                }
                // Scores without motion. Said plainly rather than left as a
                // robot standing still, which reads as a policy that froze.
                _ => self.st.toast("MuJoCo finished — no rollout to replay".into()),
            }
            self.drive_ack += 1;
            crate::drive::dump_state(self);
            self.sync_all(cx);
            return true;
        }
        // While it runs, the stage shows the elapsed state each tick.
        running && self.st.mode == Mode::Validate
    }

    /// Put the SELECTED scene's real ground under the 3D robot: generate (or
    /// reuse) the terrain mesh through the same `TerrainStrip` the trainer
    /// collides with, and hand it to RobotView. Flat scenes clear it. Called
    /// when the selection or a terrain knob changes; cheap when nothing did.
    pub fn sync_terrain_view(&mut self, cx: &mut Cx) {
        // An MJCF scene brings its own ground through the bridge; the scene
        // library's terrain would stack a second world on top of it.
        if std::env::var("NEXUS_MJCF").is_ok() {
            return;
        }
        let sc = crate::nexus::from_ui(&self.st.cur_scene());
        let key = format!("{}|{:.2}|{:.1}|{:x}", sc.terrain, sc.terrain_amp, sc.terrain_slope_deg, sc.seed);
        if self.terrain_shown == key {
            return;
        }
        let stage = self.ui.widget(cx, ids!(stage));
        if stage.is_empty() {
            return;
        }
        let rv = {
            use makepad_urdf_player::robot_view::RobotViewWidgetRefExt;
            stage.robot_view(cx, ids!(urdf_wrap.rview))
        };
        if rv.is_empty() {
            return;
        }
        let mesh = if sc.terrain.is_empty() {
            None
        } else {
            nexus_engine::zealot::ensure_terrain_mesh(&sc)
        };
        {
            let Some(mut inner) = rv.borrow_mut() else { return };
            match mesh {
                Some(path) => match inner.load_terrain(cx, &path.to_string_lossy()) {
                    Ok(n) => eprintln!("scene: terrain '{}' loaded, {n} triangles", sc.terrain),
                    Err(e) => {
                        eprintln!("scene: terrain {} failed to load: {e}", path.display());
                        return;
                    }
                },
                None => inner.clear_terrain(cx),
            }
        }
        self.terrain_shown = key;
    }

    /// Advance the loaded rollout one playback frame into RobotView.
    /// NEXUS_MJCF=<model.xml>: put the MuJoCo model itself on the stage.
    ///
    /// The bridge loads the MJCF with the real libmujoco, poses it (first
    /// keyframe when one exists), and hands over its mjvScene — every geom
    /// MuJoCo would draw, meshes and heightfields resolved to triangles,
    /// primitives to shared unit meshes scaled per instance. The stage's own
    /// renderer draws them: same shadows, same sky, same materials as
    /// everything else. One shot; failures toast and leave the stage as it
    /// was.
    pub fn sync_mjcf_view(&mut self, cx: &mut Cx) {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        if self.mjcf_pushed {
            return;
        }
        let Ok(path) = std::env::var("NEXUS_MJCF") else { return };
        let stage = self.ui.widget(cx, ids!(stage));
        if stage.is_empty() {
            return;
        }
        let rv = {
            use makepad_urdf_player::robot_view::RobotViewWidgetRefExt;
            stage.robot_view(cx, ids!(urdf_wrap.rview))
        };
        if rv.is_empty() {
            return;
        }
        self.mjcf_pushed = true;
        if let Some(why) = nexus_mujoco::why_unavailable() {
            self.st.toast(format!("MJCF view unavailable: {why}"));
            return;
        }
        let mut model = match nexus_mujoco::Model::load(&path) {
            Ok(m) => m,
            Err(e) => {
                self.st.toast(format!("MJCF load failed: {e}"));
                return;
            }
        };
        if model.nkey() > 0 {
            model.reset_keyframe(0);
        }
        let geoms = model.scene();
        let Some(mut inner) = rv.borrow_mut() else { return };
        fn h<T: Hash>(v: &T) -> u64 {
            let mut s = DefaultHasher::new();
            v.hash(&mut s);
            s.finish()
        }
        fn q(v: f32) -> i64 {
            (v * 1000.0).round() as i64
        }
        use makepad_urdf_player::robot_view::ExternalItem;
        use nexus_mujoco::{prim, GeomKind};
        let mut uploaded: std::collections::HashSet<u64> = Default::default();
        let mut items: Vec<ExternalItem> = Vec::new();
        for g in &geoms {
            // (key, per-instance scale, mesh built on first sight of the key)
            let (key, scale, built) = match g.kind {
                GeomKind::Mesh { id } => {
                    let key = h(&("mesh", id));
                    let m = (!uploaded.contains(&key))
                        .then(|| model.mesh(id as usize))
                        .flatten();
                    (key, [1.0f32; 3], m)
                }
                GeomKind::Hfield { id } => {
                    let key = h(&("hfield", id));
                    let m = (!uploaded.contains(&key))
                        .then(|| model.hfield(id as usize))
                        .flatten();
                    (key, [1.0; 3], m)
                }
                GeomKind::Sphere { r } => (h(&"usphere"), [r, r, r], Some(prim::unit_sphere(16, 24))),
                GeomKind::Ellipsoid { half } => (h(&"usphere"), half, Some(prim::unit_sphere(16, 24))),
                GeomKind::Cylinder { r, half_len } => {
                    (h(&"ucyl"), [r, r, half_len], Some(prim::unit_cylinder(28)))
                }
                GeomKind::Capsule { r, half_len } => (
                    h(&("cap", q(r), q(half_len))),
                    [1.0; 3],
                    Some(prim::capsule(r, half_len, 8, 24)),
                ),
                GeomKind::Box { half } => (h(&"ubox"), half, Some(prim::unit_box())),
                GeomKind::Plane { half } => {
                    let hx = if half[0] > 0.0 { half[0] } else { 25.0 };
                    let hy = if half[1] > 0.0 { half[1] } else { 25.0 };
                    (h(&("plane", q(hx), q(hy))), [1.0; 3], Some(prim::plane(hx, hy)))
                }
                GeomKind::Other(_) => continue,
            };
            if !uploaded.contains(&key) {
                match built {
                    Some(m) => {
                        inner.upsert_external_mesh(cx, key, &m.positions, &m.normals, &m.indices);
                        uploaded.insert(key);
                    }
                    // Extraction refused (the bridge logs why); a missing part
                    // beats a garbage one.
                    None => continue,
                }
            }
            // MJCF grounds ship white and take their look from textures this
            // path does not sample; give them the stage palette instead.
            let (color, rough, met) = match g.kind {
                GeomKind::Hfield { .. } => ([0.34, 0.33, 0.30, 1.0], 0.95, 0.0),
                GeomKind::Plane { .. } => ([0.20, 0.30, 0.40, 1.0], 0.90, 0.0),
                GeomKind::Mesh { .. } => (g.rgba, 0.42, 0.10),
                _ => (g.rgba, 0.55, 0.0),
            };
            items.push(ExternalItem {
                mesh_key: key,
                // column-major from MuJoCo's row-major 3x3 + position
                transform: [
                    g.mat[0], g.mat[3], g.mat[6], 0.0,
                    g.mat[1], g.mat[4], g.mat[7], 0.0,
                    g.mat[2], g.mat[5], g.mat[8], 0.0,
                    g.pos[0], g.pos[1], g.pos[2], 1.0,
                ],
                scale,
                color,
                roughness: rough,
                metallic: met,
            });
        }
        let n = items.len();
        inner.set_external_items(cx, items);
        inner.frame_camera_on_external(cx);
        drop(inner);
        self.mjcf = Some(model);
        self.st.toast(format!("MJCF on stage: {n} geoms via libmujoco"));
    }

    /// Keep Train mode's stage honest: the 3D robot replays what the policy
    /// can do *now*. Watches the checkpoint the run is producing (path and
    /// mtime — trainers overwrite in place) and the scene under the stage;
    /// when either changes, `zealot::drive` rolls the checkpoint out on that
    /// scene's physics in the background, and the arrival replaces the
    /// playing motion. One drive in flight at a time; a failed or collapsed
    /// rollout keeps the previous motion and is not retried until the
    /// checkpoint moves again. Returns true when fresh motion just landed.
    fn tick_train_preview(&mut self, cx: &mut Cx) -> bool {
        if self.st.mode != Mode::Train {
            return false;
        }
        // Harvest a finished drive before considering a new one.
        if let Some(job) = &self.preview {
            if !job.done.load(std::sync::atomic::Ordering::Relaxed) {
                return false;
            }
            let job = self.preview.take().unwrap();
            match job.slot.snapshot() {
                Some(r) if !r.is_empty() && !r.collapsed() => {
                    let rp = crate::nexus::replay_from(&r, "policy-preview");
                    self.st.live.frames = rp.joints.len() as u32;
                    self.st.live.frame = 0;
                    self.replay = Some(rp);
                    self.replay_map = None;
                    self.st.replay_driven = true;
                    self.st.live.playing = true;
                    self.drive_replay(cx);
                    return true;
                }
                _ => {
                    eprintln!("[preview] rollout failed or collapsed — keeping the last motion");
                    return false;
                }
            }
        }
        let ckpt = nexus_engine::cli::zealot_ckpt_path();
        let Ok(meta) = std::fs::metadata(&ckpt) else { return false };
        let stamp = meta.modified().map(|t| format!("{t:?}")).unwrap_or_default();
        let sc = self.st.cur_scene();
        let key = format!(
            "{ckpt}|{stamp}|{}|{:.2}|{:.1}|{}|{:.2}",
            sc.terrain, sc.amp, sc.slope, sc.seed, sc.stance
        );
        if key == self.preview_key {
            return false;
        }
        match crate::nexus::spawn_preview(&ckpt, &sc) {
            Some(job) => {
                eprintln!("[preview] driving {ckpt} on '{}'", sc.name);
                self.preview_key = key;
                self.preview = Some(job);
            }
            // No drive binary on this machine: nothing will ever arrive, so
            // remember the key rather than probing every tick.
            None => self.preview_key = key,
        }
        false
    }

    fn drive_replay(&mut self, cx: &mut Cx) {
        let Some(rp) = &self.replay else { return };
        let frame = (self.st.live.frame as usize).min(rp.joints.len().saturating_sub(1));
        let stage = self.ui.widget(cx, ids!(stage));
        if stage.is_empty() {
            return;
        }
        let rv = {
            use makepad_urdf_player::robot_view::RobotViewWidgetRefExt;
            stage.robot_view(cx, ids!(urdf_wrap.rview))
        };
        if rv.is_empty() {
            return;
        }
        // Lazily build robot-joint → rollout-column map by joint name.
        if self.replay_map.is_none() {
            if let Some(inner) = rv.borrow_mut() {
                let names = inner.movable_joint_names();
                if !names.is_empty() {
                    self.replay_map = Some(
                        names
                            .iter()
                            .map(|n| rp.joint_names.iter().position(|j| j == n))
                            .collect(),
                    );
                }
            }
        }
        let Some(map) = &self.replay_map else { return };
        let row = &rp.joints[frame];
        let angles: Vec<f32> = map
            .iter()
            .map(|col| col.and_then(|c| row.get(c).copied()).unwrap_or(0.0))
            .collect();
        rv.set_joint_angles(cx, &angles);
        // The base travels too — apply it, and keep the camera on the robot,
        // or a walking rollout replays as marching-in-place at the origin
        // while the real motion happens off-screen on the terrain.
        let base = rp.base.get(frame).copied();
        if let Some(b) = base {
            if let Some(mut inner) = rv.borrow_mut() {
                inner.set_base_pose(cx, [b[0], b[1], b[2]], [b[3], b[4], b[5], b[6]]);
                inner.set_camera_target(cx, [b[0], b[1], b[2]]);
            }
        }
        if frame % 50 == 0 {
            eprintln!(
                "[replay] frame {frame} angle0 {:.3} base {:?}",
                angles.first().copied().unwrap_or(0.0),
                base.map(|b| (b[0], b[1], b[2]))
            );
        }
    }

    pub fn slider_changed(&mut self, cx: &mut Cx, field: &str, v: f64) {
        self.st.edit_field(field, v);
        crate::screens::sync_right_rail(self, cx);
        crate::screens::sync_stage_tl(self, cx);
        self.ui.redraw(cx);
    }

    pub fn slider_committed(&mut self, cx: &mut Cx, field: &str, v: f64) {
        self.st.edit_field(field, v);
        self.st.resim();
        self.sync_all(cx);
    }

    /// The mockup's ACT dispatcher: every named action in one place.
    pub fn dispatch_named(&mut self, cx: &mut Cx, act: &str, arg: &str) {
        let st = &mut self.st;
        match act {
            // scenes
            "new-scene" => st.new_scene(),
            "dup-scene" => st.dup_scene(),
            "del-scene" => st.del_scene(),
            "del-scene-yes" => st.del_scene_yes(),
            "addto-set" => st.modal = AppModal::AddToSet,
            "save" => st.save(),
            "saveas" => st.saveas(),
            "revert" => {
                st.revert();
                st.resim();
            }
            "del-rec" => st.del_rec(arg),
            "record-probe" => st.record_probe(),
            // composer
            "comp-add" => st.comp_add(),
            "set-rename" => st.modal = AppModal::RenameSet,
            "set-dup" => st.set_dup(),
            "train-set" => st.modal = AppModal::Preflight,
            // train
            "pause" => st.pause(),
            "resume" => st.resume(),
            "stop" => st.stop(),
            // sweep
            // The engine's sweep, not a canned surface. There used to be two
            // controls here — this one replaying a fixture and a second one
            // labelled "real" — which put the honest measurement behind the
            // demo and made "run sweep" mean whichever the reader guessed.
            "sweep-run" => {
                match crate::nexus::run_sweep() {
                    Ok(p) => {
                        self.real_proc = Some(p);
                        self.st.sweep_grid = None;
                        self.st.sweep_at = 0;
                        self.st.sel.cell = None;
                        self.st.sweep_state = SweepState::Running;
                        self.st.toast("Sweep started — 40 cells on the linked engine".into());
                    }
                    Err(e) => self.st.toast(format!("could not start the sweep: {e}")),
                }
                self.sync_all(cx);
                return;
            }
            "real-train" => {
                self.start_real_train(cx);
                return;
            }
            // Sim-to-sim against a genuinely independent engine. 0.3 m/s is
            // what zealot's own cross-check drives, so the two are comparable;
            // 45 s is long enough for episodes to terminate, and the harness
            // only records an attempt when one ends.
            "mujoco-run" => {
                let ckpt = nexus_engine::cli::zealot_ckpt_path();
                match crate::nexus::run_mujoco(&ckpt, 0.3, 45) {
                    Ok(job) => {
                        // The rollout arrives later and drives the 3D robot;
                        // clear whatever is playing so the old motion is not
                        // showing under the new run's numbers.
                        self.replay = None;
                        self.replay_map = None;
                        self.st.replay_driven = false;
                        self.mj = Some(job);
                        self.st.toast("MuJoCo sim2sim started — a second engine, ~45 s".into());
                    }
                    Err(e) => self.st.toast(e),
                }
                self.sync_all(cx);
                return;
            }
            // The weaker arm, and labelled as such. Until this existed the
            // right rail showed four invented numbers under the same heading.
            "cross-run" => {
                match crate::nexus::run_cross() {
                    Ok(job) => {
                        let m = format!("cross-sim started — {}", job.arm);
                        self.cross = Some(job);
                        self.st.toast(m);
                    }
                    Err(e) => self.st.toast(e),
                }
                self.sync_all(cx);
                return;
            }
            // Abandon rather than cancel, and the toast says "abandoned" for
            // that reason: the engine's sweep thread has no cancellation, so
            // it runs to completion in the background. Dropping the job here
            // stops this console reading it and keeps the partial surface,
            // which is evidence.
            "sweep-stop" => {
                st.sweep_stop();
                self.real_proc = None;
            }
            "sweep-del" => {
                st.sweep_del();
                self.real_proc = None;
            }
            "cell-inspect" => st.cell_inspect(),
            "cell-scene" => st.cell_scene(),
            "clear-cell" => st.cell_phys = None,
            // probe
            "push" => {
                let m = st.tr_pub("Impulse applied — future discarded, re-simulated from here");
                st.toast(m);
            }
            "restart-probe" => {
                st.live.frame = 0;
                st.live.playing = true;
                let m = st.tr_pub("Restarted — picked up the newest checkpoint");
                st.toast(m);
            }
            // runs
            "rerun" => st.modal = AppModal::Rerun,
            "open-run" => st.mode = Mode::Train,
            "run-archive" => st.run_archive(),
            "run-delete" => st.modal = AppModal::DeleteRun,
            // robots
            "import-robot" => st.modal = AppModal::Wizard { step: 1 },
            "robot-rename" => {
                let m = st.tr_pub("Rename is metadata — the URDF is untouched");
                st.toast(m);
            }
            "robot-remap" => {
                let m = st.tr_pub("Mapping changed — rehearsal re-runs before the change commits");
                st.toast(m);
            }
            "robot-remove" => st.modal = AppModal::RemoveRobotBlocked,
            // deploy
            "target-new" => st.modal = AppModal::TargetForm { edit: None },
            "target-edit" => {
                let id = st.dg.target.clone();
                st.modal = AppModal::TargetForm { edit: id };
            }
            "target-del" => st.target_del(),
            "target-ping" => st.target_ping(),
            "gate-export" => st.gate_start(GateId::Export),
            "gate-compat" => st.gate_start(GateId::Compat),
            "gate-sim2sim" => st.gate_start(GateId::Sim2sim),
            "gate-dryrun" => st.gate_start(GateId::Dryrun),
            "cancel-export" => st.gate_cancel(GateId::Export),
            "cancel-compat" => st.gate_cancel(GateId::Compat),
            "cancel-sim2sim" => st.gate_cancel(GateId::Sim2sim),
            "cancel-dryrun" => st.gate_cancel(GateId::Dryrun),
            "dep-reset" => st.dep_reset(),
            "dep-arm" => st.modal = AppModal::Arm,
            "stage-adv" => st.stage_adv(),
            "dep-done" => st.dep_done(),
            "dep-estop" => st.dep_estop(),
            "dep-estop-rec" => st.dep_estop_rec(),
            "dep-rollback" => st.dep_rollback(),
            "dep-retire" => st.dep_retire(),
            "dep-recert" => st.dep_recert(),
            "dep-back" => st.sel.dep = None,
            _ => {}
        }
        self.sync_all(cx);
    }
}



#[repr(C)]
struct NSPointFfi {
    x: f64,
    y: f64,
}

/// Post an NSApplicationDefined event to NSApp — the same wake makepad's own
/// timer callback uses to break a blocked event loop. Callable from any
/// thread (the winit wake pattern); without it, App Nap parks the app solid
/// while backgrounded and the soak driver starves.
/// NSActivityUserInitiatedAllowingIdleSystemSleep | NSActivityLatencyCritical.
/// The returned activity token is intentionally leaked — the opt-out lasts
/// for the app's lifetime.
unsafe fn disable_app_nap() {
    use objc::runtime::Object;
    use objc::{class, msg_send, sel, sel_impl};
    let pi: *mut Object = msg_send![class!(NSProcessInfo), processInfo];
    let reason: *mut Object = msg_send![
        class!(NSString),
        stringWithUTF8String: b"background training monitor + ui soak driver ".as_ptr()
    ];
    let opts: u64 = (0x00FFFFFF & !(1 << 20)) | 0xFF00000000;
    let act: *mut Object = msg_send![pi, beginActivityWithOptions: opts reason: reason];
    let _: *mut Object = msg_send![act, retain];
}

unsafe fn wake_nsapp() {
    use objc::runtime::Object;
    use objc::{class, msg_send, sel, sel_impl};
    let pool: *mut Object = msg_send![class!(NSAutoreleasePool), new];
    let nsevent: *mut Object = msg_send![
        class!(NSEvent),
        otherEventWithType: 15u64
        location: NSPointFfi { x: 0.0, y: 0.0 }
        modifierFlags: 0u64
        timestamp: 0f64
        windowNumber: 1u64
        context: std::ptr::null_mut::<Object>()
        subtype: 0i16
        data1: 0u64
        data2: 0u64
    ];
    let ns_app: *mut Object = msg_send![class!(NSApplication), sharedApplication];
    let () = msg_send![ns_app, postEvent: nsevent atStart: false];
    let () = msg_send![pool, release];
}
