//! dorobot-nexus — a native training studio for GPU reinforcement learning.
//!
//! The simulator and the console are meant to be one process on one GPU: nexus
//! renders physics on wgpu and makepad renders on the GPU, so the viewport is
//! the simulation rather than a video of it. That is what makes single-stepping,
//! perturbing a running policy and zero-latency scrubbing possible at all.
//!
//! The trainer is not attached yet. Metrics are generated and every screen that
//! shows them says so, because a plausible curve that nothing produced is the
//! most expensive lie a tool like this can tell.

use makepad_widgets::*;

mod ckpt;
mod crosssim;
mod env;
mod json;
mod plot;
mod probe;
mod screens;
mod rl;
mod rng;
mod scene;
mod state;
mod sweep;
mod trainer;
mod ux;
/// Always compiled — it is pure std and pulls in nothing. The `zealot` feature
/// decides whether the app *prefers* this backend, not whether it exists; that
/// keeps the state it owns out of `#[cfg]` on `App`'s fields, which the Script
/// derive does not accept.
#[allow(dead_code)]
mod zealot;

use screens::{
    inspect::InspectScreenWidgetRefExt, runs::RunsScreenWidgetRefExt,
    scene::SceneScreenWidgetRefExt, task::TaskScreenWidgetRefExt,
    train::TrainScreenWidgetRefExt, validate::ValidateScreenWidgetRefExt,
};
use state::Studio;
use ux::Screen;
// Imported bare, not written as `zealot::RolloutSlot` in the field: the Script
// derive rejects a qualified path type with "Unexpected field form".
use scene::Scene;
use zealot::RolloutSlot;

/// Playback tick, in seconds. 30 Hz: smooth enough for a gait, and cheap
/// beside the metric poll.
const ANIM_TICK: f32 = 0.033;

/// makepad's `app_main!` generates the crate's `fn main`, and by the time that
/// hands control to our code it has already run `Cx::init_log` and
/// `init_websockets` — both of which write to **stdout**. That is fatal to a
/// machine-readable stream: framework chatter would land inside the JSON a
/// consumer is parsing. It also means every headless invocation starts a GUI
/// stack it never uses, which on a machine with no window server is a failure
/// rather than an overhead.
///
/// Invoking the macro inside a module demotes its `main` to `shell::main`,
/// which nothing calls, and leaves the crate's real entry point below: check
/// for a headless flag first, hand control to makepad only if there wasn't one.
#[allow(dead_code)]
mod shell {
    use super::*;
    app_main!(App);
}

fn main() {
    // Never returns if a headless flag was given.
    maybe_headless();
    shell::app_main();
}

/// Playback tick, in seconds. 30 Hz: smooth enough for a gait, and cheap
/// beside the metric poll.
const ANIM_TICK: f32 = 0.033;

fn new_state() -> Studio {
    Studio::new()
}

script_mod! {
    use mod.prelude.widgets.*
    use mod.widgets.*
    use mod.widgets.ux.*

    startup() do #(App::script_component(vm)){
        ui: Root{
            main_window := Window{
                window.title: "dorobot-nexus"
                window.inner_size: vec2(1440, 940)
                pass.clear_color: #x0E1119

                body +: {
                    width: Fill height: Fill flow: Down

                    app_bar := mod.widgets.ux.AppBar{}

                    work := View{
                        width: Fill height: Fill flow: Right

                        nav := mod.widgets.ux.NavRail{}

                        pages := View{
                            width: Fill height: Fill flow: Overlay
                            page_runs     := RunsScreen{ visible: false }
                            page_scene    := SceneScreen{ visible: false }
                            page_task     := TaskScreen{ visible: false }
                            page_train    := TrainScreen{}
                            page_inspect  := InspectScreen{ visible: false }
                            page_validate := ValidateScreen{ visible: false }
                        }
                    }
                }
            }
        }
    }
}

