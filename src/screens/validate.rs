//! Validate — predict, indoors, whether this policy survives a real robot.

use makepad_widgets::*;

use crate::sweep::{self, Surface};
use crate::ux;

script_mod! {
    use mod.prelude.widgets.*
    use mod.widgets.*
    use mod.widgets.ux.*

    // The robustness surface. `v` is the pass rate for this cell: green in the
    // middle and red at the edges means the policy learned the average, not the
    // task — which is the single most useful thing this screen can tell you.
    let Cell = View{
        width: Fill height: Fill
        show_bg: true
        draw_bg +: {
            v: instance(1.0)
            known: instance(0.0)
            pixel: fn() {
                let sdf = Sdf2d.viewport(self.pos * self.rect_size)
                let fail = mix(#x8E3A36, #xB6803A, clamp(self.v * 2.0, 0.0, 1.0))
                let good = mix(fail, #x3E8C62, clamp((self.v - 0.5) * 2.0, 0.0, 1.0))
                // An unswept cell reads as empty. Painting it red would report
                // a failure nothing measured.
                sdf.box(0.0, 0.0, self.rect_size.x, self.rect_size.y, 2.0)
                sdf.fill(mix(#x1A1F2B, good, self.known))
                return sdf.result
            }
        }
    }

    let CmpRow = View{
        width: Fill height: 28
        flow: Right
        align: Align{y: 0.5}
        padding: Inset{left: 12. right: 12.}
        k := Label{
            width: Fill text: "-"
            draw_text +: {
                text_style: mod.widgets.ux.TEXT_META{}
                get_color: fn() { return #x98A1B8 }
            }
        }
        a := Label{
            width: 74 text: "-"
            draw_text +: {
                text_style: mod.widgets.ux.TEXT_META{}
                get_color: fn() { return #xE7EAF3 }
            }
        }
        b := Label{
            width: 74 text: "-"
            draw_text +: {
                text_style: mod.widgets.ux.TEXT_META{}
                get_color: fn() { return #xE7EAF3 }
            }
        }
        d := Label{
            width: 66 text: "-"
            draw_text +: {
                text_style: mod.widgets.ux.TEXT_META{}
                get_color: fn() { return #xD6A254 }
            }
        }
    }

    let Legend = View{
        width: Fit height: Fit flow: Right align: Align{y: 0.5} spacing: 6.0
        sw := View{
            width: 10 height: 10
            show_bg: true
            draw_bg +: {
                v: instance(1.0)
                pixel: fn() {
                    let fail = mix(#x8E3A36, #xB6803A, clamp(self.v * 2.0, 0.0, 1.0))
                    return mix(fail, #x3E8C62, clamp((self.v - 0.5) * 2.0, 0.0, 1.0))
                }
            }
        }
        cap := Label{
            text: "-"
            draw_text +: {
                text_style: mod.widgets.ux.TEXT_CHIP{}
                get_color: fn() { return #x98A1B8 }
            }
        }
    }

    mod.widgets.ValidateScreenBase = #(ValidateScreen::register_widget(vm))
    mod.widgets.ValidateScreen = set_type_default() do mod.widgets.ValidateScreenBase{
        width: Fill height: Fill
        flow: Right
        spacing: 12.0
        padding: Inset{left: 14. right: 14. top: 12. bottom: 12.}
        show_bg: true
        draw_bg +: { color: #x0E1119 }

        rob := mod.widgets.ux.Card{
            width: Fill height: Fill
            flow: Down
            rob_head := mod.widgets.ux.PanelHead{ title +: {text: "ROBUSTNESS"} }
            rob_body := View{
                width: Fill height: Fill flow: Down spacing: 10.0
                padding: Inset{left: 14. right: 14. top: 12. bottom: 12.}
                grid := View{
                    width: Fill height: Fill flow: Down spacing: 3.0
                    gr0 := View{ width: Fill height: Fill flow: Right spacing: 3.0
                        c0_0 := Cell{}
                        c0_1 := Cell{}
                        c0_2 := Cell{}
                        c0_3 := Cell{}
                        c0_4 := Cell{}
                        c0_5 := Cell{}
                        c0_6 := Cell{}
                        c0_7 := Cell{}
                    }
                    gr1 := View{ width: Fill height: Fill flow: Right spacing: 3.0
                        c1_0 := Cell{}
                        c1_1 := Cell{}
                        c1_2 := Cell{}
                        c1_3 := Cell{}
                        c1_4 := Cell{}
                        c1_5 := Cell{}
                        c1_6 := Cell{}
                        c1_7 := Cell{}
                    }
                    gr2 := View{ width: Fill height: Fill flow: Right spacing: 3.0
                        c2_0 := Cell{}
                        c2_1 := Cell{}
                        c2_2 := Cell{}
                        c2_3 := Cell{}
                        c2_4 := Cell{}
                        c2_5 := Cell{}
                        c2_6 := Cell{}
                        c2_7 := Cell{}
                    }
                    gr3 := View{ width: Fill height: Fill flow: Right spacing: 3.0
                        c3_0 := Cell{}
                        c3_1 := Cell{}
                        c3_2 := Cell{}
                        c3_3 := Cell{}
                        c3_4 := Cell{}
                        c3_5 := Cell{}
                        c3_6 := Cell{}
                        c3_7 := Cell{}
                    }
                    gr4 := View{ width: Fill height: Fill flow: Right spacing: 3.0
                        c4_0 := Cell{}
                        c4_1 := Cell{}
                        c4_2 := Cell{}
                        c4_3 := Cell{}
                        c4_4 := Cell{}
                        c4_5 := Cell{}
                        c4_6 := Cell{}
                        c4_7 := Cell{}
                    }
                }
                axis := Label{
                    text: ""
                    draw_text +: {
                        text_style: mod.widgets.ux.TEXT_CHIP{}
                        get_color: fn() { return #x6C7591 }
                    }
                }
                keys := View{
                    width: Fill height: Fit flow: Right spacing: 16.0 align: Align{y: 0.5}
                    run_btn := RoundedView{
                        width: Fit height: Fit
                        padding: Inset{left: 12. right: 12. top: 6. bottom: 7.}
                        cursor: MouseCursor.Hand
                        draw_bg +: {
                            color: #x2A2350
                            border_color: #xA28BEA
                            border_size: 1.0
                            border_radius: 5.0
                        }
                        run_lbl := Label{
                            text: "Run sweep"
                            draw_text +: {
                                text_style: mod.widgets.ux.TEXT_CHIP{}
                                get_color: fn() { return #xBCA9F5 }
                            }
                        }
                    }
                    l_ok   := Legend{ sw +: {draw_bg +: {v: 1.0}} cap +: {text: "pass"} }
                    l_warn := Legend{ sw +: {draw_bg +: {v: 0.5}} cap +: {text: "degraded"} }
                    l_fail := Legend{ sw +: {draw_bg +: {v: 0.0}} cap +: {text: "fail"} }
                }
            }
        }

        side := View{
            width: 380 height: Fill flow: Down spacing: 12.0

            cross := mod.widgets.ux.Card{
                width: Fill height: Fill
                flow: Down
                cross_head := mod.widgets.ux.PanelHead{ title +: {text: "CROSS-SIM"} }
                cross_body := View{
                    width: Fill height: Fill flow: Down
                    padding: Inset{left: 14. right: 14. top: 12. bottom: 14.}
                    note := Label{
                        width: Fill
                        text: "A policy is only checked against the engine it was trained in until it is checked against another one. MuJoCo is the reference."
                        draw_text +: {
                            text_style: mod.widgets.ux.TEXT_BODY{}
                            get_color: fn() { return #x98A1B8 }
                        }
                    }
                    mod.widgets.ux.Filler{}
                }
            }

            cmp := mod.widgets.ux.Card{
                width: Fill height: Fit
                flow: Down
                cmp_head := mod.widgets.ux.PanelHead{ title +: {text: "COMPARISON"} }
                cmp_body := View{
                    width: Fill height: Fit flow: Down
                    padding: Inset{top: 6. bottom: 10.}
                    hdr := CmpRow{ k +: {text: "metric"} a +: {text: "nexus"} b +: {text: "mujoco"} d +: {text: "Δ"} }
                    v0 := CmpRow{}
                    v1 := CmpRow{}
                    v2 := CmpRow{}
                    v3 := CmpRow{}
                }
            }
        }
    }
}

#[derive(Script, ScriptHook, Widget)]
pub struct ValidateScreen {
    #[deref]
    view: View,
    #[rust(false)]
    built: bool,
}

impl Widget for ValidateScreen {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        self.view.handle_event(cx, event, scope);
    }
    fn draw_walk(&mut self, cx: &mut Cx2d, scope: &mut Scope, walk: Walk) -> DrawStep {
        self.view.draw_walk(cx, scope, walk)
    }
}

const ROWS: [&[LiveId]; 5] = [
    ids!(rob.rob_body.grid.gr0), ids!(rob.rob_body.grid.gr1), ids!(rob.rob_body.grid.gr2),
    ids!(rob.rob_body.grid.gr3), ids!(rob.rob_body.grid.gr4),
];
const COLS: usize = sweep::COLS;

const CELLS: [[&[LiveId]; COLS]; 5] = [
    [ids!(c0_0), ids!(c0_1), ids!(c0_2), ids!(c0_3), ids!(c0_4), ids!(c0_5), ids!(c0_6), ids!(c0_7)],
    [ids!(c1_0), ids!(c1_1), ids!(c1_2), ids!(c1_3), ids!(c1_4), ids!(c1_5), ids!(c1_6), ids!(c1_7)],
    [ids!(c2_0), ids!(c2_1), ids!(c2_2), ids!(c2_3), ids!(c2_4), ids!(c2_5), ids!(c2_6), ids!(c2_7)],
    [ids!(c3_0), ids!(c3_1), ids!(c3_2), ids!(c3_3), ids!(c3_4), ids!(c3_5), ids!(c3_6), ids!(c3_7)],
    [ids!(c4_0), ids!(c4_1), ids!(c4_2), ids!(c4_3), ids!(c4_4), ids!(c4_5), ids!(c4_6), ids!(c4_7)],
];

impl ValidateScreenRef {
    pub fn sync(&self, cx: &mut Cx, surface: Option<&Surface>) {
        let Some(mut inner) = self.borrow_mut() else { return };
        inner.built = true;
        let root = &mut inner.view;

        for (r, row_path) in ROWS.iter().enumerate() {
            let row = root.widget(cx, row_path);
            for (c, cell_path) in CELLS[r].iter().enumerate() {
                // Not named `v`: script_apply_eval! has a local by that name.
                let (pass, known) = match surface.and_then(|s| s.cell(r, c)) {
                    Some(p) => (p as f64, 1.0),
                    None => (0.0, 0.0),
                };
                let mut cell = row.widget(cx, cell_path);
                script_apply_eval!(cx, cell, { draw_bg +: { v: #(pass) known: #(known) } });
            }
        }

        let (head, axis) = match surface {
            Some(s) if s.running => (
                format!("{}/{} cells · {}", s.done, s.total, s.label),
                format!(
                    "force {:.2} → {:.2}   ·   pole mass {:.1}× → {:.1}×",
                    sweep::FORCE_RANGE.0, sweep::FORCE_RANGE.1,
                    sweep::MASS_RANGE.0, sweep::MASS_RANGE.1
                ),
            ),
            Some(s) if s.done > 0 => (
                format!("{:.0}% of cells pass · {}", s.pass_fraction() * 100.0, s.label),
                format!(
                    "force {:.2} → {:.2}   ·   pole mass {:.1}× → {:.1}×",
                    sweep::FORCE_RANGE.0, sweep::FORCE_RANGE.1,
                    sweep::MASS_RANGE.0, sweep::MASS_RANGE.1
                ),
            ),
            Some(s) => (s.label.clone(), String::new()),
            None => (
                "not run".into(),
                "a policy is only checked where it has been swept".into(),
            ),
        };
        ux::head(cx, root, ids!(rob.rob_head), "ROBUSTNESS", &head);
        root.label(cx, ids!(rob.rob_body.axis)).set_text(cx, &axis);

        // The comparison table has no second simulator behind it yet, so it
        // says so rather than printing numbers nothing produced.
        for (path, k, a, b, d) in [
            (ids!(side.cmp.cmp_body.v0) as &[LiveId], "success rate", "—", "—", "—"),
            (ids!(side.cmp.cmp_body.v1), "mean reward", "—", "—", "—"),
            (ids!(side.cmp.cmp_body.v2), "tracking rmse", "—", "—", "—"),
            (ids!(side.cmp.cmp_body.v3), "fall rate", "—", "—", "—"),
        ] {
            let row = root.widget(cx, path);
            row.label(cx, ids!(k)).set_text(cx, k);
            row.label(cx, ids!(a)).set_text(cx, a);
            row.label(cx, ids!(b)).set_text(cx, b);
            row.label(cx, ids!(d)).set_text(cx, d);
        }
        ux::head(cx, root, ids!(side.cmp.cmp_head), "COMPARISON", "no second simulator");
        ux::head(cx, root, ids!(side.cross.cross_head), "CROSS-SIM", "not built");
    }

    /// True when "Run sweep" was pressed.
    pub fn clicked_run(&self, cx: &mut Cx, actions: &Actions) -> bool {
        let Some(mut inner) = self.borrow_mut() else { return false };
        let b = inner.view.widget(cx, ids!(rob.rob_body.keys.run_btn));
        !b.is_empty() && ux::view_clicked(actions, b.widget_uid())
    }
}
