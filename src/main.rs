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
mod plot;
mod probe;
mod screens;
mod rl;
mod rng;
mod state;
mod sweep;
mod trainer;
mod ux;
#[cfg(feature = "zealot")]
mod zealot;

use screens::{
    inspect::InspectScreenWidgetRefExt, runs::RunsScreenWidgetRefExt,
    scene::SceneScreenWidgetRefExt, task::TaskScreenWidgetRefExt,
    train::TrainScreenWidgetRefExt, validate::ValidateScreenWidgetRefExt,
};
use state::Studio;
use ux::Screen;

/// `--headless N` trains without a window and prints progress. The fastest way
/// to answer "is it learning" without judging it through a UI.
fn headless(total: u64) -> ! {
    let envs = 256;
    // --no-random trains at nominal physics, which is the control case for
    // showing that the sweep measures anything at all.
    let randomize = !std::env::args().any(|a| a == "--no-random");

    // With --features zealot and a built stack, the same loop reports zealot's
    // GPU run. `total` stays a budget in env-steps for both backends: zealot
    // counts iterations, and at 256 envs it emits 24 steps per env per
    // iteration, so the budget converts rather than changing meaning.
    #[cfg(feature = "zealot")]
    let h = match zealot::spawn(
        envs,
        (total / (envs as u64 * 24)).max(1),
        "dorobot_nexus.safetensors",
    ) {
        Some(h) => {
            println!("backend: zealot ({})", zealot::binary_path().display());
            h
        }
        None => {
            println!(
                "backend: CPU (no zealot binary at {}; run scripts/setup-zealot.sh)",
                zealot::binary_path().display()
            );
            trainer::spawn_with(envs, total, 1, randomize)
        }
    };
    #[cfg(not(feature = "zealot"))]
    let h = trainer::spawn_with(envs, total, 1, randomize);
    let mut shown = 0usize;
    loop {
        std::thread::sleep(std::time::Duration::from_millis(250));
        let (n, done, line) = {
            let g = h.shared.lock().unwrap();
            let line = g.samples.last().map(|s| {
                format!(
                    "{:>9} steps  reward {:>6.3}  falls {:>5.1}%  ep_len {:>5.0}  {:>6.0}k steps/s",
                    s.step, s.reward, s.fall_rate * 100.0, s.episode_len,
                    s.steps_per_sec / 1000.0
                )
            });
            (g.samples.len(), !g.running, line)
        };
        if n > shown {
            if let Some(l) = line {
                println!("{l}");
            }
            shown = n;
        }
        if done {
            break;
        }
    }
    std::process::exit(0);
}

app_main!(App);

/// `--sweep` runs the robustness sweep on the newest checkpoint and prints it.
/// A surface you can only see in a GUI is a surface you cannot diff between
/// two policies, which is the comparison that makes it worth computing.
fn headless_sweep() -> ! {
    let Some(surface) = sweep::spawn(trainer::RUN_ID) else {
        eprintln!("no checkpoint to sweep");
        std::process::exit(1);
    };
    loop {
        std::thread::sleep(std::time::Duration::from_millis(150));
        let g = surface.lock().unwrap();
        if !g.running {
            println!("mass\\force   {}", (0..sweep::COLS)
                .map(|c| format!("{:>5.2}", sweep::axis_force(c)))
                .collect::<Vec<_>>().join(" "));
            for r in 0..sweep::ROWS {
                let cells: Vec<String> = (0..sweep::COLS)
                    .map(|c| match g.cell(r, c) {
                        Some(v) => format!("{:>5.2}", v),
                        None => "    —".into(),
                    })
                    .collect();
                println!("{:>9.2}   {}", sweep::axis_mass(r), cells.join(" "));
            }
            println!("\n{:.0}% of cells pass", g.pass_fraction() * 100.0);
            break;
        }
    }
    std::process::exit(0);
}

/// `--track-check` asks the one question the flat sweep raised: does the policy
/// respond to its velocity command at all?
fn track_check() -> ! {
    use crate::env::{VecEnv, N_ACT, N_OBS};
    use crate::rl::{Config, Ppo};
    let Some((path, m)) = ckpt::list(trainer::RUN_ID).into_iter().next() else {
        eprintln!("no checkpoint");
        std::process::exit(1);
    };
    let (w, _) = ckpt::read(&path).unwrap();
    let mut rng = rng::Rng::new(1);
    let hidden = if m.hidden == 0 { 64 } else { m.hidden };
    let mut ppo = Ppo::new(N_OBS, N_ACT, hidden, Config::default(), &mut rng);
    assert!(ppo.load_weights(&w));

    println!("  cmd    mean dx   tracked");
    for k in -3..=3 {
        let cmd = k as f32 * 0.25;
        let mut env = VecEnv::new(1, 5);
        env.restart(0, cmd);
        let (mut obs, mut act) = (vec![0.0; N_OBS], vec![0.0; N_ACT]);
        let (mut sum_dx, mut n) = (0.0_f32, 0usize);
        for _ in 0..400 {
            env.observe(0, &mut obs);
            ppo.act_mean(&obs, &mut act);
            let o = env.step(&[act.clone()]);
            // obs[1] is cart velocity.
            env.observe(0, &mut obs);
            sum_dx += obs[1];
            n += 1;
            if o.done[0] { break; }
        }
        let dx = sum_dx / n.max(1) as f32;
        println!("{cmd:>6.2} {dx:>10.3} {:>9.2}", (-3.0 * (dx - cmd).abs()).exp());
    }
    std::process::exit(0);
}

fn maybe_headless() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--track-check") {
        track_check();
    }
    if args.iter().any(|a| a == "--sweep") {
        headless_sweep();
    }
    if let Some(i) = args.iter().position(|a| a == "--headless") {
        let n: u64 = args.get(i + 1).and_then(|v| v.parse().ok()).unwrap_or(2_000_000);
        headless(n);
    }
}

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
    /// A checkpoint being driven in Inspect.
    #[rust]
    probe: Option<probe::Probe>,
    #[rust]
    surface: Option<std::sync::Arc<std::sync::Mutex<sweep::Surface>>>,
    #[rust]
    cross: Option<std::sync::Arc<std::sync::Mutex<crosssim::Report>>>,
}

impl App {
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
            if self.trainer.is_none() {
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
        self.poll = cx.start_interval(0.2);
        self.sync(cx);
    }

    fn handle_timer(&mut self, cx: &mut Cx, e: &TimerEvent) {
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
            match sweep::spawn(trainer::RUN_ID) {
                Some(s) => self.surface = Some(s),
                None => ::log::info!("sweep: no checkpoint to sweep yet"),
            }
            dirty = true;
        }

        if self.ui.validate_screen(cx, ids!(page_validate)).clicked_cross(cx, actions) {
            match crosssim::spawn(trainer::RUN_ID) {
                Some(c) => self.cross = Some(c),
                None => ::log::info!("cross-sim: no checkpoint yet"),
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
        maybe_headless();
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