#[derive(Script, ScriptHook)]
pub struct App {
    #[live]
    ui: WidgetRef,
    #[rust(new_state())]
    app: Studio,
    /// The live run. The UI never calls into it; it reads snapshots.
    #[rust]
    trainer: Option<trainer::Handle>,
    #[rust]
    poll: Timer,
    /// Playback runs on its own interval. The metric poll is deliberately slow;
    /// driving a 50 Hz rollout from it would play the gait at a tenth speed,
    /// which reads as a broken policy rather than a slow clock.
    #[rust]
    anim: Timer,
    /// Fractional frames carried between ticks, so playback runs at the
    /// rollout's real rate instead of rounding to whole frames each tick.
    #[rust]
    anim_debt: f32,
    /// A checkpoint being driven in Inspect.
    #[rust]
    probe: Option<probe::Probe>,
    #[rust]
    surface: Option<std::sync::Arc<std::sync::Mutex<sweep::Surface>>>,
    #[rust]
    cross: Option<std::sync::Arc<std::sync::Mutex<crosssim::Report>>>,
    /// A zealot rollout playing in the Scene view, filled on a worker thread:
    /// rolling a checkpoint out runs a GPU simulation and takes seconds.
    #[rust]
    rollout: RolloutSlot,
    #[rust]
    rollout_frame: usize,
    #[rust(true)]
    rollout_playing: bool,
    #[rust]
    urdf_joints: Vec<String>,
    /// The simulation configuration the Scene screen is showing.
    #[rust]
    scene: Scene,
    #[rust(false)]
    terrain_loaded: bool,
}

impl App {
    /// The checkpoint the zealot backend trains into, and therefore the one
    /// the sweep and the sim-to-sim comparison roll out. Kept in one place so
    /// the three screens cannot drift onto different files.
    /// Roll the checkpoint out on the current terrain, off-thread.
    #[cfg(feature = "zealot")]
    fn respawn_rollout(&mut self) {
        let ckpt = self.zealot_ckpt();
        if !zealot::drive_path().is_file() || !std::path::Path::new(&ckpt).is_file() {
            return;
        }
        let out = self.rollout.clone();
        let knobs = self.scene.knobs();
        let drive = zealot::Drive {
            vx: self.scene.vx,
            vy: self.scene.vy,
            yaw_rate: self.scene.yaw,
            seconds: self.scene.seconds,
        };
        let family = self.scene.terrain.clone();
        // Clear first: the old rollout is on the wrong terrain, and showing it
        // under new ground would be a lie the screen cannot detect.
        out.set(None);
        self.rollout_frame = 0;
        std::thread::spawn(move || {
            out.set(zealot::drive(&ckpt, drive, &knobs));
            match out.len() {
                0 => eprintln!("scene: rollout on '{family}' failed"),
                n => eprintln!("scene: rollout on '{family}' loaded, {n} frames"),
            }
        });
    }

    /// Re-read the scene and recording libraries from disk and show them.
    #[cfg(feature = "zealot")]
    fn refresh_library(&mut self, cx: &mut Cx) {
        let scenes = scene::list();
        let recs = scene::Recording::list();
        let active = self.scene.name.clone();
        self.ui
            .scene_screen(cx, ids!(page_scene))
            .show_library(cx, &scenes, &recs, &active);
    }

    /// Save the loaded rollout as a replayable recording, tagged with the
    /// scene that produced it.
    #[cfg(feature = "zealot")]
    fn record_rollout(&mut self) {
        let Some(r) = self.rollout.snapshot() else {
            eprintln!("record: nothing loaded");
            return;
        };
        let dir = scene::Recording::dir();
        if let Err(e) = std::fs::create_dir_all(&dir) {
            eprintln!("record: {e}");
            return;
        }
        // Name from the scene plus a counter, so recording twice does not
        // silently overwrite the first take.
        let base = scene::slug(&self.scene.name);
        let n = scene::Recording::list().len() + 1;
        let name = format!("{base}-{n:03}");
        let rollout_path = dir.join(format!("{name}.rollout.json"));
        if let Err(e) = std::fs::write(&rollout_path, r.to_json()) {
            eprintln!("record: {e}");
            return;
        }
        let dist = if r.len() > 1 {
            let (a, b) = (r.base[0], r.base[r.len() - 1]);
            ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt()
        } else {
            0.0
        };
        let rec = scene::Recording {
            name: name.clone(),
            scene: self.scene.name.clone(),
            frames: r.len(),
            resets: r.resets.len(),
            distance: dist,
            rollout: rollout_path,
        };
        match rec.save() {
            Ok(p) => eprintln!("record: {} -> {}", rec.summary(), p.display()),
            Err(e) => eprintln!("record: {e}"),
        }
    }

    #[cfg(feature = "zealot")]
    fn zealot_ckpt(&self) -> String {
        zealot_ckpt_path()
    }

