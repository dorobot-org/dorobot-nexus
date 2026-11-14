//! nexus-studio — makepad port of the frozen dorobot-nexus web mockup.
//! Eight workspaces, iOS semantic theme, EN/中文, light/dark.

pub mod actions;
pub mod artifacts;
pub mod deployment;
pub mod drive;
pub mod evaluation;
pub mod external_motion;
mod file_dialogs;
pub mod i18n;
pub mod jobs;
pub mod nexus;
pub mod policy_export;
pub mod preferences;
pub mod review;
pub mod robot_assets;
pub mod robot_bundle;
pub mod robot_import;
pub mod screens;
pub mod simulation;
pub mod state;
pub mod task_config;
pub mod task_editor;
mod ux_probe;
pub mod workspace;
/// Tokens and the widget kit moved to the shared `dorobot-ux` crate, which
/// dorobot-studio draws with too. Re-exported under their old paths so the
/// screens keep referring to `crate::tokens` / `crate::kit` unchanged.
pub use dorobot_ux::{kit, tokens};

use crate::deployment::State as DeploymentState;
use crate::evaluation::State as EvaluationState;
use crate::external_motion::State as ExternalMotionState;
use crate::i18n::{tr, trf};
use crate::review::State as ReviewState;
use crate::screens::{LSpec, RSpec};
use crate::state::*;
use crate::tokens::pal;
use makepad_urdf_player::robot_view::RobotViewWidgetRefExt;
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

    pub fn run() {
        // The generated entry owns the HAL's main-thread window loop.
        // Calling app_main directly only works for offscreen windows.
        main();
    }
}

