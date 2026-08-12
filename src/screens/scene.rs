//! Scene — the world and the body, seen before GPU-hours are spent in it.

use makepad_widgets::*;

use crate::state::Robot;
use crate::ux;

script_mod! {
    use mod.prelude.widgets.*
    use mod.widgets.*
    use mod.widgets.ux.*

    let Row = View{
        width: Fill height: 26
        flow: Right
        align: Align{y: 0.5}
        k := Label{
            width: Fill text: "-"
            draw_text +: {
                text_style: mod.widgets.ux.TEXT_META{}
                get_color: fn() { return #x98A1B8 }
            }
        }
        v := Label{
            text: "-"
            draw_text +: {
                text_style: mod.widgets.ux.TEXT_META{}
                get_color: fn() { return #xE7EAF3 }
            }
        }
    }

    // A randomization range. `lo`/`hi` are normalised 0..1 across the track.
    let Range = View{
        width: Fill height: 40
        flow: Down
        spacing: 5.0
        cap := View{
            width: Fill height: Fit flow: Right
            k := Label{
                width: Fill text: "-"
                draw_text +: {
                    text_style: mod.widgets.ux.TEXT_META{}
                    get_color: fn() { return #x98A1B8 }
                }
            }
            v := Label{
                text: "-"
                draw_text +: {
                    text_style: mod.widgets.ux.TEXT_META{}
                    get_color: fn() { return #xE7EAF3 }
                }
            }
        }
        track := View{
            width: Fill height: 8
            show_bg: true
            draw_bg +: {
                lo: instance(0.2)
                hi: instance(0.8)
                pixel: fn() {
                    let sdf = Sdf2d.viewport(self.pos * self.rect_size)
                    let w = self.rect_size.x
                    let h = self.rect_size.y
                    sdf.box(0.0, h * 0.30, w, h * 0.40, 2.0)
                    sdf.fill(#x222836)
                    sdf.box(w * self.lo, h * 0.30, w * (self.hi - self.lo), h * 0.40, 2.0)
                    sdf.fill(#xA28BEA)
                    sdf.circle(w * self.lo, h * 0.5, 4.0)
                    sdf.fill(#xBCA9F5)
                    sdf.circle(w * self.hi, h * 0.5, 4.0)
                    sdf.fill(#xBCA9F5)
                    return sdf.result
                }
            }
        }
    }

    // One selectable terrain family. `on` tints the active one.
    let TerrainRow = View{
        width: Fill height: 28
        flow: Right
        align: Align{y: 0.5}
        cursor: MouseCursor.Hand
        show_bg: true
        draw_bg +: {
            on: instance(0.0)
            pixel: fn() {
                let sdf = Sdf2d.viewport(self.pos * self.rect_size)
                sdf.box(0.0, 2.0, self.rect_size.x, self.rect_size.y - 4.0, 4.0)
                sdf.fill(mix(#x00000000, #x2A2140, self.on))
                return sdf.result
            }
        }
        k := Label{
            width: Fill text: "-"
            margin: Inset{left: 8.}
            draw_text +: {
                text_style: mod.widgets.ux.TEXT_BODY{}
                get_color: fn() { return #xE7EAF3 }
            }
        }
        v := Label{
            text: ""
            margin: Inset{right: 8.}
            draw_text +: {
                text_style: mod.widgets.ux.TEXT_META{}
                get_color: fn() { return #x98A1B8 }
            }
        }
    }

    let TBtn = RoundedView{
        width: 34 height: 26
        align: Align{x: 0.5, y: 0.5}
        cursor: MouseCursor.Hand
        draw_bg +: {
            color: #x1B2130
            border_color: #x2C3446
            border_size: 1.0
            border_radius: 5.0
        }
        cap := Label{
            text: "-"
            draw_text +: {
                text_style: mod.widgets.ux.TEXT_CHIP{}
                get_color: fn() { return #xC7CEDD }
            }
        }
    }

    // A single-value control. `t` is the normalised position of the fill.
    let Knob = View{
        width: Fill height: 42
        flow: Down
        spacing: 5.0
        cap := View{
            width: Fill height: Fit flow: Right
            k := Label{
                width: Fill text: "-"
                draw_text +: {
                    text_style: mod.widgets.ux.TEXT_META{}
                    get_color: fn() { return #x98A1B8 }
                }
            }
            v := Label{
                text: "-"
                draw_text +: {
                    text_style: mod.widgets.ux.TEXT_META{}
                    get_color: fn() { return #xE7EAF3 }
                }
            }
        }
        track := View{
            width: Fill height: 10
            cursor: MouseCursor.Hand
            show_bg: true
            draw_bg +: {
                t: instance(0.5)
                pixel: fn() {
                    let sdf = Sdf2d.viewport(self.pos * self.rect_size)
                    let w = self.rect_size.x
                    let h = self.rect_size.y
                    sdf.box(0.0, h * 0.35, w, h * 0.30, 2.0)
                    sdf.fill(#x222836)
                    sdf.box(0.0, h * 0.35, w * self.t, h * 0.30, 2.0)
                    sdf.fill(#xA28BEA)
                    sdf.circle(w * self.t, h * 0.5, 4.5)
                    sdf.fill(#xBCA9F5)
                    return sdf.result
                }
            }
        }
    }

    mod.widgets.SceneScreenBase = #(SceneScreen::register_widget(vm))
    mod.widgets.SceneScreen = set_type_default() do mod.widgets.SceneScreenBase{
        width: Fill height: Fill
        flow: Right
        spacing: 12.0
        padding: Inset{left: 14. right: 14. top: 12. bottom: 12.}
        show_bg: true
        draw_bg +: { color: #x0E1119 }

        stage := mod.widgets.ux.Card{
            width: Fill height: Fill
            flow: Down
            stage_head := mod.widgets.ux.PanelHead{ title +: {text: "SCENE"} }
            // The real renderer. It brings the studio gradient with it, and it
            // is the same widget DoRobot Studio uses for its calibration mirror.
            viewport := RobotView{ width: Fill height: Fill }

            // Playback for the recorded rollout. A scene you cannot stop is a
            // scene you cannot look at.
            transport := View{
                width: Fill height: 62 flow: Down spacing: 7.0
                padding: Inset{left: 12. right: 12. top: 8. bottom: 10.}
                controls := View{
                    width: Fill height: Fit flow: Right spacing: 7.0 align: Align{y: 0.5}
                    b_start  := TBtn{ cap +: {text: "|<"} }
                    b_back   := TBtn{ cap +: {text: "<"} }
                    b_play   := TBtn{ cap +: {text: "||"} draw_bg +: {color: #x3B2F6B border_color: #xA28BEA border_size: 1.0 border_radius: 5.0} }
                    b_fwd    := TBtn{ cap +: {text: ">"} }
                    b_record := TBtn{ width: 64 cap +: {text: "record"} }
                    mod.widgets.ux.Filler{}
                    frame_lbl := Label{
                        text: "0 / 0"
                        draw_text +: {
                            text_style: mod.widgets.ux.TEXT_META{}
                            get_color: fn() { return #xE7EAF3 }
                        }
                    }
                }
                scrub := View{
                    width: Fill height: 10
                    cursor: MouseCursor.Hand
                    show_bg: true
                    draw_bg +: {
                        t: instance(0.0)
                        pixel: fn() {
                            let sdf = Sdf2d.viewport(self.pos * self.rect_size)
                            let w = self.rect_size.x
                            let h = self.rect_size.y
                            sdf.box(0.0, h * 0.35, w, h * 0.30, 2.0)
                            sdf.fill(#x222836)
                            sdf.box(0.0, h * 0.35, w * self.t, h * 0.30, 2.0)
                            sdf.fill(#xA28BEA)
                            sdf.circle(w * self.t, h * 0.5, 4.0)
                            sdf.fill(#xBCA9F5)
                            return sdf.result
                        }
                    }
                }
            }
        }

        side := View{
            width: 320 height: Fill
            flow: Down
            spacing: 12.0

            robot := mod.widgets.ux.Card{
                width: Fill height: Fit
                flow: Down
                robot_head := mod.widgets.ux.PanelHead{ title +: {text: "ROBOT"} }
                robot_body := View{
                    width: Fill height: Fit flow: Down
                    padding: Inset{left: 12. right: 12. top: 8. bottom: 12.}
                    r_name := Row{ k +: {text: "model"} }
                    r_dof  := Row{ k +: {text: "actuated"} }
                    r_urdf := Row{ k +: {text: "source"} }
                    add := View{
                        width: Fit height: Fit
                        margin: Inset{top: 10.}
                        cursor: MouseCursor.Hand
                        add_lbl := Label{
                            text: "+  Add robot"
                            draw_text +: {
                                text_style: mod.widgets.ux.TEXT_BODY{}
                                get_color: fn() { return #xA28BEA }
                            }
                        }
                    }
                }
            }

            terrain := mod.widgets.ux.Card{
                width: Fill height: Fit
                flow: Down
                terrain_head := mod.widgets.ux.PanelHead{ title +: {text: "TERRAIN"} }
                terrain_body := View{
                    width: Fill height: Fit flow: Down spacing: 2.0
                    padding: Inset{left: 6. right: 6. top: 8. bottom: 12.}
                    t_flat  := TerrainRow{ k +: {text: "flat"}  v +: {text: "baseline"} }
                    t_boxes := TerrainRow{ k +: {text: "boxes"} v +: {text: "discrete"} }
                    t_rough := TerrainRow{ k +: {text: "rough"} v +: {text: "noise"} }
                    t_wave  := TerrainRow{ k +: {text: "wave"}  v +: {text: "rolling"} }
                    t_step  := TerrainRow{ k +: {text: "step"}  v +: {text: "one edge"} }
                }
            }

            rand := mod.widgets.ux.Card{
                width: Fill height: Fit
                flow: Down
                rand_head := mod.widgets.ux.PanelHead{ title +: {text: "RANDOMIZATION"} }
                rand_body := View{
                    width: Fill height: Fit flow: Down spacing: 6.0
                    padding: Inset{left: 12. right: 12. top: 10. bottom: 14.}
                    g_fric  := Knob{ cap +: {k +: {text: "friction"}}   }
                    g_kp    := Knob{ cap +: {k +: {text: "PD gain"}}    }
                    g_mass  := Knob{ cap +: {k +: {text: "mass"}}       }
                    g_push  := Knob{ cap +: {k +: {text: "push"}}       }
                    g_secs  := Knob{ cap +: {k +: {text: "length"}}     }
                    g_amp   := Knob{ cap +: {k +: {text: "relief"}}     }
                    g_slope := Knob{ cap +: {k +: {text: "grade"}}      }
                    g_stance := Knob{ cap +: {k +: {text: "stance"}}    }
                }
            }

            lib := mod.widgets.ux.Card{
                width: Fill height: Fill
                flow: Down
                lib_head := mod.widgets.ux.PanelHead{ title +: {text: "LIBRARY"} }
                lib_body := View{
                    width: Fill height: Fill flow: Down spacing: 2.0
                    padding: Inset{left: 6. right: 6. top: 8. bottom: 10.}
                    save_row := TerrainRow{ k +: {text: "+  save scene"} v +: {text: ""} }
                    s0 := TerrainRow{ k +: {text: ""} v +: {text: ""} }
                    s1 := TerrainRow{ k +: {text: ""} v +: {text: ""} }
                    s2 := TerrainRow{ k +: {text: ""} v +: {text: ""} }
                    s3 := TerrainRow{ k +: {text: ""} v +: {text: ""} }
                    rec_lbl := Label{
                        text: "RECORDINGS"
                        margin: Inset{left: 8., top: 8., bottom: 2.}
                        draw_text +: {
                            text_style: mod.widgets.ux.TEXT_META{}
                            get_color: fn() { return #x6E7691 }
                        }
                    }
                    r0 := TerrainRow{ k +: {text: ""} v +: {text: ""} }
                    r1 := TerrainRow{ k +: {text: ""} v +: {text: ""} }
                    r2 := TerrainRow{ k +: {text: ""} v +: {text: ""} }
                    mod.widgets.ux.Filler{}
                }
            }
        }
    }
}

/// A click in the scene/recording library.
#[derive(Clone, Copy, PartialEq)]
pub enum Library {
    Save,
    LoadScene(usize),
    Replay(usize),
}

/// An adjustable field of the scene.
#[derive(Clone, Copy, PartialEq)]
pub enum Knob {
    Friction,
    KpScale,
    MassDr,
    PushVel,
    Seconds,
    TerrainAmp,
    TerrainSlope,
    BaseHeight,
}

impl Knob {
    pub fn get(self, s: &crate::scene::Scene) -> f32 {
        match self {
            Knob::Friction => s.friction,
            Knob::KpScale => s.kp_scale,
            Knob::MassDr => s.mass_dr,
            Knob::PushVel => s.push_vel,
            Knob::Seconds => s.seconds,
            Knob::TerrainAmp => s.terrain_amp,
            Knob::TerrainSlope => s.terrain_slope_deg,
            Knob::BaseHeight => s.base_height,
        }
    }

    pub fn set(self, s: &mut crate::scene::Scene, v: f32) {
        match self {
            Knob::Friction => s.friction = v,
            Knob::KpScale => s.kp_scale = v,
            Knob::MassDr => s.mass_dr = v,
            Knob::PushVel => s.push_vel = v,
            Knob::Seconds => s.seconds = v,
            Knob::TerrainAmp => s.terrain_amp = v,
            Knob::TerrainSlope => s.terrain_slope_deg = v,
            Knob::BaseHeight => s.base_height = v,
        }
    }

    pub fn format(self, v: f32) -> String {
        match self {
            Knob::Seconds => format!("{v:.0} s"),
            Knob::TerrainSlope => format!("{v:.0}°"),
            Knob::BaseHeight => format!("{v:.2} m"),
            Knob::TerrainAmp => format!("{v:.2}×"),
            Knob::PushVel if v <= 0.0 => "off".into(),
            _ => format!("{v:.2}×"),
        }
    }
}

/// What the scene transport reported this frame.
#[derive(Clone, Copy, PartialEq)]
pub enum Play {
    Toggle,
    Seek(f64),
    StepBack,
    StepForward,
    Restart,
    Record,
}

#[derive(Script, ScriptHook, Widget)]
pub struct SceneScreen {
    #[deref]
    view: View,
    /// Model currently mounted, so the URDF is opened once rather than per sync.
    #[rust]
    loaded: Option<std::path::PathBuf>,
}

impl Widget for SceneScreen {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        self.view.handle_event(cx, event, scope);
    }
    fn draw_walk(&mut self, cx: &mut Cx2d, scope: &mut Scope, walk: Walk) -> DrawStep {
        self.view.draw_walk(cx, scope, walk)
    }
}

impl SceneScreenRef {
    pub fn sync(&self, cx: &mut Cx, robot: &Robot) {
        let Some(mut inner) = self.borrow_mut() else { return };
        let need_load = inner.loaded.as_deref() != Some(robot.urdf.as_path());
        if need_load {
            inner.loaded = Some(robot.urdf.clone());
        }
        let root = &mut inner.view;

        let viewer = root.widget(cx, ids!(stage.viewport));
        if need_load {
            if let Some(mut rv) = viewer.borrow_mut::<makepad_urdf_player::robot_view::RobotView>() {
                if let Err(e) = rv.load_robot(
                    &robot.urdf.to_string_lossy(),
                    &robot.assets.to_string_lossy(),
                ) {
                    ::log::error!("scene: {} failed to load: {e}", robot.urdf.display());
                }
            }
        }

        ux::head(cx, root, ids!(stage.stage_head), "SCENE", "slope 12° · rough");
        for (path, value) in [
            (ids!(side.robot.robot_body.r_name) as &[LiveId], robot.name.clone()),
            (ids!(side.robot.robot_body.r_dof), format!("{} dof", robot.dof)),
            (ids!(side.robot.robot_body.r_urdf), "urdf".to_string()),
        ] {
            root.widget(cx, path).label(cx, ids!(v)).set_text(cx, &value);
        }
        ux::head(cx, root, ids!(side.obs.obs_head), "CONTRACT", "99 → 29");
    }

    /// The viewer's movable-joint names, in the order `set_pose` expects.
    ///
    /// Asked of the viewer rather than derived from the URDF: the viewer's
    /// ordering is its own movable subset, which a second parser can get
    /// subtly wrong — the G1 file comments out `floating_base_joint`, and
    /// counting it shifts every index by one.
    pub fn movable_joint_names(&self, cx: &mut Cx) -> Vec<String> {
        let Some(mut inner) = self.borrow_mut() else { return Vec::new() };
        let root = &mut inner.view;
        let viewer = root.widget(cx, ids!(stage.viewport));
        let guard = viewer.borrow::<makepad_urdf_player::robot_view::RobotView>();
        guard.map(|rv| rv.movable_joint_names()).unwrap_or_default()
    }

    /// Pose the robot from a rollout frame, body and base together.
    ///
    /// `angles` is indexed by the viewer's movable-joint order; `base` is the
    /// floating base as `[x, y, z, qx, qy, qz, qw]`. Without the base the robot
    /// animates its legs while standing still, however far it actually walked.
    pub fn set_pose(&self, cx: &mut Cx, angles: &[f32], base: Option<[f32; 7]>) {
        let Some(mut inner) = self.borrow_mut() else { return };
        // Nothing to pose until the URDF has finished loading.
        if inner.loaded.is_none() || angles.is_empty() {
            return;
        }
        let root = &mut inner.view;
        let viewer = root.widget(cx, ids!(stage.viewport));
        // The guard is bound to its own local so it is dropped before `viewer`.
        // As the tail expression of this block it would outlive the value it
        // borrows, which the borrow checker rejects.
        let mut guard = viewer.borrow_mut::<makepad_urdf_player::robot_view::RobotView>();
        if let Some(rv) = guard.as_mut() {
            if let Some(b) = base {
                rv.set_base_pose(cx, [b[0], b[1], b[2]], [b[3], b[4], b[5], b[6]]);
                // Follow it: the camera is framed once at load, so a robot that
                // walks 5 m would simply leave the shot. Pivot only — the
                // user's orbit and zoom are theirs.
                rv.set_camera_target(cx, [b[0], b[1], b[2]]);
            }
            rv.set_joint_angles(cx, angles);
        }
    }

    /// Draw a terrain mesh under the robot. `None` clears it.
    ///
    /// Returns the triangle count actually loaded, so the caller can report
    /// what is on screen rather than assuming the file was good.
    pub fn set_terrain(&self, cx: &mut Cx, stl: Option<&std::path::Path>) -> usize {
        let Some(mut inner) = self.borrow_mut() else { return 0 };
        let root = &mut inner.view;
        let viewer = root.widget(cx, ids!(stage.viewport));
        let mut guard = viewer.borrow_mut::<makepad_urdf_player::robot_view::RobotView>();
        let Some(rv) = guard.as_mut() else { return 0 };
        match stl {
            Some(path) => match rv.load_terrain(cx, &path.to_string_lossy()) {
                Ok(n) => n,
                Err(e) => {
                    ::log::error!("scene: terrain {} failed to load: {e}", path.display());
                    0
                }
            },
            None => {
                rv.clear_terrain(cx);
                0
            }
        }
    }

    /// The five adjustable scene knobs: id, label path, and the range each
    /// spans. Ranges are wider than the training distribution on purpose — a
    /// control that cannot express a failing value cannot show you one.
    const KNOBS: [(Knob, &'static [LiveId], f32, f32); 8] = [
        (Knob::Friction, ids!(side.rand.rand_body.g_fric), 0.15, 1.5),
        (Knob::KpScale, ids!(side.rand.rand_body.g_kp), 0.4, 1.6),
        (Knob::MassDr, ids!(side.rand.rand_body.g_mass), 0.5, 2.0),
        (Knob::PushVel, ids!(side.rand.rand_body.g_push), 0.0, 1.5),
        (Knob::Seconds, ids!(side.rand.rand_body.g_secs), 2.0, 20.0),
        // Terrain shape: these turn a family into a set of variants.
        (Knob::TerrainAmp, ids!(side.rand.rand_body.g_amp), 0.25, 3.0),
        (Knob::TerrainSlope, ids!(side.rand.rand_body.g_slope), 0.0, 30.0),
        // Stance height: the knob that turns walking into crouching.
        (Knob::BaseHeight, ids!(side.rand.rand_body.g_stance), 0.45, 0.84),
    ];

    /// Which knob was dragged, and to what value in its own units.
    pub fn knob_changed(&self, cx: &mut Cx, actions: &Actions) -> Option<(Knob, f32)> {
        let Some(mut inner) = self.borrow_mut() else { return None };
        let root = &mut inner.view;
        for (knob, path, lo, hi) in Self::KNOBS {
            let mut track_path = path.to_vec();
            track_path.push(live_id!(track));
            let w = root.widget(cx, &track_path);
            if w.is_empty() {
                continue;
            }
            let rect = w.area().rect(cx);
            for a in actions.filter_widget_actions(w.widget_uid()) {
                if let ViewAction::FingerUp(fe) = a.cast::<ViewAction>() {
                    if fe.is_over && rect.size.x > 1.0 {
                        let t = ((fe.abs.x - rect.pos.x) / rect.size.x).clamp(0.0, 1.0) as f32;
                        return Some((knob, lo + t * (hi - lo)));
                    }
                }
            }
        }
        None
    }

    /// Render the knobs from the scene they describe.
    pub fn show_knobs(&self, cx: &mut Cx, sc: &crate::scene::Scene) {
        let Some(mut inner) = self.borrow_mut() else { return };
        let root = &mut inner.view;
        for (knob, path, lo, hi) in Self::KNOBS {
            let value = knob.get(sc);
            let t = (((value - lo) / (hi - lo)) as f64).clamp(0.0, 1.0);
            let mut track_path = path.to_vec();
            track_path.push(live_id!(track));
            let mut track = root.widget(cx, &track_path);
            if !track.is_empty() {
                script_apply_eval!(cx, track, { draw_bg +: { t: #(t) } });
            }
            let mut label_path = path.to_vec();
            label_path.push(live_id!(cap));
            label_path.push(live_id!(v));
            root.label(cx, &label_path).set_text(cx, &knob.format(value));
        }
    }

    const SCENE_ROWS: [&'static [LiveId]; 4] = [
        ids!(side.lib.lib_body.s0),
        ids!(side.lib.lib_body.s1),
        ids!(side.lib.lib_body.s2),
        ids!(side.lib.lib_body.s3),
    ];
    const REC_ROWS: [&'static [LiveId]; 3] = [
        ids!(side.lib.lib_body.r0),
        ids!(side.lib.lib_body.r1),
        ids!(side.lib.lib_body.r2),
    ];

    /// A click in the library: save the current scene, load a saved one, or
    /// replay a recording.
    pub fn library_action(&self, cx: &mut Cx, actions: &Actions) -> Option<Library> {
        let Some(mut inner) = self.borrow_mut() else { return None };
        let root = &mut inner.view;
        let save = root.widget(cx, ids!(side.lib.lib_body.save_row));
        if !save.is_empty() && ux::view_clicked(actions, save.widget_uid()) {
            return Some(Library::Save);
        }
        for (i, path) in Self::SCENE_ROWS.iter().enumerate() {
            let w = root.widget(cx, path);
            if !w.is_empty() && ux::view_clicked(actions, w.widget_uid()) {
                return Some(Library::LoadScene(i));
            }
        }
        for (i, path) in Self::REC_ROWS.iter().enumerate() {
            let w = root.widget(cx, path);
            if !w.is_empty() && ux::view_clicked(actions, w.widget_uid()) {
                return Some(Library::Replay(i));
            }
        }
        None
    }

    /// Fill the library rows. Empty slots are blanked rather than left showing
    /// a stale name that would load something else when clicked.
    pub fn show_library(
        &self,
        cx: &mut Cx,
        scenes: &[crate::scene::Scene],
        recs: &[crate::scene::Recording],
        active: &str,
    ) {
        let Some(mut inner) = self.borrow_mut() else { return };
        let root = &mut inner.view;
        for (i, path) in Self::SCENE_ROWS.iter().enumerate() {
            let (name, summary) = match scenes.get(i) {
                Some(sc) => (sc.name.clone(), sc.summary()),
                None => (String::new(), String::new()),
            };
            let mut k = path.to_vec();
            k.push(live_id!(k));
            root.label(cx, &k).set_text(cx, &name);
            let mut v = path.to_vec();
            v.push(live_id!(v));
            // The summary is long; the row is narrow. Show the terrain and
            // length, which is what distinguishes two scenes at a glance.
            let short = summary.split(" · ").take(2).collect::<Vec<_>>().join(" · ");
            root.label(cx, &v).set_text(cx, &short);
            let on: f64 = if !name.is_empty() && name == active { 1.0 } else { 0.0 };
            let mut row = root.widget(cx, path);
            if !row.is_empty() {
                script_apply_eval!(cx, row, { draw_bg +: { on: #(on) } });
            }
        }
        for (i, path) in Self::REC_ROWS.iter().enumerate() {
            let (name, summary) = match recs.get(i) {
                Some(r) => (r.name.clone(), format!("{} fr · {:.1} m", r.frames, r.distance)),
                None => (String::new(), String::new()),
            };
            let mut k = path.to_vec();
            k.push(live_id!(k));
            root.label(cx, &k).set_text(cx, &name);
            let mut v = path.to_vec();
            v.push(live_id!(v));
            root.label(cx, &v).set_text(cx, &summary);
        }
    }

    /// What the scene transport reported this frame.
    pub fn transport(&self, cx: &mut Cx, actions: &Actions) -> Option<Play> {
        const BTNS: [(&[LiveId], Play); 5] = [
            (ids!(stage.transport.controls.b_play), Play::Toggle),
            (ids!(stage.transport.controls.b_back), Play::StepBack),
            (ids!(stage.transport.controls.b_fwd), Play::StepForward),
            (ids!(stage.transport.controls.b_start), Play::Restart),
            (ids!(stage.transport.controls.b_record), Play::Record),
        ];
        let Some(mut inner) = self.borrow_mut() else { return None };
        let root = &mut inner.view;
        for (path, act) in BTNS {
            let w = root.widget(cx, path);
            if !w.is_empty() && ux::view_clicked(actions, w.widget_uid()) {
                return Some(act);
            }
        }
        // Clicking the scrubber seeks to that fraction of the rollout.
        let scrub = root.widget(cx, ids!(stage.transport.scrub));
        if !scrub.is_empty() {
            let rect = scrub.area().rect(cx);
            for a in actions.filter_widget_actions(scrub.widget_uid()) {
                if let ViewAction::FingerUp(fe) = a.cast::<ViewAction>() {
                    if fe.is_over && rect.size.x > 1.0 {
                        return Some(Play::Seek((fe.abs.x - rect.pos.x) / rect.size.x));
                    }
                }
            }
        }
        None
    }

    /// Show the playhead: `frame` of `total`, and whether it is running.
    pub fn show_playback(&self, cx: &mut Cx, frame: usize, total: usize, playing: bool) {
        let Some(mut inner) = self.borrow_mut() else { return };
        let root = &mut inner.view;
        let t = if total > 1 { frame as f64 / (total - 1) as f64 } else { 0.0 };
        let mut bar = root.widget(cx, ids!(stage.transport.scrub));
        if !bar.is_empty() {
            script_apply_eval!(cx, bar, { draw_bg +: { t: #(t) } });
        }
        root.label(cx, ids!(stage.transport.controls.frame_lbl))
            .set_text(cx, &format!("{frame} / {}", total.saturating_sub(1)));
        root.widget(cx, ids!(stage.transport.controls.b_play))
            .label(cx, ids!(cap))
            .set_text(cx, if playing { "||" } else { ">" });
    }

    /// Which terrain family was just clicked, if any, and highlight it.
    ///
    /// Returns the `BIPED_TERRAIN_FAMILY` value — `""` for flat ground.
    pub fn clicked_terrain(&self, cx: &mut Cx, actions: &Actions) -> Option<&'static str> {
        const ROWS: [(&[LiveId], &str); 5] = [
            (ids!(side.terrain.terrain_body.t_flat), ""),
            (ids!(side.terrain.terrain_body.t_boxes), "boxes"),
            (ids!(side.terrain.terrain_body.t_rough), "rough"),
            (ids!(side.terrain.terrain_body.t_wave), "wave"),
            (ids!(side.terrain.terrain_body.t_step), "step"),
        ];
        let Some(mut inner) = self.borrow_mut() else { return None };
        let root = &mut inner.view;
        let mut hit = None;
        for (path, family) in ROWS {
            let w = root.widget(cx, path);
            if !w.is_empty() && ux::view_clicked(actions, w.widget_uid()) {
                hit = Some(family);
            }
        }
        hit
    }

    /// Tint the row matching `family`, so the selection is visible.
    pub fn show_terrain(&self, cx: &mut Cx, family: &str) {
        const ROWS: [(&[LiveId], &str); 5] = [
            (ids!(side.terrain.terrain_body.t_flat), ""),
            (ids!(side.terrain.terrain_body.t_boxes), "boxes"),
            (ids!(side.terrain.terrain_body.t_rough), "rough"),
            (ids!(side.terrain.terrain_body.t_wave), "wave"),
            (ids!(side.terrain.terrain_body.t_step), "step"),
        ];
        let Some(mut inner) = self.borrow_mut() else { return };
        let root = &mut inner.view;
        for (path, f) in ROWS {
            let mut row = root.widget(cx, path);
            if row.is_empty() {
                continue;
            }
            let on: f64 = if f == family { 1.0 } else { 0.0 };
            script_apply_eval!(cx, row, { draw_bg +: { on: #(on) } });
        }
    }

    /// True when "Add robot" was clicked.
    pub fn clicked_add(&self, cx: &mut Cx, actions: &Actions) -> bool {
        let Some(mut inner) = self.borrow_mut() else { return false };
        let link = inner.view.widget(cx, ids!(side.robot.robot_body.add));
        !link.is_empty() && ux::view_clicked(actions, link.widget_uid())
    }
}
