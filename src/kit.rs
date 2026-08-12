//! Custom-drawn widgets: sparkline (+goal variant), segmented progress bar,
//! sweep heatmap, and the scrub bar. All follow the proven pattern from
//! dorobot's TimeSeriesPlot: a DrawQuad-deref primitive stamped with
//! `draw_abs` after the view background draws.

use makepad_widgets::*;

use crate::tokens::pal;

script_mod! {
    use mod.prelude.widgets.*
    use mod.widgets.*

    let DrawFlat = set_type_default() do #(DrawFlat::script_shader(vm)){
        ..mod.draw.DrawQuad
        pixel: fn() { return self.color }
    }

    mod.widgets.SparkBase = #(Spark::register_widget(vm))
    mod.widgets.Spark = set_type_default() do mod.widgets.SparkBase{
        width: Fill
        height: 46
        draw_flat: DrawFlat{}
    }

    mod.widgets.SegsbarBase = #(Segsbar::register_widget(vm))
    mod.widgets.Segsbar = set_type_default() do mod.widgets.SegsbarBase{
        width: Fill
        height: 9
        draw_flat: DrawFlat{}
    }

    mod.widgets.HeatBase = #(Heat::register_widget(vm))
    mod.widgets.Heat = set_type_default() do mod.widgets.HeatBase{
        width: Fill
        height: 96
        draw_flat: DrawFlat{}
    }

    mod.widgets.ScrubBase = #(Scrub::register_widget(vm))
    mod.widgets.Scrub = set_type_default() do mod.widgets.ScrubBase{
        width: Fill
        height: 14
        draw_flat: DrawFlat{}
    }
}

#[derive(Script, ScriptHook)]
#[repr(C)]
pub struct DrawFlat {
    #[deref]
    draw_super: DrawQuad,
    #[live]
    color: Vec4f,
}

fn th(light: f32) -> pal::Th {
    pal::Th { l: light }
}

// ============================================================== sparkline --

#[derive(Script, ScriptHook, Widget)]
pub struct Spark {
    #[deref]
    view: View,
    #[live]
    draw_flat: DrawFlat,
    #[rust]
    pub data: Vec<f64>,
    /// Dashed threshold line value (goal chart) when set.
    #[rust]
    pub goal: Option<f64>,
    /// 0 ok-green, 1 orange, 2 cyan.
    #[rust]
    pub tone: u8,
    #[rust]
    pub light: f32,
}

