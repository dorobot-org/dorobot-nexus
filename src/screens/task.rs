//! Task — say what "good" means, in terms that can be blamed individually.

use makepad_widgets::*;

use crate::plot::{PlotWidgetRefExt, Series, SERIES_COLORS};
use crate::state::Run;
use crate::ux;

script_mod! {
    use mod.prelude.widgets.*
    use mod.widgets.*
    use mod.widgets.ux.*

    let TermRow = View{
        width: Fill height: 46
        flow: Right
        align: Align{y: 0.5}
        spacing: 12.0
        padding: Inset{left: 14. right: 14.}
        swatch := View{
            width: 10 height: 10
            show_bg: true
            draw_bg +: {
                hue: instance(0.0)
                pixel: fn() {
                    let sdf = Sdf2d.viewport(self.pos * self.rect_size)
                    let c0 = mix(#xA28BEA, #x5CBE8B, step(0.5, self.hue))
                    let c1 = mix(c0, #xD6A254, step(1.5, self.hue))
                    let c2 = mix(c1, #x6BA1F8, step(2.5, self.hue))
                    let c3 = mix(c2, #xE2726B, step(3.5, self.hue))
                    let c  = mix(c3, #x99A9C6, step(4.5, self.hue))
                    sdf.box(0.0, 0.0, self.rect_size.x, self.rect_size.y, 2.0)
                    sdf.fill(c)
                    return sdf.result
                }
            }
        }
        name := Label{
            width: 170 text: "term"
            draw_text +: {
                text_style: mod.widgets.ux.TEXT_BODY{}
                get_color: fn() { return #xE7EAF3 }
            }
        }
        spark := Plot{ width: Fill height: 26 }
        weight := Label{
            width: 82 text: "0.00"
            draw_text +: {
                text_style: mod.widgets.ux.TEXT_META{}
                get_color: fn() { return #x98A1B8 }
            }
        }
    }

    let Stage = RoundedView{
        width: Fill height: 46
        flow: Down
        align: Align{x: 0.5, y: 0.5}
        spacing: 2.0
        draw_bg +: {
            on: instance(0.0)
            color: #x171C27
            border_color: #x252C3A
            border_size: 1.0
            border_radius: 6.0
        }
        s_name := Label{
            text: "stage"
            draw_text +: {
                text_style: mod.widgets.ux.TEXT_BODY{}
                get_color: fn() { return #xC7CEDD }
            }
        }
        s_thr := Label{
            text: "-"
            draw_text +: {
                text_style: mod.widgets.ux.TEXT_CHIP{}
                get_color: fn() { return #x6C7591 }
            }
        }
    }

    mod.widgets.TaskScreenBase = #(TaskScreen::register_widget(vm))
    mod.widgets.TaskScreen = set_type_default() do mod.widgets.TaskScreenBase{
        width: Fill height: Fill
        flow: Down
        spacing: 12.0
        padding: Inset{left: 14. right: 14. top: 12. bottom: 12.}
        show_bg: true
        draw_bg +: { color: #x0E1119 }

        terms := mod.widgets.ux.Card{
            width: Fill height: Fill
            flow: Down
            terms_head := mod.widgets.ux.PanelHead{ title +: {text: "REWARD TERMS"} }
            terms_body := View{
                width: Fill height: Fill flow: Down
                padding: Inset{top: 6. bottom: 8.}
                t0 := TermRow{ swatch +: {draw_bg +: {hue: 0.0}} }
                t1 := TermRow{ swatch +: {draw_bg +: {hue: 1.0}} }
                t2 := TermRow{ swatch +: {draw_bg +: {hue: 2.0}} }
                t3 := TermRow{ swatch +: {draw_bg +: {hue: 3.0}} }
                t4 := TermRow{ swatch +: {draw_bg +: {hue: 4.0}} }
                t5 := TermRow{ swatch +: {draw_bg +: {hue: 5.0}} }
                mod.widgets.ux.Filler{}
            }
        }

        curric := mod.widgets.ux.Card{
            width: Fill height: 132
            flow: Down
            cur_head := mod.widgets.ux.PanelHead{ title +: {text: "CURRICULUM"} }
            cur_body := View{
                width: Fill height: Fill flow: Right spacing: 10.0
                padding: Inset{left: 14. right: 14. top: 14. bottom: 16.}
                g0 := Stage{ s_name +: {text: "flat"}   s_thr +: {text: "cleared"} }
                g1 := Stage{ s_name +: {text: "slope"}  s_thr +: {text: "current"} draw_bg +: {on: 1.0 color: #x241E3C border_color: #xA28BEA border_size: 1.0 border_radius: 6.0} }
                g2 := Stage{ s_name +: {text: "stairs"} s_thr +: {text: "≥ 0.85"} }
                g3 := Stage{ s_name +: {text: "rough"}  s_thr +: {text: "≥ 0.90"} }
            }
        }
    }
}

#[derive(Script, ScriptHook, Widget)]
pub struct TaskScreen {
    #[deref]
    view: View,
}

impl Widget for TaskScreen {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        self.view.handle_event(cx, event, scope);
    }
    fn draw_walk(&mut self, cx: &mut Cx2d, scope: &mut Scope, walk: Walk) -> DrawStep {
        self.view.draw_walk(cx, scope, walk)
    }
}

const ROWS: [&[LiveId]; 6] = [
    ids!(terms.terms_body.t0), ids!(terms.terms_body.t1), ids!(terms.terms_body.t2),
    ids!(terms.terms_body.t3), ids!(terms.terms_body.t4), ids!(terms.terms_body.t5),
];

impl TaskScreenRef {
    pub fn sync(&self, cx: &mut Cx, run: &Run) {
        let Some(mut inner) = self.borrow_mut() else { return };
        let root = &mut inner.view;

        for (i, path) in ROWS.iter().enumerate() {
            let row = root.widget(cx, path);
            let Some(term) = run.terms.get(i) else {
                row.set_visible(cx, false);
                continue;
            };
            row.set_visible(cx, true);
            row.label(cx, ids!(name)).set_text(cx, &term.name);
            row.label(cx, ids!(weight)).set_text(cx, &format!("{:+.5}", term.weight));
            // Each term carries its own evidence, so weights are edited against
            // what the term actually did rather than against intuition.
            let plot = row.plot(cx, ids!(spark));
            plot.set_grid_rows(0);
            plot.set_weight(1.3);
            plot.set_series(cx, vec![Series {
                name: term.name.clone(),
                color: SERIES_COLORS[i % SERIES_COLORS.len()],
                points: term.series.clone(),
            }]);
        }
        ux::head(cx, root, ids!(terms.terms_head), "REWARD TERMS",
                 &format!("{} active", run.terms.len()));
        ux::head(cx, root, ids!(curric.cur_head), "CURRICULUM", "stage 2 of 4");
    }
}