    /// Repaint every screen from the current snapshot.
    fn sync(&mut self, cx: &mut Cx) {
        let nav = self.ui.widget(cx, ids!(nav));
        ux::sync_nav(cx, &nav, self.app.screen);

        let run = self.app.run().clone();
        let robot = self.app.robot().clone();
        let ckpt = run
            .checkpoints
            .first()
            .map(|c| c.name())
            .unwrap_or_else(|| "no checkpoint".into());

        self.ui.runs_screen(cx, ids!(page_runs)).sync(cx, &self.app.runs, self.app.selected);
        self.ui.scene_screen(cx, ids!(page_scene)).sync(cx, &robot);
        self.ui.task_screen(cx, ids!(page_task)).sync(cx, &run);
        self.ui.train_screen(cx, ids!(page_train)).sync(cx, &run);
        self.ui.inspect_screen(cx, ids!(page_inspect)).sync(cx, self.probe.as_ref());
        let surf = self.surface.as_ref().and_then(|s| s.lock().ok());
        let xsim = self.cross.as_ref().and_then(|c| c.lock().ok());
        self.ui
            .validate_screen(cx, ids!(page_validate))
            .sync(cx, surf.as_deref(), xsim.as_deref());
        drop(xsim);
        drop(surf);

        let live = self
            .trainer
            .as_ref()
            .and_then(|h| h.shared.lock().ok().map(|g| (g.running, g.samples.len())));
        let status = match live {
            Some((true, n)) => format!("cpu · training · {n} intervals"),
            Some((false, _)) => "cpu · run finished".to_string(),
            None => "idle".to_string(),
        };
        self.ui
            .label(cx, ids!(app_bar.device))
            .set_text(cx, &format!("{} · {status}", device_line()));

        // Overlay flow: exactly one page is visible.
        for (path, screen) in [
            (ids!(page_runs) as &[LiveId], Screen::Runs),
            (ids!(page_scene), Screen::Scene),
            (ids!(page_task), Screen::Task),
            (ids!(page_train), Screen::Train),
            (ids!(page_inspect), Screen::Inspect),
            (ids!(page_validate), Screen::Validate),
        ] {
            self.ui.widget(cx, path).set_visible(cx, screen == self.app.screen);
        }
    }
}

/// The GPU backend line in the app bar. A status line, not a screen — which is
/// where the multi-backend build belongs.
fn device_line() -> &'static str {
    if cfg!(target_os = "macos") {
        "metal"
    } else if cfg!(target_os = "windows") {
        "vulkan"
    } else {
        "vulkan"
    }
}

impl MatchEvent for App {
    fn handle_startup(&mut self, cx: &mut Cx) {
        // 256 environments for 4M steps: enough that the balance task is solved
        // while you watch, which is the point of having the screen at all.
        //
        // With --features zealot, the same screens are fed by zealot's GPU
        // biped trainer when its stack has been built; the CPU learner remains
        // the fallback, so a missing build degrades to a working app rather
        // than an empty one.
        #[cfg(feature = "zealot")]
        {
            self.trainer = zealot::spawn(256, 2_000, "dorobot_nexus.safetensors");
            if self.trainer.is_some() {
                ::log::info!("backend: zealot ({})", zealot::binary_path().display());
            } else {
                ::log::warn!(
                    "zealot binary not found at {} — falling back to the CPU trainer \
                     (run scripts/setup-zealot.sh to build it)",
                    zealot::binary_path().display()
                );
            }
        }
        if self.trainer.is_none() {
            self.trainer = Some(trainer::spawn(256, 4_000_000, 1));
        }

        // Roll the checkpoint out once, off-thread, so the Scene view shows the
        // policy walking rather than a mannequin. Absent stack or checkpoint
        // simply leaves the model static.
        #[cfg(feature = "zealot")]
        {
            // Initial terrain family; the Scene picker changes it later.
            self.scene = Scene::default();
            self.scene.terrain = std::env::var("DOROBOT_TERRAIN").unwrap_or_default();
            // One spawn path, so startup and a knob change cannot drift.
            self.respawn_rollout();
        }
        self.poll = cx.start_interval(0.2);
        self.anim = cx.start_interval(ANIM_TICK as f64);
        self.sync(cx);
    }

