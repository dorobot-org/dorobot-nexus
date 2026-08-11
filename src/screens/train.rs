//! Train — judge a six-hour run at minute ten, and know why.

use makepad_widgets::*;

use crate::plot::{PlotWidgetExt, Series, SERIES_COLORS};
use crate::state::Run;
use crate::ux;

script_mod! {
    use mod.prelude.widgets.*
    use mod.widgets.*
    use mod.widgets.ux.*

    // A headline run statistic. Large enough to read from across a desk, which
    // is the actual use: you glance at this while doing something else.
    let Stat = View{
        width: Fill height: Fit
        flow: Down
        padding: Inset{left: 16. right: 16. top: 10. bottom: 12.}
        spacing: 3.0
        k := Label{
            text: "STAT"
            draw_text +: {
                text_style: mod.widgets.ux.TEXT_CHIP{}
                get_color: fn() { return #x6C7591 }
            }
        }
        v := Label{
            text: "-"
            draw_text +: {
                text_style: mod.widgets.ux.TEXT_NUM{}
                get_color: fn() { return #xE7EAF3 }
            }
        }
    }

    // One environment in the contact sheet. `lean` bends the figure, `fell`
    // tints the cell. Schematic on purpose: with no simulator attached there
    // are no environment states to render, and a photoreal cell would be a lie.
    let EnvCell = View{
        width: Fill height: Fill
        show_bg: true
        draw_bg +: {
            lean: instance(0.0)
            fell: instance(0.0)
            pixel: fn() {
                let sdf = Sdf2d.viewport(self.pos * self.rect_size)
                let w = self.rect_size.x
                let h = self.rect_size.y
                // Studio gradient, tinted when the environment terminated.
                let sky = mix(#xFBF3F4, #xEFE6CE, self.pos.y)
                let bg = mix(sky, #xE8B4AE, self.fell * 0.8)
                sdf.box(0.0, 0.0, w, h, 3.0)
                sdf.fill(bg)
                // A humanoid reduced to what reads at 40px: torso, head, limbs.
                let cx = w * (0.5 + 0.14 * self.lean)
                let ink = mix(#x3B4152, #xA33A33, self.fell)
                sdf.move_to(w * 0.5, h * 0.74)
                sdf.line_to(cx, h * 0.34)
                sdf.stroke(ink, max(w * 0.045, 1.2))
                sdf.circle(cx, h * 0.26, max(w * 0.075, 1.6))
                sdf.fill(ink)
                sdf.move_to(cx, h * 0.44)
                sdf.line_to(cx - w * 0.16, h * 0.56)
                sdf.stroke(ink, max(w * 0.035, 1.0))
                sdf.move_to(cx, h * 0.44)
                sdf.line_to(cx + w * 0.16, h * 0.56)
                sdf.stroke(ink, max(w * 0.035, 1.0))
                sdf.move_to(w * 0.5, h * 0.74)
                sdf.line_to(w * 0.5 - w * 0.10, h * 0.92)
                sdf.stroke(ink, max(w * 0.04, 1.1))
                sdf.move_to(w * 0.5, h * 0.74)
                sdf.line_to(w * 0.5 + w * 0.10, h * 0.92)
                sdf.stroke(ink, max(w * 0.04, 1.1))
                return sdf.result
            }
        }
    }

    let Row = View{
        width: Fill height: 24
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

    mod.widgets.TrainScreenBase = #(TrainScreen::register_widget(vm))
    mod.widgets.TrainScreen = set_type_default() do mod.widgets.TrainScreenBase{
        width: Fill height: Fill
        flow: Down
        spacing: 12.0
        padding: Inset{left: 14. right: 14. top: 12. bottom: 12.}
        show_bg: true
        draw_bg +: { color: #x0E1119 }

        header := mod.widgets.ux.Card{
            width: Fill height: Fit
            flow: Down
            title_row := View{
                width: Fill height: Fit
                flow: Right
                align: Align{y: 0.5}
                padding: Inset{left: 16. right: 14. top: 11.}
                spacing: 10.0
                run_id := Label{
                    text: "run"
                    draw_text +: {
                        text_style: mod.widgets.ux.TEXT_TITLE{}
                        get_color: fn() { return #xE7EAF3 }
                    }
                }
                run_chip := mod.widgets.ux.Chip{ draw_bg +: {tone: 4.0} label +: {text: "training" draw_text +: {tone: 4.0}} }
                mod.widgets.ux.Filler{}
                scene_lbl := Label{
                    text: "-"
                    draw_text +: {
                        text_style: mod.widgets.ux.TEXT_META{}
                        get_color: fn() { return #x6C7591 }
                    }
                }
            }
            stats := View{
                width: Fill height: Fit
                flow: Right
                st_envs  := Stat{ k +: {text: "ENVIRONMENTS"} }
                st_rate  := Stat{ k +: {text: "STEPS / SEC"} }
                st_time  := Stat{ k +: {text: "ELAPSED"} }
                st_prog  := Stat{ k +: {text: "PROGRESS"} }
            }
        }

        chart := mod.widgets.ux.Card{
            width: Fill height: Fill
            flow: Down
            chart_head := mod.widgets.ux.PanelHead{ title +: {text: "REWARD"} }
            chart_body := View{
                width: Fill height: Fill
                padding: Inset{left: 10. right: 10. top: 8. bottom: 8.}
                curves := Plot{}
            }
        }

        lower := View{
            width: Fill height: 260
            flow: Right
            spacing: 12.0

            envs := mod.widgets.ux.Card{
                width: 340 height: Fill
                flow: Down
                envs_head := mod.widgets.ux.PanelHead{ title +: {text: "ENVIRONMENTS"} }
                envs_body := View{
                    width: Fill height: Fill flow: Down spacing: 4.0
                    padding: Inset{left: 10. right: 10. top: 10. bottom: 10.}
                    r0 := View{ width: Fill height: Fill flow: Right spacing: 4.0
                        c0 := EnvCell{} c1 := EnvCell{} c2 := EnvCell{} c3 := EnvCell{} }
                    r1 := View{ width: Fill height: Fill flow: Right spacing: 4.0
                        c4 := EnvCell{} c5 := EnvCell{} c6 := EnvCell{} c7 := EnvCell{} }
                    r2 := View{ width: Fill height: Fill flow: Right spacing: 4.0
                        c8 := EnvCell{} c9 := EnvCell{} c10 := EnvCell{} c11 := EnvCell{} }
                    r3 := View{ width: Fill height: Fill flow: Right spacing: 4.0
                        c12 := EnvCell{} c13 := EnvCell{} c14 := EnvCell{} c15 := EnvCell{} }
                }
            }

            diag := mod.widgets.ux.Card{
                width: Fill height: Fill
                flow: Down
                diag_head := mod.widgets.ux.PanelHead{ title +: {text: "DIAGNOSIS"} }
                diag_body := View{
                    width: Fill height: Fill flow: Down spacing: 10.0
                    padding: Inset{left: 12. right: 12. top: 12. bottom: 12.}
                    card := RoundedView{
                        width: Fill height: Fit
                        flow: Down
                        spacing: 6.0
                        padding: Inset{left: 14. right: 14. top: 12. bottom: 13.}
                        draw_bg +: {
                            color: #x1B1710
                            border_color: #xD6A254
                            border_size: 1.0
                            border_radius: 6.0
                        }
                        headline := Label{
                            text: "-"
                            draw_text +: {
                                text_style: mod.widgets.ux.TEXT_TITLE{}
                                get_color: fn() { return #xE0B473 }
                            }
                        }
                        detail := Label{
                            width: Fill
                            text: ""
                            draw_text +: {
                                text_style: mod.widgets.ux.TEXT_BODY{}
                                get_color: fn() { return #xC7CEDD }
                            }
                        }
                    }
                    clean := Label{
                        text: "Nothing to report."
                        draw_text +: {
                            text_style: mod.widgets.ux.TEXT_BODY{}
                            get_color: fn() { return #x6C7591 }
                        }
                    }
                    mod.widgets.ux.Filler{}
                }
            }

            ckpts := mod.widgets.ux.Card{
                width: 280 height: Fill
                flow: Down
                ck_head := mod.widgets.ux.PanelHead{ title +: {text: "CHECKPOINTS"} }
                ck_body := View{
                    width: Fill height: Fill flow: Down
                    padding: Inset{left: 12. right: 12. top: 8. bottom: 10.}
                    k0 := Row{} k1 := Row{} k2 := Row{} k3 := Row{}
                    mod.widgets.ux.Filler{}
                }
            }
        }
    }
}

#[derive(Script, ScriptHook, Widget)]
pub struct TrainScreen {
    #[deref]
    view: View,
}

impl Widget for TrainScreen {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        self.view.handle_event(cx, event, scope);
    }
    fn draw_walk(&mut self, cx: &mut Cx2d, scope: &mut Scope, walk: Walk) -> DrawStep {
        self.view.draw_walk(cx, scope, walk)
    }
}

const CELLS: [&[LiveId]; 16] = [
    ids!(lower.envs.envs_body.r0.c0), ids!(lower.envs.envs_body.r0.c1),
    ids!(lower.envs.envs_body.r0.c2), ids!(lower.envs.envs_body.r0.c3),
    ids!(lower.envs.envs_body.r1.c4), ids!(lower.envs.envs_body.r1.c5),
    ids!(lower.envs.envs_body.r1.c6), ids!(lower.envs.envs_body.r1.c7),
    ids!(lower.envs.envs_body.r2.c8), ids!(lower.envs.envs_body.r2.c9),
    ids!(lower.envs.envs_body.r2.c10), ids!(lower.envs.envs_body.r2.c11),
    ids!(lower.envs.envs_body.r3.c12), ids!(lower.envs.envs_body.r3.c13),
    ids!(lower.envs.envs_body.r3.c14), ids!(lower.envs.envs_body.r3.c15),
];

const CKPT_ROWS: [&[LiveId]; 4] = [
    ids!(lower.ckpts.ck_body.k0), ids!(lower.ckpts.ck_body.k1),
    ids!(lower.ckpts.ck_body.k2), ids!(lower.ckpts.ck_body.k3),
];

impl TrainScreenRef {
    pub fn sync(&self, cx: &mut Cx, run: &Run) {
        let Some(mut inner) = self.borrow_mut() else { return };
        let root = &mut inner.view;

        root.label(cx, ids!(header.title_row.run_id)).set_text(cx, &run.id);
        root.label(cx, ids!(header.title_row.scene_lbl))
            .set_text(cx, &format!("{} · seed {}", run.scene, run.seed));
        let chip = root.widget(cx, ids!(header.title_row.run_chip));
        chip.label(cx, ids!(label)).set_text(cx, run.state.label());
        let tone = run.state.tone();
        let mut c = chip.clone();
        script_apply_eval!(cx, c, { draw_bg +: { tone: #(tone) } });
        let mut cl = chip.widget(cx, ids!(label));
        script_apply_eval!(cx, cl, { draw_text +: { tone: #(tone) } });

        for (path, value) in [
            (ids!(header.stats.st_envs) as &[LiveId], format!("{}", run.envs)),
            (ids!(header.stats.st_rate), format!("{:.1}k", run.steps_per_sec / 1000.0)),
            (ids!(header.stats.st_time), run.elapsed_label()),
            (ids!(header.stats.st_prog), format!("{:.0}%", run.progress() * 100.0)),
        ] {
            root.widget(cx, path).label(cx, ids!(v)).set_text(cx, &value);
        }
        ux::head(cx, root, ids!(chart.chart_head), "REWARD", &run.steps_label());

        // One line per named term: the whole reason terms are named.
        let series: Vec<Series> = run
            .terms
            .iter()
            .enumerate()
            .map(|(i, t)| Series {
                name: t.name.clone(),
                color: SERIES_COLORS[i % SERIES_COLORS.len()],
                points: t.series.clone(),
            })
            .collect();
        root.plot(cx, ids!(chart.chart_body.curves)).set_series(cx, series);

        // Environment cells. Falls are drawn from the run's terminal fall rate,
        // so the sheet agrees with the curve above it.
        let fall_rate = run.fall_rate.last().copied().unwrap_or(0.0);
        let fallen = (fall_rate * CELLS.len() as f64).round() as usize;
        for (i, path) in CELLS.iter().enumerate() {
            let fell = if i < fallen { 1.0 } else { 0.0 };
            // Live pose when the trainer supplies one; otherwise a stable
            // pseudo-lean so a fixture run still reads as a population.
            let lean = run
                .leans
                .get(i)
                .map(|l| *l as f64)
                .unwrap_or_else(|| (i as f64 * 2.399).sin() * 0.6);
            let mut w = root.widget(cx, path);
            script_apply_eval!(cx, w, { draw_bg +: { lean: #(lean) fell: #(fell) } });
        }
        ux::head(cx, root, ids!(lower.envs.envs_head), "ENVIRONMENTS",
                 &format!("{} of {}", CELLS.len(), run.envs));

        // Diagnosis: the app's opinion, in words.
        let findings = run.findings();
        let card = root.widget(cx, ids!(lower.diag.diag_body.card));
        let clean = root.widget(cx, ids!(lower.diag.diag_body.clean));
        match findings.first() {
            Some(f) => {
                card.set_visible(cx, true);
                clean.set_visible(cx, false);
                card.label(cx, ids!(headline)).set_text(cx, &f.headline);
                card.label(cx, ids!(detail)).set_text(cx, &f.detail);
            }
            None => {
                card.set_visible(cx, false);
                clean.set_visible(cx, true);
            }
        }
        ux::head(cx, root, ids!(lower.diag.diag_head), "DIAGNOSIS",
                 &format!("{}", findings.len()));

        for (i, path) in CKPT_ROWS.iter().enumerate() {
            let row = root.widget(cx, path);
            match run.checkpoints.get(i) {
                Some(ck) => {
                    row.set_visible(cx, true);
                    row.label(cx, ids!(k)).set_text(cx, &ck.name());
                    row.label(cx, ids!(v)).set_text(cx, &format!("{:.2}", ck.score));
                }
                None => row.set_visible(cx, false),
            }
        }
        ux::head(cx, root, ids!(lower.ckpts.ck_head), "CHECKPOINTS",
                 &format!("{}", run.checkpoints.len()));
    }
}
