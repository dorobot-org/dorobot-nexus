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

            rand := mod.widgets.ux.Card{
                width: Fill height: Fit
                flow: Down
                rand_head := mod.widgets.ux.PanelHead{ title +: {text: "RANDOMIZATION"} }
                rand_body := View{
                    width: Fill height: Fit flow: Down spacing: 8.0
                    padding: Inset{left: 12. right: 12. top: 10. bottom: 14.}
                    g_mass  := Range{ cap +: {k +: {text: "mass"}       v +: {text: "±15%"}} track +: {draw_bg +: {lo: 0.35 hi: 0.65}} }
                    g_fric  := Range{ cap +: {k +: {text: "friction"}   v +: {text: "0.4 – 1.2"}} track +: {draw_bg +: {lo: 0.20 hi: 0.75}} }
                    g_lat   := Range{ cap +: {k +: {text: "latency"}    v +: {text: "0 – 20 ms"}} track +: {draw_bg +: {lo: 0.00 hi: 0.40}} }
                    g_motor := Range{ cap +: {k +: {text: "motor gain"} v +: {text: "0.8 – 1.2"}} track +: {draw_bg +: {lo: 0.30 hi: 0.70}} }
                }
            }

            obs := mod.widgets.ux.Card{
                width: Fill height: Fill
                flow: Down
                obs_head := mod.widgets.ux.PanelHead{ title +: {text: "CONTRACT"} }
                obs_body := View{
                    width: Fill height: Fill flow: Down
                    padding: Inset{left: 12. right: 12. top: 8. bottom: 12.}
                    o0 := Row{ k +: {text: "base lin vel"}   v +: {text: "3"} }
                    o1 := Row{ k +: {text: "base ang vel"}   v +: {text: "3"} }
                    o2 := Row{ k +: {text: "projected grav"} v +: {text: "3"} }
                    o3 := Row{ k +: {text: "joint pos"}      v +: {text: "29"} }
                    o4 := Row{ k +: {text: "joint vel"}      v +: {text: "29"} }
                    o5 := Row{ k +: {text: "last action"}    v +: {text: "29"} }
                    o6 := Row{ k +: {text: "command"}        v +: {text: "3"} }
                    mod.widgets.ux.Filler{}
                }
            }
        }
    }
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

    /// Pose the robot from a rollout frame.
    ///
    /// `angles` is indexed by URDF joint order, which is what the viewer's
    /// `set_joint_angles` expects; `zealot::Rollout::pose` builds it by name.
    /// This is what turns the scene from a static model into the policy the
    /// trainer just produced.
    pub fn set_pose(&self, cx: &mut Cx, angles: &[f32]) {
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
            rv.set_joint_angles(cx, angles);
        }
    }

    /// True when "Add robot" was clicked.
    pub fn clicked_add(&self, cx: &mut Cx, actions: &Actions) -> bool {
        let Some(mut inner) = self.borrow_mut() else { return false };
        let link = inner.view.widget(cx, ids!(side.robot.robot_body.add));
        !link.is_empty() && ux::view_clicked(actions, link.widget_uid())
    }
}