    fn handle_timer(&mut self, cx: &mut Cx, e: &TimerEvent) {
        // Playback has its own clock, and runs whether or not the trainer has
        // published anything yet.
        #[cfg(feature = "zealot")]
        if self.anim.is_timer(e).is_some() {
            let dt = self.rollout.dt().max(1e-3);
            self.anim_debt += ANIM_TICK / dt;
            let step = self.anim_debt.floor();
            self.anim_debt -= step;
            if self.rollout_playing {
                self.rollout_frame = self.rollout_frame.wrapping_add(step as usize);
            }
            let total = self.rollout.len();
            if total > 0 {
                let frame = self.rollout_frame % total;
                let playing = self.rollout_playing;
                self.ui
                    .scene_screen(cx, ids!(page_scene))
                    .show_playback(cx, frame, total, playing);
            }
            // Terrain is static: load it once, after the URDF is up so the
            // viewer exists to receive it.
            if !self.terrain_loaded && !self.urdf_joints.is_empty() {
                self.terrain_loaded = true;
                let mesh = zealot::ensure_terrain_mesh(&self.scene);
                let n = self
                    .ui
                    .scene_screen(cx, ids!(page_scene))
                    .set_terrain(cx, mesh.as_deref());
                if n > 0 {
                    eprintln!("scene: terrain '{}' loaded, {n} triangles", self.scene.terrain);
                }
                let sc = self.scene.clone();
                let scene = self.ui.scene_screen(cx, ids!(page_scene));
                scene.show_terrain(cx, &sc.terrain);
                scene.show_knobs(cx, &sc);
                self.refresh_library(cx);
            }
            // The viewer owns the joint ordering; ask it once the model is up.
            if self.urdf_joints.is_empty() {
                self.urdf_joints = self
                    .ui
                    .scene_screen(cx, ids!(page_scene))
                    .movable_joint_names(cx);
            }
            if let Some(pose) = self.rollout.pose(self.rollout_frame, &self.urdf_joints) {
                let base = self.rollout.base(self.rollout_frame);
                self.ui
                    .scene_screen(cx, ids!(page_scene))
                    .set_pose(cx, &pose, base);
            }
        }

        if self.poll.is_timer(e).is_none() {
            return;
        }

        let Some(h) = &self.trainer else { return };
        let (samples, envs, total) = {
            let Ok(g) = h.shared.lock() else { return };
            (g.samples.clone(), g.envs as u32, g.total_steps)
        };
        if samples.is_empty() {
            return;
        }
        // Same Run shape as the fixtures, so every screen and the diagnosis
        // catalogue work on live data without knowing the difference.
        self.app.runs[0] = state::live_run(&samples, envs, total, 1);

        // Pick up the first checkpoint the trainer writes, then play it.
        if self.probe.is_none() {
            self.probe = probe::Probe::load_latest(trainer::RUN_ID);
        }
        if let Some(p) = self.probe.as_mut() {
            p.tick();
        }
        // The sweep fills in cell by cell; the same tick shows its progress.
        self.sync(cx);
        self.ui.redraw(cx);
    }