impl Widget for Spark {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        self.view.handle_event(cx, event, scope);
    }
    fn draw_walk(&mut self, cx: &mut Cx2d, scope: &mut Scope, walk: Walk) -> DrawStep {
        while self.view.draw_walk(cx, scope, walk).step().is_some() {}
        let rect = self.view.area().rect(cx);
        if self.data.len() >= 2 && rect.size.x > 4.0 {
            let t = th(self.light);
            let color = match self.tone {
                0 => t.ok(),
                1 => t.vio(),
                _ => t.cy(),
            };
            let mut mn = f64::MAX;
            let mut mx = f64::MIN;
            for v in &self.data {
                mn = mn.min(*v);
                mx = mx.max(*v);
            }
            if let Some(g) = self.goal {
                mn = mn.min(g);
                mx = mx.max(g);
            }
            // near-flat data (a converged metric) still deserves a visible line
            let rg = (mx - mn).max(0.02);
            let h = rect.size.y - 4.0;
            let y_of = |v: f64| rect.pos.y + 2.0 + (1.0 - (v - mn) / rg) * h;
            let n = self.data.len();
            let x_of = |i: usize| rect.pos.x + i as f64 / (n - 1) as f64 * rect.size.x;
            // baseline hair
            self.draw_flat.color = t.edge();
            self.draw_flat.draw_abs(cx, Rect { pos: dvec2(rect.pos.x, rect.pos.y + rect.size.y - 1.0), size: dvec2(rect.size.x, 1.0) });
            // area fill: quad blending ignores alpha here, so premix an
            // opaque wash toward the panel ground instead
            let ground = t.deep();
            let fill = pal::mixv(ground, color, 0.22);
            for i in 0..n {
                let x = x_of(i);
                let y = y_of(self.data[i]);
                let w = (rect.size.x / (n - 1) as f64).max(1.0);
                self.draw_flat.color = fill;
                self.draw_flat.draw_abs(cx, Rect { pos: dvec2(x, y), size: dvec2(w, (rect.pos.y + rect.size.y - 1.0 - y).max(0.0) ) });
            }
            // line: vertical bars connecting consecutive samples
            self.draw_flat.color = color;
            let mut prev = y_of(self.data[0]);
            for i in 1..n {
                let x0 = x_of(i - 1);
                let x1 = x_of(i);
                let y = y_of(self.data[i]);
                let (a, b) = if prev < y { (prev, y) } else { (y, prev) };
                self.draw_flat.draw_abs(cx, Rect { pos: dvec2(x0, a), size: dvec2((x1 - x0).max(1.0), (b - a).max(1.6)) });
                prev = y;
            }
            // threshold line, dashed
            if let Some(g) = self.goal {
                let y = y_of(g);
                self.draw_flat.color = t.cy();
                let mut x = rect.pos.x;
                while x < rect.pos.x + rect.size.x {
                    self.draw_flat.draw_abs(cx, Rect { pos: dvec2(x, y), size: dvec2(4.0, 1.2) });
                    x += 7.0;
                }
            }
            // endpoint
            let ex = x_of(n - 1) - 2.0;
            let ey = y_of(self.data[n - 1]) - 2.0;
            self.draw_flat.color = color;
            self.draw_flat.draw_abs(cx, Rect { pos: dvec2(ex, ey), size: dvec2(4.0, 4.0) });
        }
        DrawStep::done()
    }
}

impl SparkRef {
    pub fn set(&self, cx: &mut Cx, data: &[f64], goal: Option<f64>, tone: u8, light: f32) {
        if let Some(mut s) = self.borrow_mut() {
            s.data = data.to_vec();
            s.goal = goal;
            s.tone = tone;
            s.light = light;
            s.view.redraw(cx);
        }
    }
}

// ================================================================ segsbar --

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SegTone {
    Ok,
    Vio,
    Sink,
    Cy,
    Hot,
    Edge,
}

#[derive(Script, ScriptHook, Widget)]
pub struct Segsbar {
    #[deref]
    view: View,
    #[live]
    draw_flat: DrawFlat,
    #[rust]
    pub segs: Vec<(f64, SegTone)>, // (weight, tone)
    #[rust]
    pub light: f32,
}

impl Widget for Segsbar {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        self.view.handle_event(cx, event, scope);
    }
    fn draw_walk(&mut self, cx: &mut Cx2d, scope: &mut Scope, walk: Walk) -> DrawStep {
        while self.view.draw_walk(cx, scope, walk).step().is_some() {}
        let rect = self.view.area().rect(cx);
        let total: f64 = self.segs.iter().map(|s| s.0).sum();
        if total > 0.0 && rect.size.x > 2.0 {
            let t = th(self.light);
            let mut x = rect.pos.x;
            let n = self.segs.len();
            for (i, (w, tone)) in self.segs.iter().enumerate() {
                let mut ww = w / total * (rect.size.x - (n as f64 - 1.0) * 2.0);
                if ww < 0.0 {
                    ww = 0.0;
                }
                self.draw_flat.color = match tone {
                    SegTone::Ok => t.ok(),
                    SegTone::Vio => t.vio(),
                    SegTone::Sink => t.sink(),
                    SegTone::Cy => t.cy(),
                    SegTone::Hot => t.hot(),
                    SegTone::Edge => t.edge(),
                };
                self.draw_flat.draw_abs(cx, Rect { pos: dvec2(x, rect.pos.y), size: dvec2(ww, rect.size.y) });
                x += ww + if i + 1 < n { 2.0 } else { 0.0 };
            }
        }
        DrawStep::done()
    }
}

