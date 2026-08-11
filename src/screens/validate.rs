//! Validate — predict, indoors, whether this policy survives a real robot.

use makepad_widgets::*;

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
            pixel: fn() {
                let sdf = Sdf2d.viewport(self.pos * self.rect_size)
                let fail = mix(#x8E3A36, #xB6803A, clamp(self.v * 2.0, 0.0, 1.0))
                let good = mix(fail, #x3E8C62, clamp((self.v - 0.5) * 2.0, 0.0, 1.0))
                sdf.box(0.0, 0.0, self.rect_size.x, self.rect_size.y, 2.0)
                sdf.fill(good)
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
                    text: "friction  0.4 → 1.2      ·      payload  0 → 5 kg"
                    draw_text +: {
                        text_style: mod.widgets.ux.TEXT_CHIP{}
                        get_color: fn() { return #x6C7591 }
                    }
                }
                keys := View{
                    width: Fill height: Fit flow: Right spacing: 16.0
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
const COLS: usize = 8;

const CELLS: [[&[LiveId]; COLS]; 5] = [
    [ids!(c0_0), ids!(c0_1), ids!(c0_2), ids!(c0_3), ids!(c0_4), ids!(c0_5), ids!(c0_6), ids!(c0_7)],
    [ids!(c1_0), ids!(c1_1), ids!(c1_2), ids!(c1_3), ids!(c1_4), ids!(c1_5), ids!(c1_6), ids!(c1_7)],
    [ids!(c2_0), ids!(c2_1), ids!(c2_2), ids!(c2_3), ids!(c2_4), ids!(c2_5), ids!(c2_6), ids!(c2_7)],
    [ids!(c3_0), ids!(c3_1), ids!(c3_2), ids!(c3_3), ids!(c3_4), ids!(c3_5), ids!(c3_6), ids!(c3_7)],
    [ids!(c4_0), ids!(c4_1), ids!(c4_2), ids!(c4_3), ids!(c4_4), ids!(c4_5), ids!(c4_6), ids!(c4_7)],
];

/// Pass rate over the sweep. Strong in the middle, weak at the corners: the
/// shape a policy has when its randomization was too narrow.
fn cell_value(row: usize, col: usize) -> f64 {
    let x = col as f64 / (COLS - 1) as f64;
    let y = row as f64 / (ROWS.len() - 1) as f64;
    let dx = (x - 0.45) / 0.62;
    let dy = (y - 0.40) / 0.58;
    (1.0 - (dx * dx + dy * dy)).clamp(0.0, 1.0)
}

impl ValidateScreenRef {
    pub fn sync(&self, cx: &mut Cx) {
        let Some(mut inner) = self.borrow_mut() else { return };
        let first = !inner.built;
        inner.built = true;
        let root = &mut inner.view;

        if first {
            // The sweep does not change while you look at it, so the cells are
            // painted once rather than on every sync.
            for (r, row_path) in ROWS.iter().enumerate() {
                let row = root.widget(cx, row_path);
                for (c, cell_path) in CELLS[r].iter().enumerate() {
                    // Not named `v`: script_apply_eval! has a local by that
                    // name and the collision moves the wrong value.
                    let pass = cell_value(r, c);
                    let mut cell = row.widget(cx, cell_path);
                    script_apply_eval!(cx, cell, { draw_bg +: { v: #(pass) } });
                }
            }
        }

        let passing = (0..ROWS.len())
            .flat_map(|r| (0..COLS).map(move |c| cell_value(r, c)))
            .filter(|v| *v > 0.5)
            .count();
        ux::head(cx, root, ids!(rob.rob_head), "ROBUSTNESS",
                 &format!("{passing}/{} pass", ROWS.len() * COLS));

        for (path, k, a, b, d) in [
            (ids!(side.cmp.cmp_body.v0) as &[LiveId], "success rate", "0.93", "0.91", "−0.02"),
            (ids!(side.cmp.cmp_body.v1), "mean reward", "0.81", "0.79", "−0.02"),
            (ids!(side.cmp.cmp_body.v2), "tracking rmse", "0.048", "0.052", "+0.004"),
            (ids!(side.cmp.cmp_body.v3), "fall rate", "1.6%", "2.8%", "+1.2"),
        ] {
            let row = root.widget(cx, path);
            row.label(cx, ids!(k)).set_text(cx, k);
            row.label(cx, ids!(a)).set_text(cx, a);
            row.label(cx, ids!(b)).set_text(cx, b);
            row.label(cx, ids!(d)).set_text(cx, d);
        }
        ux::head(cx, root, ids!(side.cmp.cmp_head), "COMPARISON", "nexus vs mujoco");
        ux::head(cx, root, ids!(side.cross.cross_head), "CROSS-SIM", "not run");
    }
}