    fn handle_actions(&mut self, cx: &mut Cx, actions: &Actions) {
        let mut dirty = false;

        let nav = self.ui.widget(cx, ids!(nav));
        for (path, screen) in ux::NAV_ITEMS {
            let item = nav.widget(cx, path);
            if item.is_empty() {
                continue;
            }
            if ux::view_clicked(actions, item.widget_uid()) {
                self.app.screen = screen;
                dirty = true;
            }
        }

        // Selecting a run changes what every other screen is about.
        if let Some(i) = self.ui.runs_screen(cx, ids!(page_runs)).clicked(cx, actions) {
            self.app.selected = i;
            self.app.screen = Screen::Train;
            dirty = true;
        }

        // Inspect's transport. Because the simulation is in this process, each
        // of these is a function call rather than a script and a re-run.
        if let Some(t) = self
            .ui
            .inspect_screen(cx, ids!(page_inspect))
            .transport(cx, actions)
        {
            if self.probe.is_none() {
                self.probe = probe::Probe::load_latest(trainer::RUN_ID);
            }
            if let Some(p) = self.probe.as_mut() {
                use screens::inspect::Transport as T;
                match t {
                    T::Play => p.toggle_play(),
                    T::StepBack => p.step_by(-1),
                    T::StepForward => p.step_by(1),
                    // Restart also pulls in any newer checkpoint, so a probe
                    // left open while training continues is not stuck on the
                    // policy it happened to load first.
                    T::Restart => {
                        match probe::Probe::load_latest(trainer::RUN_ID) {
                            Some(fresh) => *p = fresh,
                            None => p.restart(),
                        }
                    }
                    T::Push => p.push(1.6),
                    T::Seek(f) => p.seek_fraction(f),
                }
                dirty = true;
            }
        }

        if self.ui.validate_screen(cx, ids!(page_validate)).clicked_run(cx, actions) {
            // Prefer zealot's GPU biped when its stack is built; its sweep
            // varies real PD gain and ground friction rather than a cart-pole's
            // force and mass, and falls back rather than showing an empty grid.
            #[cfg(feature = "zealot")]
            let started = sweep::zealot_sweep::spawn(&self.zealot_ckpt());
            #[cfg(not(feature = "zealot"))]
            let started: Option<_> = None;

            match started.or_else(|| sweep::spawn(trainer::RUN_ID)) {
                Some(s) => self.surface = Some(s),
                None => ::log::info!("sweep: no checkpoint to sweep yet"),
            }
            dirty = true;
        }

        if self.ui.validate_screen(cx, ids!(page_validate)).clicked_cross(cx, actions) {
            #[cfg(feature = "zealot")]
            let started = crosssim::zealot_cross::spawn(&self.zealot_ckpt());
            #[cfg(not(feature = "zealot"))]
            let started: Option<_> = None;

            match started.or_else(|| crosssim::spawn(trainer::RUN_ID)) {
                Some(c) => self.cross = Some(c),
                None => ::log::info!("cross-sim: no checkpoint yet"),
            }
            dirty = true;
        }

        #[cfg(feature = "zealot")]
        if let Some(act) = self.ui.scene_screen(cx, ids!(page_scene)).library_action(cx, actions) {
            use screens::scene::Library;
            match act {
                Library::Save => {
                    // Name it for what makes it distinct, so a library of
                    // scenes is readable without opening them.
                    let terrain = if self.scene.terrain.is_empty() { "flat" } else { &self.scene.terrain };
                    self.scene.name = format!(
                        "{terrain}-f{:.2}-kp{:.2}{}",
                        self.scene.friction,
                        self.scene.kp_scale,
                        if self.scene.push_vel > 0.0 { "-push" } else { "" }
                    );
                    match self.scene.save() {
                        Ok(p) => eprintln!("scene: saved {} -> {}", self.scene.name, p.display()),
                        Err(e) => eprintln!("scene: save failed: {e}"),
                    }
                }
                Library::LoadScene(i) => {
                    if let Some(sc) = scene::list().get(i).cloned() {
                        eprintln!("scene: loaded '{}' ({})", sc.name, sc.summary());
                        self.scene = sc;
                        let sc = self.scene.clone();
                        let screen = self.ui.scene_screen(cx, ids!(page_scene));
                        screen.show_knobs(cx, &sc);
                        screen.show_terrain(cx, &sc.terrain);
                        screen.set_terrain(cx, zealot::ensure_terrain_mesh(&sc).as_deref());
                        self.respawn_rollout();
                    }
                }
                Library::Replay(i) => {
                    if let Some(rec) = scene::Recording::list().get(i).cloned() {
                        match std::fs::read_to_string(&rec.rollout) {
                            Ok(t) if self.rollout.load_json(&t) => {
                                self.rollout_frame = 0;
                                self.rollout_playing = true;
                                eprintln!("scene: replaying {}", rec.summary());
                            }
                            Ok(_) => eprintln!("scene: {} is not a readable rollout", rec.name),
                            Err(e) => eprintln!("scene: replay failed: {e}"),
                        }
                    }
                }
            }
            self.refresh_library(cx);
            dirty = true;
        }

        #[cfg(feature = "zealot")]
        if let Some((knob, value)) = self.ui.scene_screen(cx, ids!(page_scene)).knob_changed(cx, actions) {
            use screens::scene::Knob;
            let reshapes = matches!(knob, Knob::TerrainAmp | Knob::TerrainSlope);
            knob.set(&mut self.scene, value);
            let sc = self.scene.clone();
            let screen = self.ui.scene_screen(cx, ids!(page_scene));
            screen.show_knobs(cx, &sc);
            if reshapes {
                // New geometry: regenerate and reload, or the ground on screen
                // would be a different terrain from the one simulated.
                screen.set_terrain(cx, zealot::ensure_terrain_mesh(&sc).as_deref());
            }
            // Re-roll: the displayed trajectory was produced under the old
            // physics and no longer describes this scene.
            self.respawn_rollout();
            dirty = true;
        }

        #[cfg(feature = "zealot")]
        if let Some(t) = self.ui.scene_screen(cx, ids!(page_scene)).transport(cx, actions) {
            use screens::scene::Play;
            let total = self.rollout.len().max(1);
            match t {
                Play::Toggle => self.rollout_playing = !self.rollout_playing,
                Play::StepBack => {
                    self.rollout_playing = false;
                    self.rollout_frame = self.rollout_frame.saturating_sub(1);
                }
                Play::StepForward => {
                    self.rollout_playing = false;
                    self.rollout_frame = self.rollout_frame.wrapping_add(1);
                }
                Play::Restart => {
                    self.rollout_frame = 0;
                    self.rollout_playing = true;
                }
                Play::Seek(f) => {
                    self.rollout_playing = false;
                    self.rollout_frame = ((f.clamp(0.0, 1.0) * (total - 1) as f64) as usize).min(total - 1);
                }
                Play::Record => {
                    self.record_rollout();
                    self.refresh_library(cx);
                }
            }
            dirty = true;
        }

        #[cfg(feature = "zealot")]
        if let Some(family) = self.ui.scene_screen(cx, ids!(page_scene)).clicked_terrain(cx, actions) {
            if family != self.scene.terrain {
                self.scene.terrain = family.to_string();
                let scene = self.ui.scene_screen(cx, ids!(page_scene));
                scene.show_terrain(cx, family);
                scene.set_terrain(cx, zealot::ensure_terrain_mesh(&self.scene).as_deref());
                self.respawn_rollout();
            }
            dirty = true;
        }

        if self.ui.scene_screen(cx, ids!(page_scene)).clicked_add(cx, actions) {
            // The import flow is designed but not built; saying so beats a
            // control that looks live and does nothing.
            ::log::info!("add robot: import flow not implemented yet");
        }

        if dirty {
            self.sync(cx);
            self.ui.redraw(cx);
        }
    }
}