impl SegsbarRef {
    pub fn set(&self, cx: &mut Cx, segs: Vec<(f64, SegTone)>, light: f32) {
        if let Some(mut s) = self.borrow_mut() {
            s.segs = segs;
            s.light = light;
            s.view.redraw(cx);
        }
    }
}

// =================================================================== heat --

#[derive(Clone, Debug, Default)]
pub enum HeatAction {
    #[default]
    None,
    Cell(usize, usize),
}

#[derive(Script, ScriptHook, Widget)]
pub struct Heat {
    #[deref]
    view: View,
    #[live]
    draw_flat: DrawFlat,
    /// None = hidden (not yet revealed); Some(None) = measured-null.
    #[rust]
    pub cells: Vec<Vec<Option<Option<f64>>>>,
    #[rust]
    pub sel: Option<(usize, usize)>,
    #[rust]
    pub light: f32,
    #[rust]
    pub mini: bool,
}

impl Widget for Heat {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        self.view.handle_event(cx, event, scope);
        if self.mini {
            return;
        }
        if let Hit::FingerDown(fe) = event.hits(cx, self.view.area()) {
            let rect = self.view.area().rect(cx);
            let rows = self.cells.len().max(1);
            let cols = self.cells.first().map(|r| r.len()).unwrap_or(8).max(1);
            let cw = rect.size.x / cols as f64;
            let chh = rect.size.y / rows as f64;
            let ci = (((fe.abs.x - rect.pos.x) / cw) as usize).min(cols - 1);
            let ri = (((fe.abs.y - rect.pos.y) / chh) as usize).min(rows - 1);
            cx.widget_action(self.widget_uid(), HeatAction::Cell(ri, ci));
        }
    }
    fn draw_walk(&mut self, cx: &mut Cx2d, scope: &mut Scope, walk: Walk) -> DrawStep {
        while self.view.draw_walk(cx, scope, walk).step().is_some() {}
        let rect = self.view.area().rect(cx);
        let rows = self.cells.len();
        if rows == 0 {
            return DrawStep::done();
        }
        let cols = self.cells[0].len().max(1);
        let t = th(self.light);
        let gap = if self.mini { 1.0 } else { 2.0 };
        let cw = (rect.size.x - (cols as f64 - 1.0) * gap) / cols as f64;
        let chh = (rect.size.y - (rows as f64 - 1.0) * gap) / rows as f64;
        for (ri, row) in self.cells.iter().enumerate() {
            for (ci, cell) in row.iter().enumerate() {
                let x = rect.pos.x + ci as f64 * (cw + gap);
                let y = rect.pos.y + ri as f64 * (chh + gap);
                self.draw_flat.color = match cell {
                    None => t.sink(),
                    Some(None) => t.edge(),
                    Some(Some(v)) => {
                        if *v >= 0.7 {
                            t.ok()
                        } else if *v >= 0.45 {
                            t.amb()
                        } else {
                            t.hot()
                        }
                    }
                };
                self.draw_flat.draw_abs(cx, Rect { pos: dvec2(x, y), size: dvec2(cw, chh) });
                if self.sel == Some((ri, ci)) {
                    self.draw_flat.color = t.ink();
                    let b = 1.6;
                    self.draw_flat.draw_abs(cx, Rect { pos: dvec2(x, y), size: dvec2(cw, b) });
                    self.draw_flat.draw_abs(cx, Rect { pos: dvec2(x, y + chh - b), size: dvec2(cw, b) });
                    self.draw_flat.draw_abs(cx, Rect { pos: dvec2(x, y), size: dvec2(b, chh) });
                    self.draw_flat.draw_abs(cx, Rect { pos: dvec2(x + cw - b, y), size: dvec2(b, chh) });
                }
            }
        }
        DrawStep::done()
    }
}