fn main() {
    if let Some(path) = std::env::args().skip_while(|s| s != "--job-worker").nth(1) {
        if let Err(error) = crate::jobs::worker(std::path::Path::new(&path)) {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return;
    }
    // Never returns if a headless flag was given.
    nexus_engine::cli::maybe_headless();
    shell::run();
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
        row := mod.widgets.screens.Row{visible:false}
        acts := View{
            width: Fill height: Fit
            visible: false
            flow: Flow.Right{wrap: true}
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
        width: Fill height: 48
        align: Align{x: 0.0 y: 0.5}
        padding: 12.0
        text: "Mode"
        draw_text +: {
            light: instance(0.0)
            on: instance(0.0)
            text_style: mod.widgets.nx.T_BODY{font_size: 12.0}
            get_color: fn() {
                let idle = mix(#xAEAEB2, #x636366, self.light)
                let ink = mix(#x64B5FF, #x0060CC, self.light)
                return mix(idle, ink, max(self.on, self.hover))
            }
        }
        draw_bg +: {
            light: instance(0.0)
            on: instance(0.0)
            pixel: fn() {
                let sdf = Sdf2d.viewport(self.pos * self.rect_size)
                let vio = mix(#x0A84FF, #x007AFF, self.light)
                sdf.box(0.0, 0.0, self.rect_size.x, self.rect_size.y, 10.0)
                let well = mix(#x19334D, #xE5F0FF, self.light)
                sdf.fill(mix(vec4(0.0,0.0,0.0,0.0), well, max(self.on, self.hover * 0.5)))
                return sdf.result
            }
        }
    }

    startup() do #(App::script_component(vm)){
        ui: Root{
            main_window := Window{
                window.title: "Nexus Studio"
                window.inner_size: vec2(1500, 980)
                pass.clear_color: #x000000

                body +: {
                    width: Fill
                    height: Fill
                    flow: Overlay

                    root_bg := mod.widgets.nx.Ground{
                        flow: Right

                        // ------------------------------------------ chrome --
                        modes := View{
                            width: 196 height: Fill flow: Down spacing: 4.0 padding: 12.0
                            show_bg: true
                            draw_bg +: {
                                light: instance(0.0)
                                pixel: fn() {
                                    let base = mix(#x1C1C1E, #xFFFFFF, self.light)
                                    let edge = mix(#x38383A, #xD1D1D6, self.light)
                                    let t = step(self.rect_size.y - 1.0, self.pos.y * self.rect_size.y)
                                    return mix(base, edge, t)
                                }
                            }
                            Label{width: Fill height: 64 text: "Nexus Studio"
                                draw_text +: {color: mod.theme.color_text text_style: mod.widgets.nx.T_TITLE{}} }
                            tab0 := ModeTab{}
                            tab1 := ModeTab{}
                            tab2 := ModeTab{}
                            tab3 := ModeTab{}
                            tab4 := ModeTab{}
                            tab5 := ModeTab{}
                            tab6 := ModeTab{}
                            tab7 := ModeTab{}
                            Filler{}
                            lang_btn := mod.widgets.nx.NxBtn{width: Fill height: 44 text: "中文"}
                            theme_btn := mod.widgets.nx.NxBtn{width: Fill height: 44 text: "Appearance: Auto"}
                            help_btn := mod.widgets.nx.NxBtn{width: Fill height: 44 text: "Help"}
                        }
                        workspace := View{width: Fill height: Fill flow: Down
                            page_header := View{width: Fill height: Fit flow: Right padding: 18.0 spacing: 12.0 align: Align{y: 0.5}
                                page_title := Label{width: Fill text: "Home"
                                    draw_text +: {color: mod.theme.color_text text_style: mod.widgets.nx.T_TITLE{font_size: 22.0}} }
                                c_state := mod.widgets.nx.ChipV{}
                                c_steps := mod.widgets.nx.ChipV{}
                                c_dep := mod.widgets.nx.ChipV{visible: false}
                                inspector_btn := mod.widgets.nx.NxBtn{height: 44 text: "Details"}
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
                                    cap := mod.widgets.nx.Cap{width: Fill max_lines: 2 text_overflow: Ellipsis}
                                    Filler{}
                                    cap_btn := mod.widgets.nx.Mini{ visible: false }
                                }
                                filter_wrap := View{
                                    width: Fill height: Fit
                                    visible: false
                                    padding: Inset{left: 9. right: 7. top: 2. bottom: 2.}
                                    filter_in := TextInput{ width: Fill empty_text: "⌕ filter…" }
                                }
                                lp := View{
                                    width: Fill height: Fit flow: Right visible: false
                                    padding: 7.0 spacing: 5.0
                                    prev := mod.widgets.nx.Mini{text:"←"}
                                    label := mod.widgets.nx.BodyLbl{width: Fill max_lines:1}
                                    next := mod.widgets.nx.Mini{text:"→"}
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
                                empty_stage := mod.widgets.nx.Panel{visible: false width: Fill height: Fill flow: Down padding: 28.0 spacing: 18.0
                                    empty_title := Label{width: Fill text: "No training yet"
                                        draw_text +: {color: mod.theme.color_text text_style: mod.widgets.nx.T_TITLE{font_size: 20.0}} }
                                    empty_description := Label{width: Fill height: Fit text: ""
                                        draw_text +: {color: mod.theme.color_text text_style: mod.widgets.nx.T_BODY{font_size: 12.0}} }
                                    empty_action := mod.widgets.nx.Primary{height: 44 text: "Choose a scene set"}
                                }
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
    #[rust]
    workspace_error: Option<String>,
    #[rust]
    pending_dialog: Option<file_dialogs::Pending>,
    #[rust]
    workspace_session: Option<crate::workspace::Session>,
    #[rust]
    artifact_plan: Option<crate::artifacts::Plan>,
    #[rust]
    checkpoint_page: usize,
    #[rust]
    pub left_page: usize,
    #[rust]
    pub left_page_mode: Option<Mode>,
    #[rust]
    pub system_light: f32,
    #[rust]
    pub window_width: f64,
    #[rust]
    pub inspector_open: bool,
    #[rust]
    export_job: Option<crate::policy_export::Job>,
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
    pub review: ReviewState,
    #[rust]
    pub replay_map: Option<Vec<Option<usize>>>,
    #[rust]
    pub drive_ack: u64,
    #[rust]
    last_fast: Option<std::time::Instant>,
    /// Samples already folded into `st.live` from the real training handle.
    #[rust]
    train_seen: usize,
    #[rust]
    replay_clock: Option<std::time::Instant>,
    #[rust]
    replay_phase: f64,
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
    #[rust]
    pub external_motion: ExternalMotionState,
    #[rust]
    pub simulation: Option<crate::simulation::Session>,
    #[rust]
    pub evaluation: EvaluationState,
    #[rust]
    pub deployment: DeploymentState,
}

impl AppMain for App {
    fn script_mod(vm: &mut ScriptVm) -> ScriptValue {
        dorobot_ux::install_ios(vm, crate::preferences::system_light() > 0.5);
        makepad_widgets::script_mod(vm);
        makepad_urdf_player::makepad_xr::script_mod(vm);
        // Nexus supplies its own joint controls. Register only the viewer;
        // the unused upstream JointPanel has an invalid TextInput property.
        makepad_urdf_player::render::draw::script_mod(vm);
        makepad_urdf_player::render::draw::composite_shader::script_mod(vm);
        makepad_urdf_player::robot_view::script_mod(vm);
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
            && matches!(
                self.st.mode,
                Mode::Scenes | Mode::Inspect | Mode::Validate | Mode::Train
            )
        {
            let now = std::time::Instant::now();
            let elapsed = self
                .replay_clock
                .replace(now)
                .map(|t| now.duration_since(t).as_secs_f64())
                .unwrap_or(0.0);
            let dt = self.replay.as_ref().map(|r| r.dt).unwrap_or(0.04);
            self.replay_phase += elapsed;
            let frames = (self.replay_phase / dt).floor() as u32;
            self.replay_phase %= dt;
            self.st.live.frame = (self.st.live.frame + frames) % self.st.live.frames.max(1);
            self.drive_replay(cx);
            self.ui.redraw(cx);
        }
        if !self.st.live.playing {
            self.replay_clock = None;
            self.replay_phase = 0.0;
        }
        self.match_event(cx, event);
        self.ui.handle_event(cx, event, &mut Scope::empty());
        if matches!(event, Event::LiveEdit | Event::ScriptReapply) {
            self.sync_all(cx);
        }
    }
}

impl MatchEvent for App {
    fn handle_startup(&mut self, cx: &mut Cx) {
        if !self.st.demo {
            match crate::workspace::Session::open_with_runs(
                std::path::Path::new(&crate::nexus::repo_dir()),
                &crate::artifacts::runs_root(),
            ) {
                Ok(session) => self.workspace_session = Some(session),
                Err(e) => {
                    self.workspace_error = Some(e.to_string());
                    self.st.toast(e.to_string());
                }
            }
        }
        self.fast_timer = cx.start_interval(0.12);
        self.slow_timer = cx.start_interval(1.1);
        // 25 Hz: the rate tools/lafan1_to_rollout.py resamples motion to, so a
        // recorded clip plays at 1.0x and one frame per tick keeps it smooth.
        self.replay_timer = cx.start_interval(0.04);
        self.st.merge_disk();
        if self.workspace_session.is_some() {
            self.refresh_artifacts();
        }
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
            let pending = std::fs::metadata(crate::drive::CMD_PATH)
                .map(|m| m.len() > 0)
                .unwrap_or(false);
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
        if self.st.demo {
            self.st.modal = AppModal::Tour;
        }
        // A portable model can be opened directly by the standalone JIT launcher.
        if let Some(path) = std::env::args().skip_while(|s| s != "--robot").nth(1) {
            self.import_robot_path(&path);
            self.st.mode = Mode::Robots;
        }
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
        self.route_file_dialogs(cx, actions);
        self.route_chrome(cx, actions);
        self.route_rails(cx, actions);
        self.route_modal(cx, actions);
        if self.st_dirty_check() {
            self.sync_all(cx);
        }
    }

    fn handle_key_down(&mut self, cx: &mut Cx, e: &KeyEvent) {
        if self.st.modal == AppModal::None && (e.modifiers.logo || e.modifiers.control) {
            let index = match e.key_code {
                KeyCode::Key1 => Some(0),
                KeyCode::Key2 => Some(1),
                KeyCode::Key3 => Some(2),
                KeyCode::Key4 => Some(3),
                KeyCode::Key5 => Some(4),
                KeyCode::Key6 => Some(5),
                KeyCode::Key7 => Some(6),
                KeyCode::Key8 => Some(7),
                _ => None,
            };
            if let Some(index) = index {
                self.st.mode = MODES[index];
                self.sync_all(cx);
                return;
            }
        }

        if cx.has_key_focus(self.ui.widget(cx, ids!(filter_in)).area())
            && !(e.modifiers.logo || e.modifiers.control)
        {
            return;
        }
        if self.simulation.is_some() && self.st.modal == AppModal::None {
            let action = match e.key_code {
                KeyCode::ArrowRight => Some("sim-step"),
                KeyCode::ArrowLeft => Some("sim-pause"),
                KeyCode::Home => Some("sim-restart"),
                _ => None,
            };
            if let Some(action) = action {
                self.dispatch_named(cx, action, "");
                return;
            }
        }
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
                        self.st.live.frame =
                            (self.st.live.frame + 3).min(self.st.live.frames.saturating_sub(1));
                    } else {
                        self.st.live.frame = self.st.live.frame.saturating_sub(3);
                    }
                    crate::screens::sync_stage_tl(self, cx);
                    self.ui.redraw(cx);
                }
            }
            KeyCode::Home | KeyCode::End => {
                if self.st.modal == AppModal::None
                    && matches!(self.st.mode, Mode::Scenes | Mode::Inspect)
                {
                    self.st.live.playing = false;
                    self.st.live.frame = if e.key_code == KeyCode::Home {
                        0
                    } else {
                        self.st.live.frames.saturating_sub(1)
                    };
                    crate::screens::sync_stage_tl(self, cx);
                    self.ui.redraw(cx);
                }
            }
            _ => {}
        }
    }
}

impl App {
    pub fn can_edit(&mut self) -> bool {
        if self.st.demo || self.workspace_session.is_some() {
            true
        } else {
            self.st.toast(self.workspace_error.clone().unwrap_or_else(|| "Workspace is open read-only. Close the other Studio and reopen this window to edit.".into()));
            false
        }
    }

    fn st_dirty_check(&mut self) -> bool {
        // Cheap: any handler that mutates sets this via toasts/mode/modal
        // changes; we just always re-sync after actions.
        true
    }

    pub fn light(&self) -> f32 {
        match self.st.theme {
            Theme::Dark => 0.0,
            Theme::Light => 1.0,
            Theme::Auto => self.system_light,
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
        for (i, id) in [
            ids!(tab0),
            ids!(tab1),
            ids!(tab2),
            ids!(tab3),
            ids!(tab4),
            ids!(tab5),
            ids!(tab6),
            ids!(tab7),
        ]
        .into_iter()
        .enumerate()
        {
            if self.ui.button(cx, id).clicked(actions) {
                self.st.mode = MODES[i];
                self.sync_all(cx);
                return;
            }
        }
        if self.ui.button(cx, ids!(empty_action)).clicked(actions) {
            self.st.mode = Mode::Scenes;
            self.sync_all(cx);
        }
        if self.ui.button(cx, ids!(inspector_btn)).clicked(actions) {
            self.inspector_open = !self.inspector_open;
            self.sync_all(cx);
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
            self.dispatch_named(cx, "help", "");
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

    fn save_workspace(&mut self) {
        if self.st.demo {
            return;
        }
        let Some(session) = &self.workspace_session else {
            return;
        };
        match session.save(&self.st) {
            Ok(()) => self.workspace_error = None,
            Err(e) => {
                let message = format!("Workspace could not be saved: {e}");
                if self.workspace_error.as_ref() != Some(&message) {
                    self.st.toast(message.clone());
                }
                self.workspace_error = Some(message);
            }
        }
    }

    pub fn sync_all(&mut self, cx: &mut Cx) {
        self.release_simulation_if_selection_changed();
        self.st.sync_run_metrics();
        self.save_workspace();
        let l = self.light();
        dorobot_ux::sync_appearance(cx, l > 0.5);
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
        if self.replay.is_some() {
            self.drive_replay(cx);
        }
        self.ui.redraw(cx);
    }

    fn sync_fast(&mut self, cx: &mut Cx) {
        if std::env::var_os("DOROBOT_UX_REPORT").is_some() {
            ux_probe::write(
                cx,
                &self.ui,
                serde_json::json!({
                    "mode": self.st.mode.label(), "theme": format!("{:?}", self.st.theme), "light": self.light(),
                    "robot_name": self.st.sel.robot.as_ref().and_then(|id| self.st.robots.iter().find(|r| &r.id == id)).map(|r| &r.name),
                    "robot_structure": self.st.sel.robot.as_ref().and_then(|id| self.st.robots.iter().find(|r| &r.id == id)).map(|r| [r.links, r.joints, r.movable, r.meshes]),
                    "robot": self.st.sel.robot, "robots": self.st.robots.len(), "modal": format!("{:?}", self.st.modal),
                    "workspace_writable": self.workspace_session.is_some(), "robot_animating": self.ui.robot_view(cx, ids!(rview)).is_animating(), "workspace_error": self.workspace_error,
                    "inspector_open": self.inspector_open, "filter": self.st.filter, "picker_pending": self.pending_dialog.is_some()
                }),
                &[
                    "tab0",
                    "tab1",
                    "tab2",
                    "tab3",
                    "tab4",
                    "tab5",
                    "tab6",
                    "tab7",
                    "theme_btn",
                    "help_btn",
                    "lang_btn",
                    "inspector_btn",
                    "rail_l",
                    "rail_r",
                    "filter_in",
                    "cap_btn",
                    "stage",
                    "modal_host",
                    "b_rename",
                    "rail_r.rr.r0.btns.b0",
                    "rail_r.rr.r5.cap.btn",
                    "rail_r.rr.r7.btns.b0",
                    "modal_host.panel.opts.opt0",
                    "modal_host.panel.foot.b_cancel",
                ],
            );
        }

        let width = self.ui.view(cx, ids!(root_bg)).area().rect(cx).size.x;
        if width > 0. && (self.window_width - width).abs() > 1. {
            self.window_width = width;
            self.sync_all(cx);
        }
        let appearance = crate::preferences::system_light();
        if self.system_light != appearance {
            self.system_light = appearance;
            if self.st.theme == Theme::Auto {
                self.sync_all(cx);
            }
        }
        self.tick_queue();
        let mut dirty = self.poll_policy_export();
        dirty |= self.tick_simulation(cx);
        dirty |= self.tick_evaluation(cx);
        dirty |= self.tick_agent(cx);
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
        if self.tick_external_motion(cx) {
            self.review = Default::default();
            crate::screens::sync_stage_tl(self, cx);
            if self.st.mode == Mode::Validate {
                crate::screens::sync_left_rail(self, cx);
                crate::screens::sync_right_rail(self, cx);
            }
            dirty = true;
        }
        // real rollout replay drives the 3D robot. Validate is in this list
        // because sim-to-sim delivers a rollout there: the scores say whether
        // the policy transferred, and watching it is how you see what "it
        // didn't" actually looked like. Train is here for the policy preview.
        let plays = matches!(
            self.st.mode,
            Mode::Scenes | Mode::Inspect | Mode::Validate | Mode::Train
        );
        if self.replay.is_some() && self.st.live.playing && plays {
            self.drive_replay(cx);
            dirty = true;
        }
        // playback: two cheap value writes, no VM applies
        if self.st.live.playing && plays {
            crate::screens::sync_transport_fast(self, cx);
            dirty = true;
        }
        if self.st.mode == Mode::Inspect && self.review.document.is_some() {
            crate::screens::sync_right_rail(self, cx);
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
        self.save_workspace();
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
        self.ui
            .label(cx, ids!(page_title))
            .set_text(cx, &self.t(self.st.mode.label()));
        self.ui.button(cx, ids!(inspector_btn)).set_text(
            cx,
            if self.inspector_open {
                "Hide details"
            } else {
                "Details"
            },
        );
        self.ui
            .widget(cx, ids!(inspector_btn))
            .set_visible(cx, self.st.mode != Mode::Home);

        let modes_bg = self.ui.view(cx, ids!(modes));
        if !modes_bg.is_empty() {
            let mut m = modes_bg.clone();
            script_apply_eval!(cx, m, { draw_bg +: { light: #(l as f64) } });
        }
        let mut root = self.ui.view(cx, ids!(root_bg));
        if !root.is_empty() {
            script_apply_eval!(cx, root, { draw_bg +: { light: #(l as f64) } });
        }
        for (i, id) in [
            ids!(tab0),
            ids!(tab1),
            ids!(tab2),
            ids!(tab3),
            ids!(tab4),
            ids!(tab5),
            ids!(tab6),
            ids!(tab7),
        ]
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
        let mut inspector = self.ui.button(cx, ids!(inspector_btn));
        script_apply_eval!(cx, inspector, {draw_bg +: {light: #(l as f64)} draw_text +: {light: #(l as f64)}});
        // chips
        let running = self.st.runs.iter().find(|r| r.state == RunState::Running);
        let shown = running
            .or_else(|| self.st.runs.iter().find(|r| r.state == RunState::Paused))
            .or_else(|| self.st.sel.run.as_ref().and_then(|id| self.st.run(id)));
        let (state_txt, tone, pulse) = match running {
            Some(r) if r.set == "robot training task" => {
                (format!("training · iteration {}", r.iter), 0.0, true)
            }
            Some(r) => (
                self.tf(
                    "training · stage {0}/{1}",
                    &[&(r.stage + 1).to_string(), &r.stages.len().to_string()],
                ),
                0.0,
                true,
            ),
            None => {
                if let Some(r) = shown {
                    (self.t(r.state.key()), 0.5, r.state == RunState::Paused)
                } else {
                    (self.t("stopped"), 0.5, false)
                }
            }
        };
        let steps = shown.map(|r| r.steps).unwrap_or(0.0);
        let steps_txt = self.tf("{0}M steps", &[&format!("{steps:.2}")]);
        crate::screens::set_chip(
            cx,
            &self.ui,
            ids!(c_state),
            &state_txt,
            Some(tone),
            pulse,
            l,
        );
        crate::screens::set_chip(cx, &self.ui, ids!(c_steps), &steps_txt, None, false, l);
        let live_n = self
            .st
            .deploys
            .iter()
            .filter(|d| d.state == DepState::Live)
            .count();
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
            tb.set_text(
                cx,
                match self.st.theme {
                    Theme::Auto => "Appearance: Auto",
                    Theme::Light => "Appearance: Light",
                    Theme::Dark => "Appearance: Dark",
                },
            );
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
            LAct::SelRobot(id) => {
                self.review = Default::default();
                if self
                    .real_proc
                    .as_ref()
                    .is_some_and(|p| p.kind == nexus::ProcKind::Sweep)
                    && self.st.sel.robot.as_ref() != Some(id)
                {
                    self.st.toast(
                        "Stop the sweep and wait for it to finish before switching robots.".into(),
                    );
                    self.sync_all(cx);
                    return;
                }
                if self.st.sel.robot.as_ref() != Some(id) {
                    self.st.sel.ck = None;
                    self.st.sel.ck_run = None;
                    self.mj = None;
                    self.cross = None;
                    self.preview_key.clear();
                    self.st.sweep_grid = None;
                    self.st.sweep_state = SweepState::Idle;

                    self.replay = None;
                    self.replay_map = None;
                    self.st.replay_driven = false;
                    self.st.live.frames = 0;
                    self.external_motion = ExternalMotionState::default();
                }
                self.st.sel.robot = Some(id.clone());
            }
            LAct::SelRun(id) => {
                self.review = Default::default();
                if self
                    .real_proc
                    .as_ref()
                    .is_some_and(|p| p.kind == nexus::ProcKind::Sweep)
                    && self.st.sel.run.as_ref() != Some(id)
                {
                    self.st.toast(
                        "Stop the sweep and wait for it to finish before switching runs.".into(),
                    );
                    self.sync_all(cx);
                    return;
                }
                if self.st.sel.run.as_ref() != Some(id) {
                    self.st.sel.ck = None;
                    self.st.sel.ck_run = None;
                    self.mj = None;
                    self.cross = None;
                    self.preview_key.clear();
                    self.st.sweep_grid = None;
                    self.st.sweep_state = SweepState::Idle;

                    self.replay = None;
                    self.replay_map = None;
                    self.st.replay_driven = false;
                    self.st.live.frames = 0;
                    self.external_motion = ExternalMotionState::default();
                }
                self.st.sel.run = Some(id.clone());
                self.st.sel.robot = self
                    .st
                    .run(id)
                    .and_then(|r| self.st.snapshot_robot(&r.snapshot));
            }
            LAct::PickCk(rn, l) => {
                self.evaluation.report = None;
                self.evaluation.second = None;
                self.review = Default::default();
                self.st.pick_ck(rn, l);
                self.external_motion = ExternalMotionState::default();
                if self.external_robot_run().is_some() {
                    self.st.mode = Mode::Validate;
                }
                self.mj = None;
                self.cross = None;
                self.preview_key.clear();
                self.replay = None;
                self.replay_map = None;
                self.st.replay_driven = false;
            }
            LAct::DepPickCk(rn, l) => {
                if self.st.demo {
                    self.st.dep_pick_ck(rn, l);
                } else {
                    self.st.pick_ck(rn, l);
                    self.st.mode = Mode::Deploy;
                }
            }
            LAct::SelTarget(id) => {
                if self.deployment.session.is_some() {
                    self.st
                        .toast("Disconnect the current agent before changing target".into());
                } else {
                    self.st.sel_target(id);
                }
            }
            LAct::Replay(id) => {
                if let Err(e) = self.review_action("review-open", id) {
                    self.st.toast(e);
                }
            }
            LAct::SelRecipe(i) => self.st.sel_recipe(*i),
            LAct::ExternalCase(i) => {
                self.external_motion.case = *i;
                self.tick_external_motion(cx);
            }
            LAct::EvaluationCase(i) => {
                if let Err(e) = self.select_evaluation_case(*i) {
                    self.st.toast(e);
                }
            }
            LAct::None => return,
        }
        self.sync_all(cx);
    }

    pub fn dispatch(&mut self, cx: &mut Cx, act: &str, _idx: usize) {
        self.dispatch_named(cx, act, "");
    }

    /// Parse "action:arg" composite ids from spec-built buttons/rows.
    pub fn dispatch_arg(&mut self, cx: &mut Cx, composite: &str) {
        if !self.can_edit() {
            self.sync_all(cx);
            return;
        }
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
                    "ck-onnx" => {
                        self.st.modal = AppModal::ExportPolicy { checkpoint: b };
                        self.sync_all(cx);
                    }
                    "ck-export" => {
                        let source = std::path::Path::new(&b);
                        if source.is_file() {
                            if self.pending_dialog.is_none() {
                                self.pending_dialog =
                                    Some(file_dialogs::Pending::Checkpoint(source.to_path_buf()));
                                cx.open_save_file_dialog(
                                    FileDialog::new()
                                        .set_id(live_id!(checkpoint_export))
                                        .set_title("Export checkpoint".into())
                                        .set_filename(
                                            source
                                                .file_name()
                                                .unwrap()
                                                .to_string_lossy()
                                                .into_owned(),
                                        ),
                                );
                            }
                        } else {
                            self.st.toast("Checkpoint file is unavailable.".into());
                        }
                        self.sync_all(cx);
                    }
                    "ck-del" => {
                        if self.st.demo {
                            self.st.ck_del(&b);
                        } else {
                            self.preview_artifacts(Some(&b), false);
                        }
                        self.sync_all(cx);
                    }
                    "ck-inspect" => {
                        self.st.ck_inspect(&b);
                        self.external_motion = ExternalMotionState::default();
                        if self.external_robot_run().is_some() {
                            self.st.mode = Mode::Validate;
                        }
                        self.mj = None;
                        self.cross = None;
                        self.preview_key.clear();
                        self.replay = None;
                        self.replay_map = None;
                        self.st.replay_driven = false;
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
        if self
            .st
            .task_draft
            .as_ref()
            .is_some_and(|d| self.st.sel.robot.as_ref() == Some(&d.robot_id))
        {
            self.st.modal = AppModal::TaskEditor;
            self.st
                .toast("Save or discard the task draft before starting a new run.".into());
            self.sync_all(cx);
            return;
        }
        let scene = self.st.cur_scene().clone();
        self.st.adopt_real_run("studio run", &scene.name, 400);
        if std::env::var_os("DOROBOT_ATTACH_LOG").is_none() {
            if let Some(id) = self.st.sel.run.clone() {
                if let Some(run) = self.st.run_mut(&id) {
                    run.set = "single scene".into();
                    run.snapshot.seed = scene.seed.clone();
                    run.snapshot.cfg = vec![CfgScene {
                        full: Some(scene.clone()),
                        name: scene.name.clone(),
                        stance: scene.stance,
                        terrain: scene.terrain.clone(),
                        friction: scene.friction,
                        kp: scene.kp,
                        mass: scene.mass,
                        push: scene.push,
                    }];
                }
            }
        }
        self.st.mode = Mode::Train;
        self.begin_launched_run(cx);
    }

    fn begin_launched_run(&mut self, cx: &mut Cx) {
        if !self.can_edit() {
            return;
        }
        let task = self
            .st
            .sel
            .robot
            .as_ref()
            .and_then(|id| self.st.robots.iter().find(|r| &r.id == id))
            .filter(|r| r.use_task)
            .and_then(|r| r.task.clone());
        if let Some(run) = self.st.sel.run.clone().and_then(|id| self.st.run_mut(&id)) {
            if let Err(error) = crate::nexus::configure_external_run(run, task.as_ref()) {
                run.state = RunState::Failed;
                run.diagnosis = Some(error.clone());
                self.st.toast(format!("Training did not start: {error}"));
                self.sync_all(cx);
                return;
            }
        }
        if let Err(error) = self.enqueue_selected() {
            if let Some(id) = self.st.sel.run.clone() {
                if let Some(run) = self.st.run_mut(&id) {
                    run.state = RunState::Failed;
                    run.diagnosis = Some(error.clone());
                }
            }
            self.st.toast(format!("Could not queue training: {error}"));
        }
        self.tick_queue();
        self.sync_all(cx);
    }

    /// Drain progress from the running engine job; harvest the sweep surface.
    fn drain_real_proc(&mut self, cx: &mut Cx) -> bool {
        let Some(proc_) = &mut self.real_proc else {
            return false;
        };
        let mut got = false;
        if let Some(update) = proc_
            .job_update
            .as_ref()
            .and_then(|u| u.lock().ok())
            .map(|u| u.clone())
        {
            if let Some(run) = proc_
                .run_id
                .as_ref()
                .and_then(|id| self.st.runs.iter_mut().find(|r| &r.id == id))
            {
                if let Some(job) = &mut run.job {
                    job.connection_error = update.error.clone();
                    if update.at > 0 {
                        job.last_contact = Some(update.at);
                    }
                }
                if let Some(status) = update.status {
                    run.state = match status.state.as_str() {
                        "Queued" => RunState::Queued,
                        "Starting" => RunState::Starting,
                        "Running" => RunState::Running,
                        "Paused" => RunState::Paused,
                        "Stopping" => RunState::Stopping,
                        "Stopped" => RunState::Stopped,
                        "Completed" => RunState::Completed,
                        "Failed" => RunState::Failed,
                        _ => RunState::Unknown,
                    };
                    if let Some(error) = status.error {
                        run.diagnosis = Some(error);
                    }
                }
                got = true;
            }
        }
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
                    let fresh: Vec<_> = g
                        .samples
                        .iter()
                        .filter(|s| s.step > self.train_seen as u64)
                        .collect();
                    if !fresh.is_empty() {
                        got = true;
                        let last = &fresh[fresh.len() - 1];
                        let envs = g.envs.max(1) as u64;
                        let horizon = if proc_.backend.starts_with("CPU") {
                            32
                        } else if proc_.backend.starts_with("External") {
                            proc_
                                .run_id
                                .as_ref()
                                .and_then(|id| self.st.run(id))
                                .and_then(|run| run.snapshot.training_profile.as_ref())
                                .and_then(|profile| profile["recipe"]["steps_per_env"].as_u64())
                                .unwrap_or(24)
                                .max(1)
                        } else {
                            24
                        };
                        let iter = last.step / (envs * horizon);
                        let steps_m = last.step as f64 / 1.0e6;
                        let best = fresh
                            .iter()
                            .map(|s| s.reward as f64)
                            .fold(f64::NEG_INFINITY, f64::max);
                        if let Some(r) = self
                            .st
                            .runs
                            .iter()
                            .find(|r| Some(&r.id) == proc_.run_id.as_ref())
                            .map(|r| r.id.clone())
                            .and_then(|id| self.st.run_mut(&id))
                        {
                            for sample in &fresh {
                                r.metrics.push(RunMetric {
                                    fall_rate_unmeasured: sample.fall_rate_unmeasured,
                                    step: sample.step,
                                    reward: sample.reward as f64,
                                    fall_rate: sample.fall_rate as f64,
                                    steps_per_sec: sample.steps_per_sec,
                                });
                            }
                            let excess = r.metrics.len().saturating_sub(1200);
                            r.metrics.drain(..excess);
                            r.iter = (iter % r.iters_per.max(1) as u64) as u32;
                            r.stage = ((iter / r.iters_per.max(1) as u64) as usize)
                                % r.stages.len().max(1);
                            r.round = (iter
                                / (r.iters_per.max(1) as u64 * r.stages.len().max(1) as u64))
                                as u32
                                + 1;
                            if proc_.backend.starts_with("External") {
                                r.iter = iter.min(r.iters_per as u64) as u32;
                                r.stage = 0;
                                r.round = 1;
                            }
                            r.steps = steps_m;
                            r.best = Some(r.best.unwrap_or(f64::NEG_INFINITY).max(best));
                        }
                    }
                    if let Some(s) = fresh.last() {
                        self.train_seen = s.step as usize;
                    }
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
                self.st.live.real = !self.st.demo;
                let failure = proc_
                    .train
                    .as_ref()
                    .and_then(|h| h.shared.lock().ok())
                    .and_then(|g| g.error.clone());
                if let Some(r) = proc_
                    .run_id
                    .as_ref()
                    .and_then(|id| self.st.runs.iter_mut().find(|r| &r.id == id))
                {
                    if let Some(e) = failure {
                        r.state = RunState::Failed;
                        r.diagnosis = Some(e);
                    } else if r.state == RunState::Running {
                        r.state = RunState::Completed;
                    }
                    let cps = nexus_engine::ckpt::list(&r.id);
                    for (path, m) in cps {
                        let id = path.to_string_lossy().to_string();
                        if !r.ckpts.iter().any(|c| c.id == id) {
                            r.ckpts.push(Ckpt {
                                score_known: true,
                                id,
                                label: format!("step {}", m.step),
                                score: m.score,
                                st: CkSt::Ord,
                                pname: None,
                            });
                        }
                    }
                    if let Some(path) = proc_
                        .artifact
                        .as_ref()
                        .filter(|p| std::path::Path::new(p).is_file())
                    {
                        if !r.ckpts.iter().any(|c| &c.id == path) {
                            r.ckpts.push(Ckpt {
                                score_known: true,
                                id: path.clone(),
                                label: std::path::Path::new(path)
                                    .file_name()
                                    .map(|n| n.to_string_lossy().into_owned())
                                    .unwrap_or_else(|| "policy".into()),
                                score: r.metrics.last().map(|s| s.reward).or(r.best).unwrap_or(0.0),
                                st: CkSt::Ord,
                                pname: None,
                            });
                        }
                    }
                }
            }
            self.drive_ack += 1;
            let finished_id = proc_.run_id.clone();
            self.real_proc = None;
            self.refresh_artifacts();
            self.st.sync_run_metrics();
            let msg = match kind {
                crate::nexus::ProcKind::Train => {
                    match finished_id.as_ref().and_then(|id| self.st.run(id)) {
                        Some(run) if run.state == RunState::Failed => format!(
                            "Training failed: {}",
                            run.diagnosis.as_deref().unwrap_or("See run diagnosis")
                        ),
                        Some(run) if run.state == RunState::Stopped => {
                            "Training stopped; saved checkpoints are available.".into()
                        }
                        _ => "Training completed.".into(),
                    }
                }
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
            self.st.sync_run_metrics();
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
                _ => self
                    .st
                    .toast("MuJoCo finished — no rollout to replay".into()),
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
        if self.st.mode == Mode::Robots {
            let key = format!("robot-inspector:{:?}", self.loaded_urdf);
            if self.terrain_shown != key {
                use makepad_urdf_player::robot_view::RobotViewWidgetRefExt;
                let rv = self.ui.robot_view(cx, ids!(stage.urdf_wrap.rview));
                if let Some(mut inner) = rv.borrow_mut() {
                    inner.clear_external(cx);
                    inner.clear_terrain(cx);
                    inner.set_base_pose(cx, [0., 0., 0.], [0., 0., 0., 1.]);
                    inner.reset_view(cx);
                    self.terrain_shown = key;
                };
            }
            return;
        }
        // External flat-ground rollouts use simulation world z=0. The URDF
        // inspector's automatically fitted floor is below the rest-pose feet
        // (z=-0.125 for Microduck), so it cannot represent this world.
        let recorded_duck = self.st.mode == Mode::Inspect
            && self.replay.as_ref().is_some_and(|r| !r.base.is_empty())
            && self
                .st
                .sel
                .robot
                .as_ref()
                .and_then(|id| self.st.robots.iter().find(|r| &r.id == id))
                .is_some_and(|r| matches!(r.name.as_str(), "Microduck" | "Open Duck Mini v2"));
        if self.external_robot_run().is_some()
            || recorded_duck
            || self.simulation_matches_selection()
            || self.evaluation_visible()
            || (self.st.mode == Mode::Inspect
                && self
                    .review
                    .document
                    .as_ref()
                    .and_then(|d| d.samples.first())
                    .is_some_and(|s| s.session_id.is_some()))
        {
            if self.terrain_shown == "external-flat" {
                return;
            }
            use makepad_urdf_player::robot_view::{ExternalItem, RobotViewWidgetRefExt};
            let rv = self.ui.robot_view(cx, ids!(stage.urdf_wrap.rview));
            let comparison = self.evaluation_visible() && self.evaluation.second.is_some();
            let rv_b = self.ui.robot_view(cx, ids!(stage.urdf_wrap.rview_b));
            let Some(mut inner) = rv.borrow_mut() else {
                return;
            };
            inner.clear_terrain(cx);
            let mut items = Vec::new();
            for color in 0..2 {
                let mut positions = Vec::new();
                let mut normals = Vec::new();
                let mut indices = Vec::new();
                for x in -40..40_i32 {
                    for y in -40..40_i32 {
                        if (x + y).rem_euclid(2) != color {
                            continue;
                        }
                        let n = (positions.len() / 3) as u32;
                        for (dx, dy) in [(0, 0), (1, 0), (1, 1), (0, 1)] {
                            positions.extend_from_slice(&[
                                (x + dx) as f32 * 0.2,
                                (y + dy) as f32 * 0.2,
                                0.0,
                            ]);
                            normals.extend_from_slice(&[0.0, 0.0, 1.0]);
                        }
                        indices.extend_from_slice(&[n, n + 1, n + 2, n, n + 2, n + 3]);
                    }
                }
                let key = 0x4e585f464c4f4f00 + color as u64;
                inner.upsert_external_mesh(cx, key, &positions, &normals, &indices);
                if comparison {
                    if let Some(mut second) = rv_b.borrow_mut() {
                        second.clear_terrain(cx);
                        second.upsert_external_mesh(cx, key, &positions, &normals, &indices);
                    }
                }
                items.push(ExternalItem {
                    mesh_key: key,
                    transform: [
                        1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
                    ],
                    scale: [1., 1., 1.],
                    color: if color == 0 {
                        [0.20, 0.30, 0.40, 1.]
                    } else {
                        [0.10, 0.20, 0.31, 1.]
                    },
                    metallic: 0.0,
                    roughness: 1.0,
                });
            }
            inner.set_external_items(cx, items.clone());
            if comparison {
                if let Some(mut second) = rv_b.borrow_mut() {
                    second.set_external_items(cx, items);
                }
            }
            self.terrain_shown = "external-flat".into();
            return;
        }
        let sc = crate::nexus::from_ui(&self.st.cur_scene());
        let key = format!(
            "{}|{:.2}|{:.1}|{:x}",
            sc.terrain, sc.terrain_amp, sc.terrain_slope_deg, sc.seed
        );
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
            let Some(mut inner) = rv.borrow_mut() else {
                return;
            };
            inner.clear_external(cx);
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
        let Ok(path) = std::env::var("NEXUS_MJCF") else {
            return;
        };
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
        let Some(mut inner) = rv.borrow_mut() else {
            return;
        };
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
                GeomKind::Sphere { r } => {
                    (h(&"usphere"), [r, r, r], Some(prim::unit_sphere(16, 24)))
                }
                GeomKind::Ellipsoid { half } => {
                    (h(&"usphere"), half, Some(prim::unit_sphere(16, 24)))
                }
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
                    (
                        h(&("plane", q(hx), q(hy))),
                        [1.0; 3],
                        Some(prim::plane(hx, hy)),
                    )
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
                    g.mat[0], g.mat[3], g.mat[6], 0.0, g.mat[1], g.mat[4], g.mat[7], 0.0, g.mat[2],
                    g.mat[5], g.mat[8], 0.0, g.pos[0], g.pos[1], g.pos[2], 1.0,
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
        self.st
            .toast(format!("MJCF on stage: {n} geoms via libmujoco"));
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
        if self.review.document.is_some() && self.st.mode == Mode::Inspect {
            return false;
        }
        if !matches!(self.st.mode, Mode::Train | Mode::Scenes | Mode::Inspect) {
            return false;
        }
        let Some(ckpt) = crate::nexus::selected_artifact(&self.st) else {
            return false;
        };
        if !crate::artifacts::is_safetensors(&ckpt) {
            return false;
        }
        let Ok(meta) = std::fs::metadata(&ckpt) else {
            return false;
        };
        let stamp = meta
            .modified()
            .map(|t| format!("{t:?}"))
            .unwrap_or_default();
        let sc = self.st.cur_scene();
        let key = format!(
            "{ckpt}|{stamp}|{}",
            serde_json::to_string(&sc).unwrap_or_default()
        );
        if self
            .st
            .sel
            .robot
            .as_ref()
            .and_then(|id| self.st.robots.iter().find(|r| &r.id == id))
            .is_some_and(|r| r.name != "Unitree G1-29dof")
        {
            return false;
        }
        // Harvest a finished drive before considering a new one.
        if let Some(job) = &self.preview {
            if !job.done.load(std::sync::atomic::Ordering::Relaxed) {
                return false;
            }
            let job = self.preview.take().unwrap();
            if key != self.preview_key {
                return false;
            }
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
                    self.st.toast(
                        "Policy preview failed or collapsed; no new replay is available.".into(),
                    );
                    return false;
                }
            }
        }
        if key == self.preview_key {
            return false;
        }
        match crate::nexus::spawn_preview(&ckpt, &sc) {
            Some(job) => {
                eprintln!("[preview] driving {ckpt} on '{}'", sc.name);
                self.preview_key = key;
                self.replay = None;
                self.replay_map = None;
                self.st.live.playing = false;
                self.st.live.frames = 0;
                self.preview = Some(job);
            }
            // No drive binary on this machine: nothing will ever arrive, so
            // remember the key rather than probing every tick.
            None => self.preview_key = key,
        }
        false
    }

    fn drive_replay(&mut self, cx: &mut Cx) {
        self.drive_comparison(cx);
        if self.st.mode == Mode::Robots {
            return;
        }
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
            // Small duck roots are low to the ground. Aim at the torso and
            // keep the camera above the floor when Open Duck falls; the
            // displayed joint/root pose still comes directly from physics.
            let robot_name = self
                .st
                .sel
                .robot
                .as_ref()
                .and_then(|id| self.st.robots.iter().find(|r| &r.id == id))
                .map(|r| r.model_name())
                .unwrap_or("");
            let target_z = match robot_name {
                "Microduck" => b[2] + 0.07,
                "Open Duck Mini v2" => (b[2] + 0.10).max(0.15),
                _ => b[2],
            };
            if let Some(mut inner) = rv.borrow_mut() {
                inner.set_base_pose(cx, [b[0], b[1], b[2]], [b[3], b[4], b[5], b[6]]);
                let target = if matches!(
                    robot_name,
                    "Microduck" | "Open Duck Mini v2" | "Unitree G1-29dof"
                ) {
                    [b[0], b[1], target_z]
                } else {
                    inner
                        .robot()
                        .map(|r| {
                            let (lo, hi) = r.bounds_world();
                            let c = (lo + hi) * 0.5;
                            [c.x, c.y, c.z]
                        })
                        .unwrap_or([b[0], b[1], target_z])
                };
                inner.set_camera_target(cx, target);
            }
        } else if let Some(mut inner) = rv.borrow_mut() {
            inner.set_base_pose(cx, [0., 0., 0.], [0., 0., 0., 1.]);
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
        if !self.can_edit() {
            self.sync_all(cx);
            return;
        }
        self.st.edit_field(field, v);
        crate::screens::sync_right_rail(self, cx);
        crate::screens::sync_stage_tl(self, cx);
        self.ui.redraw(cx);
    }

    pub fn slider_committed(&mut self, cx: &mut Cx, field: &str, v: f64) {
        if !self.can_edit() {
            self.sync_all(cx);
            return;
        }
        self.st.edit_field(field, v);
        self.st.resim();
        self.sync_all(cx);
    }

    /// The mockup's ACT dispatcher: every named action in one place.
    pub fn dispatch_named(&mut self, cx: &mut Cx, act: &str, arg: &str) {
        if act == "help" {
            self.st.modal = AppModal::Tour;
            self.sync_all(cx);
            return;
        }
        if matches!(act, "list-next" | "list-prev") {
            self.left_page = if act == "list-next" {
                self.left_page + 1
            } else {
                self.left_page.saturating_sub(1)
            };
            crate::screens::sync_left_rail(self, cx);
            self.redraw_ui(cx);
            return;
        }
        if !self.can_edit() {
            self.sync_all(cx);
            return;
        }
        if act.starts_with("agent-") || (!self.st.demo && act == "target-ping") {
            let action = if act == "target-ping" {
                "agent-connect"
            } else {
                act
            };
            if let Err(error) = self.agent_action(action, arg) {
                self.st.toast(error);
            }
            self.sync_all(cx);
            return;
        }
        if act.starts_with("eval-") {
            if let Err(error) = self.evaluation_action(act, arg) {
                self.st.toast(error);
            }
            self.sync_all(cx);
            return;
        }
        if act.starts_with("sim-")
            || (self.simulation.is_some()
                && matches!(act, "push" | "restart-probe" | "record-probe"))
        {
            let action = match act {
                "push" => "sim-push",
                "restart-probe" => "sim-restart",
                "record-probe" => "sim-save",
                _ => act,
            };
            if let Err(e) = self.simulation_action(action, arg) {
                self.st.toast(e);
            }
            self.sync_all(cx);
            return;
        }
        if matches!(act, "real-train" | "train-set")
            && self
                .st
                .task_draft
                .as_ref()
                .is_some_and(|d| self.st.sel.robot.as_ref() == Some(&d.robot_id))
        {
            self.st.modal = AppModal::TaskEditor;
            self.st
                .toast("Save or discard the task draft before starting a new run.".into());
            self.sync_all(cx);
            return;
        }
        if matches!(
            act,
            "sweep-run" | "backend-profile" | "backend-native" | "backend-cpu"
        ) && self.real_proc.is_some()
        {
            self.st
                .toast("A job is already running; stop it before starting another.".into());
            self.sync_all(cx);
            return;
        }
        if matches!(act, "pause" | "resume" | "stop") && !self.st.demo {
            if let Some(job) = &self.real_proc {
                if let Some(h) = &job.train {
                    let result = match act {
                        "pause" => h.pause(),
                        "resume" => h.resume(),
                        _ => {
                            h.stop();
                            Ok(())
                        }
                    };
                    match result {
                        Ok(()) => {
                            if let Some(r) = job
                                .run_id
                                .as_ref()
                                .and_then(|id| self.st.runs.iter_mut().find(|r| &r.id == id))
                            {
                                if job.job_update.is_none() {
                                    r.state = match act {
                                        "pause" => RunState::Paused,
                                        "resume" => RunState::Running,
                                        _ => RunState::Stopped,
                                    };
                                }
                            }
                            self.st.toast(format!("{act} requested"));
                        }
                        Err(e) => self.st.toast(e),
                    }
                }
            } else {
                self.st.toast("No active training job.".into());
            }
            self.sync_all(cx);
            return;
        }
        if act.starts_with("queue-") && !self.st.demo {
            if let Err(error) = self.queue_action(act, arg) {
                self.st.toast(error);
            }
            self.sync_all(cx);
            return;
        }
        if act.starts_with("review-") && !self.st.demo {
            if let Err(e) = self.review_action(act, arg) {
                self.st.toast(format!("Review: {e}"));
            }
            self.sync_all(cx);
            return;
        }
        if act == "record-probe" && !self.st.demo {
            let result = if let Some(doc) = self.review.document.clone() {
                self.register_review(doc)
            } else if let Some(replay) = self.replay.clone() {
                self.st
                    .sel
                    .robot
                    .as_ref()
                    .and_then(|id| self.st.robots.iter().find(|r| &r.id == id))
                    .ok_or_else(|| "Select the replay robot".to_string())
                    .and_then(|r| {
                        crate::review::Document::from_replay(
                            replay,
                            Some(r.id.clone()),
                            r.model_name().into(),
                            self.st.sel.run.clone(),
                        )
                    })
                    .and_then(|doc| self.register_review(doc))
            } else {
                Err("Open a policy replay or import the training recording first".into())
            };
            match result {
                Ok(()) => self
                    .st
                    .toast("Recording saved with provenance and reversible episode edits.".into()),
                Err(e) => self.st.toast(e),
            }
            self.sync_all(cx);
            return;
        }
        let st = &mut self.st;
        match act {
            "external-validation" => st.mode = Mode::Validate,
            "external-live" => st.mode = Mode::Train,
            "validation-older" | "validation-newer" | "validation-latest" => {
                let last = self.external_motion.validations.len().saturating_sub(1);
                self.external_motion.checkpoint = match act {
                    "validation-older" => (self.external_motion.checkpoint + 1).min(last),
                    "validation-newer" => self.external_motion.checkpoint.saturating_sub(1),
                    _ => 0,
                };
            }
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
                let Some(checkpoint) = crate::nexus::selected_artifact(st) else {
                    st.toast("Select a run with a saved checkpoint.".into());
                    self.sync_all(cx);
                    return;
                };
                self.st.sweep_ckpt = checkpoint.clone();
                match crate::nexus::run_sweep(&checkpoint) {
                    Ok(p) => {
                        self.real_proc = Some(p);
                        self.st.sweep_grid = None;
                        self.st.sweep_at = 0;
                        self.st.sel.cell = None;
                        self.st.sweep_state = SweepState::Running;
                        self.st
                            .toast("Sweep started — 40 cells on the linked engine".into());
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
                let Some(ckpt) = crate::nexus::selected_artifact(st) else {
                    st.toast("Select a G1 checkpoint to validate.".into());
                    self.sync_all(cx);
                    return;
                };
                if !crate::artifacts::is_safetensors(&ckpt) {
                    st.toast("MuJoCo G1 validation requires a G1 safetensors checkpoint. Duck policies use their robot validation tab.".into());
                    self.sync_all(cx);
                    return;
                }
                match crate::nexus::run_mujoco(&ckpt, self.st.cur_scene().vx as f32, 45) {
                    Ok(job) => {
                        // The rollout arrives later and drives the 3D robot;
                        // clear whatever is playing so the old motion is not
                        // showing under the new run's numbers.
                        self.replay = None;
                        self.replay_map = None;
                        self.st.replay_driven = false;
                        self.mj = Some(job);
                        self.st
                            .toast("MuJoCo sim2sim started — a second engine, ~45 s".into());
                    }
                    Err(e) => self.st.toast(e),
                }
                self.sync_all(cx);
                return;
            }
            // The weaker arm, and labelled as such. Until this existed the
            // right rail showed four invented numbers under the same heading.
            "cross-run" => {
                let Some(checkpoint) = crate::nexus::selected_artifact(st) else {
                    st.toast("Select a checkpoint to validate.".into());
                    self.sync_all(cx);
                    return;
                };
                match crate::nexus::run_cross(&checkpoint) {
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
            "sweep-stop" => {
                st.sweep_stop();
                if let Some(job) = &self.real_proc {
                    job.stop();
                }
                st.toast(
                    "Stopping sweep after the current cell; partial results will be retained."
                        .into(),
                );
            }
            "sweep-del" => {
                if self
                    .real_proc
                    .as_ref()
                    .is_some_and(|p| p.kind == nexus::ProcKind::Sweep)
                {
                    st.toast(
                        "Stop the sweep and wait for it to finish before removing results.".into(),
                    );
                } else {
                    st.sweep_del();
                }
            }
            "cell-inspect" => st.cell_inspect(),
            "cell-scene" => st.cell_scene(),
            "clear-cell" => st.cell_phys = None,
            // probe
            "push" => {
                let m =
                    "Start live physics in Inspect to apply a force to the selected policy.".into();
                st.toast(m);
            }
            "restart-probe" => {
                st.live.frame = 0;
                st.live.playing = true;
                let m = "Replay restarted.".into();
                st.toast(m);
            }
            // runs
            "rerun" => st.modal = AppModal::Rerun,
            "continue-run" => st.modal = AppModal::ContinueRun,
            "open-run" => st.mode = Mode::Train,
            "run-archive" => st.run_archive(),
            "run-delete" => {
                if self.st.demo {
                    self.st.modal = AppModal::DeleteRun;
                } else {
                    self.preview_artifacts(None, true);
                }
            }
            "export-cancel" => {
                self.export_job = None;
                self.st.toast("Policy export cancelled".into());
            }
            "artifacts-import-runs" => {
                match crate::artifacts::discover_runs(&mut self.st, &crate::artifacts::runs_root())
                {
                    Ok(n) => self
                        .st
                        .toast(format!("Recovered {n} saved run records from disk")),
                    Err(e) => self.st.toast(format!("Run discovery failed: {e}")),
                }
            }
            "artifacts-refresh" => self.refresh_artifacts(),
            "ck-page-prev" => self.checkpoint_page = self.checkpoint_page.saturating_sub(1),
            "ck-page-next" => {
                self.checkpoint_page = (self.checkpoint_page + 1).min(
                    self.st
                        .sel
                        .run
                        .as_ref()
                        .and_then(|id| self.st.run(id))
                        .map(|r| r.ckpts.len().saturating_sub(1) / 2)
                        .unwrap_or(0),
                )
            }
            "artifacts-cleanup" => self.st.modal = AppModal::ArtifactRetention,
            "artifacts-trash" => self.st.modal = AppModal::Trash { page: 0 },
            // robots
            "import-robot" => {
                st.modal = AppModal::ImportRobot {
                    path: String::new(),
                    summary: None,
                }
            }
            "robot-export" => st.modal = AppModal::ExportRobot,
            "robots-discover" => self.discover_robot_assets(),
            "backend-native" | "backend-cpu" => {
                if let Some(r) = st
                    .sel
                    .robot
                    .as_ref()
                    .and_then(|id| st.robots.iter_mut().find(|r| &r.id == id))
                {
                    r.use_task = false;
                }
                std::env::remove_var("NEXUS_TRAINING_PROFILE");
                std::env::set_var(
                    "NEXUS_BACKEND",
                    if act == "backend-cpu" {
                        "cpu"
                    } else {
                        "zealot"
                    },
                );
                st.toast(
                    if act == "backend-cpu" {
                        "CPU cart-pole learner selected."
                    } else {
                        "Native Unitree G1 trainer selected."
                    }
                    .into(),
                );
            }
            "backend-profile" => {
                if self.pending_dialog.is_none() {
                    self.pending_dialog =
                        Some(file_dialogs::Pending::Profile(st.sel.robot.clone()));
                    cx.open_select_file_dialog(
                        FileDialog::new()
                            .set_id(live_id!(training_profile))
                            .set_title("Choose robot RL training profile".into())
                            .add_filter("Training profile".into(), vec!["json".into()]),
                    );
                }
            }
            "robot-rename" => st.modal = AppModal::RenameRobot,
            "robot-remap" | "task-edit" => {
                if let Err(e) = crate::task_editor::start(st) {
                    st.toast(e);
                }
            }
            "task-specialized" => {
                if let Err(e) = crate::task_editor::start_specialized(st) {
                    st.toast(e);
                }
            }
            "task-generic" => {
                if let Err(error) = crate::task_editor::start_generic(&mut self.st) {
                    self.st.toast(error);
                }
            }
            "task-profile" => {
                if let Some(r) = st
                    .sel
                    .robot
                    .as_ref()
                    .and_then(|id| st.robots.iter_mut().find(|r| &r.id == id))
                {
                    r.use_task = false;
                }
                st.toast("Trainer profile selected; the editable task is retained.".into());
            }
            "robot-remove" => st.modal = AppModal::RemoveRobot,
            // deploy
            "target-new" => st.modal = AppModal::TargetForm { edit: None },
            "target-edit" => {
                let id = st.dg.target.clone();
                st.modal = AppModal::TargetForm { edit: id };
            }
            "target-del" => {
                if self.deployment.session.is_some() {
                    st.toast("Disconnect the deployment agent before removing a target.".into());
                } else {
                    st.target_del();
                }
            }
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
        stringWithUTF8String: b"background training monitor + ui soak driver\0".as_ptr()
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
