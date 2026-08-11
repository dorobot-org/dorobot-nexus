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

mod plot;
mod screens;
mod state;
mod ux;

use screens::{
    inspect::InspectScreenWidgetRefExt, runs::RunsScreenWidgetRefExt,
    scene::SceneScreenWidgetRefExt, task::TaskScreenWidgetRefExt,
    train::TrainScreenWidgetRefExt, validate::ValidateScreenWidgetRefExt,
};
use state::Studio;
use ux::Screen;

app_main!(App);

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
        self.ui.inspect_screen(cx, ids!(page_inspect)).sync(cx, &robot, &ckpt);
        self.ui.validate_screen(cx, ids!(page_validate)).sync(cx);

        self.ui
            .label(cx, ids!(app_bar.device))
            .set_text(cx, &format!("{} · no trainer attached", device_line()));

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
        self.sync(cx);
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