impl HeatRef {
    /// Real sweep results: arbitrary rows × cols, all revealed.
    pub fn set_dyn(&self, cx: &mut Cx, grid: &[Vec<Option<f64>>], sel: Option<(usize, usize)>, light: f32) {
        if let Some(mut s) = self.borrow_mut() {
            s.cells = grid.iter().map(|row| row.iter().map(|v| Some(*v)).collect()).collect();
            s.sel = sel;
            s.light = light;
            s.mini = false;
            s.view.redraw(cx);
        }
    }

    pub fn set(
        &self,
        cx: &mut Cx,
        grid: Option<&[[Option<f64>; 8]; 5]>,
        revealed: usize,
        sel: Option<(usize, usize)>,
        light: f32,
        mini: bool,
    ) {
        if let Some(mut s) = self.borrow_mut() {
            s.cells = match grid {
                None => vec![],
                Some(g) => g
                    .iter()
                    .enumerate()
                    .map(|(ri, row)| {
                        row.iter()
                            .enumerate()
                            .map(|(ci, v)| {
                                let idx = ri * 8 + ci;
                                if idx < revealed {
                                    Some(*v)
                                } else {
                                    None
                                }
                            })
                            .collect()
                    })
                    .collect(),
            };
            s.sel = sel;
            s.light = light;
            s.mini = mini;
            s.view.redraw(cx);
        }
    }
}

// ================================================================== scrub --

#[derive(Clone, Debug, Default)]
pub enum ScrubAction {
    #[default]
    None,
    Seek(f64), // 0..1
}

#[derive(Script, ScriptHook, Widget)]
pub struct Scrub {
    #[deref]
    view: View,
    #[live]
    draw_flat: DrawFlat,
    #[rust]
    pub frac: f64,
    #[rust]
    pub light: f32,
}

impl Widget for Scrub {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        self.view.handle_event(cx, event, scope);
        let abs = match event.hits(cx, self.view.area()) {
            Hit::FingerDown(fe) => Some(fe.abs),
            Hit::FingerMove(fe) => Some(fe.abs),
            _ => None,
        };
        if let Some(abs) = abs {
            let rect = self.view.area().rect(cx);
            if rect.size.x > 1.0 {
                let f = ((abs.x - rect.pos.x) / rect.size.x).clamp(0.0, 1.0);
                cx.widget_action(self.widget_uid(), ScrubAction::Seek(f));
            }
        }
    }
    fn draw_walk(&mut self, cx: &mut Cx2d, scope: &mut Scope, walk: Walk) -> DrawStep {
        while self.view.draw_walk(cx, scope, walk).step().is_some() {}
        let rect = self.view.area().rect(cx);
        let t = th(self.light);
        self.draw_flat.color = t.sink();
        self.draw_flat.draw_abs(cx, rect);
        self.draw_flat.color = t.edge();
        self.draw_flat.draw_abs(cx, Rect { pos: rect.pos, size: dvec2(rect.size.x, 1.0) });
        self.draw_flat.draw_abs(cx, Rect { pos: dvec2(rect.pos.x, rect.pos.y + rect.size.y - 1.0), size: dvec2(rect.size.x, 1.0) });
        self.draw_flat.color = t.vio();
        self.draw_flat.draw_abs(cx, Rect { pos: rect.pos, size: dvec2(rect.size.x * self.frac, rect.size.y) });
        DrawStep::done()
    }
}

impl ScrubRef {
    pub fn set(&self, cx: &mut Cx, frac: f64, light: f32) {
        if let Some(mut s) = self.borrow_mut() {
            s.frac = frac;
            s.light = light;
            s.view.redraw(cx);
        }
    }
    pub fn seeked(&self, actions: &Actions) -> Option<f64> {
        if let Some(item) = actions.find_widget_action(self.widget_uid()) {
            if let ScrubAction::Seek(f) = item.cast() {
                return Some(f);
            }
        }
        None
    }
}