impl AppMain for App {
    fn script_mod(vm: &mut ScriptVm) -> ScriptValue {
        makepad_widgets::script_mod(vm);
        // RobotView draws into an XR scene, so both register before anything
        // that mounts one.
        makepad_urdf_player::makepad_xr::script_mod(vm);
        makepad_urdf_player::script_mod(vm);
        makepad_app_shell::script_mod(vm);
        ux::script_mod(vm);
        plot::script_mod(vm);
        screens::script_mod(vm);
        self::script_mod(vm)
    }

    fn handle_event(&mut self, cx: &mut Cx, event: &Event) {
        self.match_event(cx, event);
        self.ui.handle_event(cx, event, &mut Scope::empty());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sample_names_its_reward_terms() {
        // `Sample::terms` is positional. Pairing it with TERM_NAMES at the edge
        // means a consumer never has to know the order, and a reweighting
        // cannot silently relabel a column.
        let s = trainer::Sample {
            step: 8192,
            reward: 0.5,
            terms: vec![0.1, 0.2, 0.3, 0.4],
            fall_rate: 0.25,
            steps_per_sec: 60000.0,
            episode_len: 24.0,
            leans: vec![],
        };
        let out = sample_json(&s);
        assert!(out.contains(r#""event":"sample""#));
        assert!(out.contains(r#""step":8192"#));
        assert!(out.contains(r#""track_lin_vel":0.10000"#), "{out}");
        assert!(out.contains(r#""torque":0.40000"#), "{out}");
    }

    #[test]
    fn a_sample_with_fewer_terms_than_names_omits_rather_than_invents() {
        // A checkpoint from an older reward shape must not have its missing
        // terms reported as zero — an absent term and a zero term differ.
        let s = trainer::Sample { terms: vec![0.1], ..Default::default() };
        let out = sample_json(&s);
        assert!(out.contains("track_lin_vel"));
        assert!(!out.contains("torque"), "{out}");
    }

    #[test]
    fn a_non_finite_metric_does_not_break_the_stream() {
        // A diverged run produces NaN. Emitting the literal `NaN` would make the
        // line unparseable, taking the whole stream down with it.
        let s = trainer::Sample { reward: f32::NAN, ..Default::default() };
        assert!(sample_json(&s).contains(r#""reward":null"#));
    }

    #[test]
    fn a_flag_is_never_read_as_a_run_id() {
        // `--probe --json` must probe the default run, not a run called "--json".
        let args: Vec<String> = ["dorobot-nexus", "--probe", "--json"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(run_arg(&args, 1), trainer::RUN_ID);
    }

    #[test]
    fn an_explicit_run_id_is_taken() {
        let args: Vec<String> = ["dorobot-nexus", "--probe", "other-run", "--json"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(run_arg(&args, 1), "other-run");
    }

    #[test]
    fn a_trailing_flag_falls_back_to_the_default_run() {
        let args: Vec<String> = ["dorobot-nexus", "--crosssim"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(run_arg(&args, 1), trainer::RUN_ID);
    }
}
