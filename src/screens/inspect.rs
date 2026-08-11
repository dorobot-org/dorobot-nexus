//! Inspect — reach into the running simulation.
//!
//! The differentiator. Because the simulator runs in this process, a push is a
//! function call and the response is on screen in the same frame; every other
//! tool in this space needs a script and a re-run to ask the same question.

use makepad_widgets::*;

use crate::state::Robot;
use crate::ux;

script_mod! {
    use mod.prelude.widgets.*
    use mod.widgets.*
    use mod.widgets.ux.*

    let Meter = View{
        width: Fill height: 18
        flow: Right
        align: Align{y: 0.5}
        spacing: 8.0
        k := Label{
            width: 86 text: "-"
            draw_text +: {
                text_style: mod.widgets.ux.TEXT_CHIP{}
                get_color: fn() { return #x98A1B8 }
            }
        }
        bar := View{
            width: Fill height: 7
            show_bg: true
            draw_bg +: {
                v: instance(0.4)
                pixel: fn() {
                    let sdf = Sdf2d.viewport(self.pos * self.rect_size)
                    let w = self.rect_size.x
                    let h = self.rect_size.y
                    sdf.box(0.0, 0.0, w, h, 2.0)
                    sdf.fill(#x222836)
                    // Saturated effort turns amber: the explanation for gaits
                    // that look inexplicable from outside.
                    let c = mix(#x5CBE8B, #xD6A254, step(0.82, self.v))
                    sdf.box(0.0, 0.0, w * clamp(self.v, 0.0, 1.0), h, 2.0)
                    sdf.fill(c)
                    return sdf.result
                }
            }
        }
    }

    let ObsRow = View{
        width: Fill height: 19
        flow: Right
        align: Align{y: 0.5}
        k := Label{
            width: Fill text: "-"
            draw_text +: {
                text_style: mod.widgets.ux.TEXT_CHIP{}
                get_color: fn() { return #x98A1B8 }
            }
        }
        v := Label{
            text: "-"
            draw_text +: {
                text_style: mod.widgets.ux.TEXT_CHIP{}
                get_color: fn() { return #xE7EAF3 }
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

    mod.widgets.InspectScreenBase = #(InspectScreen::register_widget(vm))
    mod.widgets.InspectScreen = set_type_default() do mod.widgets.InspectScreenBase{
        width: Fill height: Fill
        flow: Right
        spacing: 12.0
        padding: Inset{left: 14. right: 14. top: 12. bottom: 12.}
        show_bg: true
        draw_bg +: { color: #x0E1119 }

        main := View{
            width: Fill height: Fill
            flow: Down
            spacing: 12.0

            stage := mod.widgets.ux.Card{
                width: Fill height: Fill
                flow: Down
                stage_head := mod.widgets.ux.PanelHead{ title +: {text: "ROLLOUT"} }
                viewport := RobotView{ width: Fill height: Fill }
            }

            transport := mod.widgets.ux.Card{
                width: Fill height: 84
                flow: Down
                t_body := View{
                    width: Fill height: Fill flow: Down spacing: 8.0
                    padding: Inset{left: 12. right: 12. top: 10. bottom: 10.}
                    controls := View{
                        width: Fill height: Fit flow: Right spacing: 7.0 align: Align{y: 0.5}
                        b_start := TBtn{ cap +: {text: "|<"} }
                        b_step_b := TBtn{ cap +: {text: "<"} }
                        b_play := TBtn{ cap +: {text: ">"} draw_bg +: {color: #x3B2F6B border_color: #xA28BEA border_size: 1.0 border_radius: 5.0} }
                        b_step_f := TBtn{ cap +: {text: ">"} }
                        b_push := TBtn{ width: 74 cap +: {text: "push"} }
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
                        width: Fill height: 22
                        show_bg: true
                        draw_bg +: {
                            head: instance(0.35)
                            pixel: fn() {
                                let sdf = Sdf2d.viewport(self.pos * self.rect_size)
                                let w = self.rect_size.x
                                let h = self.rect_size.y
                                sdf.box(0.0, h * 0.38, w, h * 0.24, 2.0)
                                sdf.fill(#x1B2130)
                                sdf.box(0.0, h * 0.38, w * self.head, h * 0.24, 2.0)
                                sdf.fill(#x4B4076)
                                sdf.box(w * self.head - 1.0, 0.0, 2.0, h, 1.0)
                                sdf.fill(#xBCA9F5)
                                return sdf.result
                            }
                        }
                    }
                }
            }
        }

        side := View{
            width: 264 height: Fill
            flow: Down
            spacing: 12.0

            obs := mod.widgets.ux.Card{
                width: Fill height: Fill
                flow: Down
                obs_head := mod.widgets.ux.PanelHead{ title +: {text: "OBSERVATION"} }
                obs_body := View{
                    width: Fill height: Fill flow: Down
                    padding: Inset{left: 12. right: 12. top: 8. bottom: 10.}
                    ob0 := ObsRow{} ob1 := ObsRow{} ob2 := ObsRow{} ob3 := ObsRow{}
                    ob4 := ObsRow{} ob5 := ObsRow{} ob6 := ObsRow{} ob7 := ObsRow{}
                    mod.widgets.ux.Filler{}
                }
            }

            torque := mod.widgets.ux.Card{
                width: Fill height: Fit
                flow: Down
                tq_head := mod.widgets.ux.PanelHead{ title +: {text: "TORQUE"} }
                tq_body := View{
                    width: Fill height: Fit flow: Down spacing: 6.0
                    padding: Inset{left: 12. right: 12. top: 10. bottom: 13.}
                    m0 := Meter{ k +: {text: "hip_pitch"}   bar +: {draw_bg +: {v: 0.42}} }
                    m1 := Meter{ k +: {text: "knee"}        bar +: {draw_bg +: {v: 0.91}} }
                    m2 := Meter{ k +: {text: "ankle_pitch"} bar +: {draw_bg +: {v: 0.63}} }
                    m3 := Meter{ k +: {text: "waist_yaw"}   bar +: {draw_bg +: {v: 0.28}} }
                    m4 := Meter{ k +: {text: "shoulder"}    bar +: {draw_bg +: {v: 0.35}} }
                }
            }
        }
    }
}

#[derive(Script, ScriptHook, Widget)]
pub struct InspectScreen {
    #[deref]
    view: View,
    #[rust]
    loaded: Option<std::path::PathBuf>,
}

impl Widget for InspectScreen {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        self.view.handle_event(cx, event, scope);
    }
    fn draw_walk(&mut self, cx: &mut Cx2d, scope: &mut Scope, walk: Walk) -> DrawStep {
        self.view.draw_walk(cx, scope, walk)
    }
}

/// The observation contract for the G1 whole-body task, as the policy sees it.
const OBS: [(&str, &str); 8] = [
    ("base_lin_vel.x", "+0.74"),
    ("base_lin_vel.y", "−0.03"),
    ("base_ang_vel.z", "+0.11"),
    ("proj_gravity.z", "−0.98"),
    ("cmd.vel_x", "+0.80"),
    ("joint_pos[0]", "+0.12"),
    ("joint_pos[1]", "−0.44"),
    ("joint_vel[0]", "+1.62"),
];

const OBS_ROWS: [&[LiveId]; 8] = [
    ids!(side.obs.obs_body.ob0), ids!(side.obs.obs_body.ob1),
    ids!(side.obs.obs_body.ob2), ids!(side.obs.obs_body.ob3),
    ids!(side.obs.obs_body.ob4), ids!(side.obs.obs_body.ob5),
    ids!(side.obs.obs_body.ob6), ids!(side.obs.obs_body.ob7),
];

impl InspectScreenRef {
    pub fn sync(&self, cx: &mut Cx, robot: &Robot, checkpoint: &str) {
        let Some(mut inner) = self.borrow_mut() else { return };
        let need_load = inner.loaded.as_deref() != Some(robot.urdf.as_path());
        if need_load {
            inner.loaded = Some(robot.urdf.clone());
        }
        let root = &mut inner.view;

        let viewer = root.widget(cx, ids!(main.stage.viewport));
        if need_load {
            if let Some(mut rv) = viewer.borrow_mut::<makepad_urdf_player::robot_view::RobotView>() {
                if let Err(e) = rv.load_robot(
                    &robot.urdf.to_string_lossy(),
                    &robot.assets.to_string_lossy(),
                ) {
                    ::log::error!("inspect: {} failed to load: {e}", robot.urdf.display());
                }
            }
        }

        ux::head(cx, root, ids!(main.stage.stage_head), "ROLLOUT", checkpoint);
        root.label(cx, ids!(main.transport.t_body.controls.frame_lbl))
            .set_text(cx, "— / —");
        for (i, path) in OBS_ROWS.iter().enumerate() {
            let row = root.widget(cx, path);
            row.label(cx, ids!(k)).set_text(cx, OBS[i].0);
            row.label(cx, ids!(v)).set_text(cx, OBS[i].1);
        }
        ux::head(cx, root, ids!(side.obs.obs_head), "OBSERVATION", "99 ch");
    }
}
