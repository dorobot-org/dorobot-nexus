//! Runs — the document store. Everything else hangs off what is listed here.

use makepad_widgets::*;

use crate::plot::{PlotWidgetRefExt, Series, SERIES_COLORS};
use crate::state::Run;
use crate::ux;

script_mod! {
    use mod.prelude.widgets.*
    use mod.widgets.*
    use mod.widgets.ux.*

    let RunCard = mod.widgets.ux.Card{
        width: Fill height: 190
        flow: Down
        cursor: MouseCursor.Hand
        draw_bg +: {
            sel: instance(0.0)
            color: #x131822
            border_color: #x252C3A
            border_size: 1.0
            border_radius: 8.0
        }
        body := View{
            width: Fill height: Fill flow: Down spacing: 7.0
            padding: Inset{left: 14. right: 14. top: 13. bottom: 12.}
            name := Label{
                text: "run"
                draw_text +: {
                    text_style: mod.widgets.ux.TEXT_TITLE{}
                    get_color: fn() { return #xE7EAF3 }
                }
            }
            meta := Label{
                text: "-"
                draw_text +: {
                    text_style: mod.widgets.ux.TEXT_META{}
                    get_color: fn() { return #x6C7591 }
                }
            }
            spark := Plot{ width: Fill height: Fill }
            foot := View{
                width: Fill height: Fit flow: Right align: Align{y: 0.5} spacing: 7.0
                chip := mod.widgets.ux.Chip{}
                mod.widgets.ux.Filler{}
                score := Label{
                    text: "-"
                    draw_text +: {
                        text_style: mod.widgets.ux.TEXT_META{}
                        get_color: fn() { return #xE7EAF3 }
                    }
                }
            }
        }
    }

    mod.widgets.RunsScreenBase = #(RunsScreen::register_widget(vm))
    mod.widgets.RunsScreen = set_type_default() do mod.widgets.RunsScreenBase{
        width: Fill height: Fill
        flow: Down
        spacing: 12.0
        padding: Inset{left: 14. right: 14. top: 12. bottom: 12.}
        show_bg: true
        draw_bg +: { color: #x0E1119 }

        head := View{
            width: Fill height: Fit flow: Right align: Align{y: 0.5} spacing: 10.0
            title := Label{
                text: "Runs"
                draw_text +: {
                    text_style: mod.widgets.ux.TEXT_H1{}
                    get_color: fn() { return #xE7EAF3 }
                }
            }
            mod.widgets.ux.Filler{}
            note := Label{
                text: ""
                draw_text +: {
                    text_style: mod.widgets.ux.TEXT_META{}
                    get_color: fn() { return #x6C7591 }
                }
            }
        }

        grid := View{
            width: Fill height: Fit flow: Right spacing: 12.0
            card_0 := RunCard{}
            card_1 := RunCard{}
            card_2 := RunCard{}
        }
        mod.widgets.ux.Filler{}
    }
}

#[derive(Script, ScriptHook, Widget)]
pub struct RunsScreen {
    #[deref]
    view: View,
}

impl Widget for RunsScreen {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        self.view.handle_event(cx, event, scope);
    }
    fn draw_walk(&mut self, cx: &mut Cx2d, scope: &mut Scope, walk: Walk) -> DrawStep {
        self.view.draw_walk(cx, scope, walk)
    }
}

const CARDS: [&[LiveId]; 3] = [ids!(grid.card_0), ids!(grid.card_1), ids!(grid.card_2)];

impl RunsScreenRef {
    pub fn sync(&self, cx: &mut Cx, runs: &[Run], selected: usize) {
        let Some(mut inner) = self.borrow_mut() else { return };
        let root = &mut inner.view;

        root.label(cx, ids!(head.note))
            .set_text(cx, "run 1 is live · the others are fixtures");

        for (i, path) in CARDS.iter().enumerate() {
            let card = root.widget(cx, path);
            let Some(run) = runs.get(i) else {
                card.set_visible(cx, false);
                continue;
            };
            card.set_visible(cx, true);
            card.label(cx, ids!(body.name)).set_text(cx, &run.id);
            card.label(cx, ids!(body.meta))
                .set_text(cx, &format!("{} · seed {} · {}", run.scene, run.seed, run.steps_label()));

            let best = run.checkpoints.iter().map(|c| c.score).fold(0.0_f64, f64::max);
            card.label(cx, ids!(body.foot.score)).set_text(cx, &format!("{best:.2}"));

            let chip = card.widget(cx, ids!(body.foot.chip));
            chip.label(cx, ids!(label)).set_text(cx, run.state.label());
            let tone = run.state.tone();
            let mut c = chip.clone();
            script_apply_eval!(cx, c, { draw_bg +: { tone: #(tone) } });
            let mut cl = chip.widget(cx, ids!(label));
            script_apply_eval!(cx, cl, { draw_text +: { tone: #(tone) } });

            // One line: the run's total reward. The card answers "did it learn",
            // not "how"; that is Train's job.
            let plot = card.plot(cx, ids!(body.spark));
            plot.set_grid_rows(0);
            plot.set_series(cx, vec![Series {
                name: "total".into(),
                color: SERIES_COLORS[0],
                points: run.total_reward(),
            }]);

            let sel = if i == selected { 1.0 } else { 0.0 };
            let mut w = card.clone();
            script_apply_eval!(cx, w, { draw_bg +: { border_color: #(if sel > 0.5 { vec4(0.64,0.55,0.92,1.0) } else { vec4(0.145,0.173,0.227,1.0) }) } });
        }
    }

    /// Index of the card released under the pointer.
    pub fn clicked(&self, cx: &mut Cx, actions: &Actions) -> Option<usize> {
        let mut inner = self.borrow_mut()?;
        for (i, path) in CARDS.iter().enumerate() {
            let card = inner.view.widget(cx, path);
            if !card.is_empty() && ux::view_clicked(actions, card.widget_uid()) {
                return Some(i);
            }
        }
        None
    }
}
