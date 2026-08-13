//! Inspect — reach into the running simulation.
//!
//! The differentiator. Because the simulator runs in this process, a push is a
//! function call and the response is on screen in the same frame; every other
//! tool in this space needs a script and a re-run to ask the same question.

use makepad_widgets::*;

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
                // The cart-pole, because that is what the policy drives. A G1
                // here would be decoration: nothing simulates it.
                viewport := View{
                    width: Fill height: Fill
                    show_bg: true
                    draw_bg +: {
                        cart_x: instance(0.0)
                        angle: instance(0.0)
                        push: instance(0.0)
                        pixel: fn() {
                            let sdf = Sdf2d.viewport(self.pos * self.rect_size)
                            let w = self.rect_size.x
                            let h = self.rect_size.y
                            // clear() paints the whole field; a zero-radius box
                            // was drawing nothing.
                            let sky = mix(#xFBF3F4, #xEFE6CE, self.pos.y)
                            sdf.clear(sky)

                            let ground = h * 0.72
                            sdf.box(0.0, ground, w, 1.5, 0.0)
                            sdf.fill(#xC8BFA6)

                            // The rail spans +/-3 units, the env's own bound.
                            let cx = w * 0.5 + self.cart_x * (w / 6.0)
                            let cw = w * 0.055
                            let ch = h * 0.045
                            sdf.box(cx - cw * 0.5, ground - ch, cw, ch, 3.0)
                            sdf.fill(#x3B4152)

                            // Pole, leaning by the recorded angle.
                            let len = h * 0.30
                            let tipx = cx + sin(self.angle) * len
                            let tipy = ground - ch - cos(self.angle) * len
                            sdf.move_to(cx, ground - ch)
                            sdf.line_to(tipx, tipy)
                            sdf.stroke(#x4A5164, 5.0)
                            sdf.circle(tipx, tipy, 7.0)
                            sdf.fill(#x8A6ED8)

                            // The push, drawn while it is being applied.
                            let ax = cx + 46.0 * sign(self.push)
                            let arrow = (1.0 - step(0.01, abs(self.push)))
                            sdf.move_to(ax, ground - ch * 2.0)
                            sdf.line_to(cx + 12.0 * sign(self.push), ground - ch * 2.0)
                            sdf.stroke(mix(#xA28BEA00, #xA28BEA, 1.0 - arrow), 3.0)
                            return sdf.result
                        }
                    }
                }
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
                        cursor: MouseCursor.Hand
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

/// What the transport reported this frame.
#[derive(Clone, Copy, PartialEq)]
pub enum Transport {
    Play,
    StepBack,
    StepForward,
    Restart,
    Push,
    Seek(f64),
}

impl InspectScreenRef {
    pub fn sync(&self, cx: &mut Cx, probe: Option<&crate::probe::Probe>) {
        let Some(mut inner) = self.borrow_mut() else { return };
        let root = &mut inner.view;

        let Some(p) = probe else {
            ux::head(cx, root, ids!(main.stage.stage_head), "ROLLOUT", "no checkpoint yet");
            root.label(cx, ids!(main.transport.t_body.controls.frame_lbl))
                .set_text(cx, "— / —");
            return;
        };

        let f = p.frame();
        let mut view = root.widget(cx, ids!(main.stage.viewport));
        let push_vis = if p.last_push == Some(p.cursor) { 1.0 } else { 0.0 };
        script_apply_eval!(cx, view, {
            draw_bg +: { cart_x: #(f.cart_x as f64) angle: #(f.angle as f64) push: #(push_vis) }
        });

        ux::head(cx, root, ids!(main.stage.stage_head), "ROLLOUT", &p.label);
        root.label(cx, ids!(main.transport.t_body.controls.frame_lbl))
            .set_text(cx, &format!("{} / {}", p.cursor, p.frames.len().saturating_sub(1)));

        let mut scrub = root.widget(cx, ids!(main.transport.t_body.scrub));
        let head = p.progress();
        script_apply_eval!(cx, scrub, { draw_bg +: { head: #(head) } });

        let play = root.widget(cx, ids!(main.transport.t_body.controls.b_play));
        play.label(cx, ids!(cap)).set_text(cx, if p.playing { "||" } else { ">" });

        // The observation the policy actually consumed on this tick.
        let rows: [(&str, String); 8] = [
            ("cart_x", format!("{:+.3}", f.cart_x)),
            ("pole_angle", format!("{:+.3}", f.angle)),
            ("action", format!("{:+.3}", f.action)),
            ("reward", format!("{:+.3}", f.reward)),
            ("frame", format!("{}", p.cursor)),
            ("frames", format!("{}", p.frames.len())),
            ("terminated", if f.fell { "fell".into() } else { "no".into() }),
            ("policy", "mean, no noise".into()),
        ];
        for (i, path) in OBS_ROWS.iter().enumerate() {
            let row = root.widget(cx, path);
            row.label(cx, ids!(k)).set_text(cx, rows[i].0);
            row.label(cx, ids!(v)).set_text(cx, &rows[i].1);
        }
        ux::head(cx, root, ids!(side.obs.obs_head), "STATE", "live");

        // Effort, from the action the policy chose.
        let effort = f.action.abs() as f64;
        for (i, path) in [
            ids!(side.torque.tq_body.m0) as &[LiveId],
            ids!(side.torque.tq_body.m1),
            ids!(side.torque.tq_body.m2),
            ids!(side.torque.tq_body.m3),
            ids!(side.torque.tq_body.m4),
        ]
        .iter()
        .enumerate()
        {
            let row = root.widget(cx, path);
            row.label(cx, ids!(k)).set_text(cx, if i == 0 { "cart_force" } else { "—" });
            // Not named `v`: script_apply_eval! has a local by that name.
            let level = if i == 0 { effort } else { 0.0 };
            let mut bar = row.widget(cx, ids!(bar));
            script_apply_eval!(cx, bar, { draw_bg +: { v: #(level) } });
        }
        ux::head(cx, root, ids!(side.torque.tq_head), "EFFORT", "1 actuator");
    }

    /// What the operator pressed, if anything.
    pub fn transport(&self, cx: &mut Cx, actions: &Actions) -> Option<Transport> {
        let mut inner = self.borrow_mut()?;
        let v = &mut inner.view;
        for (path, out) in [
            (ids!(main.transport.t_body.controls.b_play) as &[LiveId], Transport::Play),
            (ids!(main.transport.t_body.controls.b_step_b), Transport::StepBack),
            (ids!(main.transport.t_body.controls.b_step_f), Transport::StepForward),
            (ids!(main.transport.t_body.controls.b_start), Transport::Restart),
            (ids!(main.transport.t_body.controls.b_push), Transport::Push),
        ] {
            let b = v.widget(cx, path);
            if !b.is_empty() && ux::view_clicked(actions, b.widget_uid()) {
                return Some(out);
            }
        }
        // Clicking the scrubber seeks to that fraction of the rollout.
        let scrub = v.widget(cx, ids!(main.transport.t_body.scrub));
        if !scrub.is_empty() {
            let rect = scrub.area().rect(cx);
            for a in actions.filter_widget_actions(scrub.widget_uid()) {
                if let ViewAction::FingerUp(fe) = a.cast::<ViewAction>() {
                    if fe.is_over && rect.size.x > 1.0 {
                        return Some(Transport::Seek((fe.abs.x - rect.pos.x) / rect.size.x));
                    }
                }
            }
        }
        None
    }
}
