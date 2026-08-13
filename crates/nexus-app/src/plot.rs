//! Multi-series line plot.
//!
//! Deliberately small. DoRobot Studio's `TimeSeriesPlot` does more — cursors,
//! sliding windows, channel toggles — but it lives behind that project's
//! dataset dependencies, and a trainer only needs to draw curves against a
//! shared axis. Segments are drawn as connected vertical spans rather than
//! rotated quads, which is what dense metric data wants anyway.

use makepad_widgets::*;

script_mod! {
    use mod.prelude.widgets.*
    use mod.widgets.*

    let DrawSeg = set_type_default() do #(DrawSeg::script_shader(vm)){
        ..mod.draw.DrawQuad
        pixel: fn() { return self.color }
    }

    mod.widgets.PlotBase = #(Plot::register_widget(vm))
    mod.widgets.Plot = set_type_default() do mod.widgets.PlotBase{
        width: Fill height: Fill
        draw_seg: DrawSeg{}
        draw_grid: DrawSeg{ color: #x1B2130 }
    }
}

#[derive(Script, ScriptHook)]
#[repr(C)]
pub struct DrawSeg {
    #[deref]
    draw_super: DrawQuad,
    #[live]
    color: Vec4,
}

/// One named series and the colour it draws in.
#[derive(Clone, Debug)]
pub struct Series {
    pub name: String,
    pub color: Vec4,
    pub points: Vec<f64>,
}

/// The palette series cycle through, in order. Six distinct hues that stay
/// separable on a very dark ground.
pub const SERIES_COLORS: [Vec4; 6] = [
    Vec4 { x: 0.64, y: 0.55, z: 0.92, w: 1.0 }, // violet
    Vec4 { x: 0.36, y: 0.75, z: 0.55, w: 1.0 }, // green
    Vec4 { x: 0.84, y: 0.64, z: 0.33, w: 1.0 }, // amber
    Vec4 { x: 0.42, y: 0.63, z: 0.94, w: 1.0 }, // blue
    Vec4 { x: 0.89, y: 0.45, z: 0.42, w: 1.0 }, // red
    Vec4 { x: 0.60, y: 0.66, z: 0.78, w: 1.0 }, // grey
];

#[derive(Script, ScriptHook, Widget)]
pub struct Plot {
    #[deref]
    view: View,
    #[live]
    draw_seg: DrawSeg,
    #[live]
    draw_grid: DrawSeg,
    #[rust]
    series: Vec<Series>,
    /// Vertical range. Recomputed from the data unless pinned.
    #[rust]
    range: Option<(f64, f64)>,
    /// Line thickness in device pixels.
    #[rust(1.6f64)]
    weight: f64,
    /// Horizontal gridlines drawn behind the series.
    #[rust(3usize)]
    grid_rows: usize,
}

impl Widget for Plot {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        self.view.handle_event(cx, event, scope);
    }

    fn draw_walk(&mut self, cx: &mut Cx2d, scope: &mut Scope, walk: Walk) -> DrawStep {
        self.view.draw_walk(cx, scope, walk)?;
        let rect = self.view.area().rect(cx);
        if rect.size.x < 2.0 || rect.size.y < 2.0 {
            return DrawStep::done();
        }

        for i in 1..=self.grid_rows {
            let y = rect.pos.y + rect.size.y * (i as f64 / (self.grid_rows + 1) as f64);
            self.draw_grid.draw_abs(
                cx,
                Rect { pos: dvec2(rect.pos.x, y), size: dvec2(rect.size.x, 1.0) },
            );
        }

        let (lo, hi) = self.bounds();
        let span = (hi - lo).max(1e-9);
        // Resample to pixel columns rather than drawing one quad per sample.
        // At 120 samples across 1000px a per-sample quad is 8px wide and the
        // curve reads as a staircase; per-column spans read as a line.
        let cols = (rect.size.x as usize).clamp(2, 900);
        for s in &self.series {
            if s.points.len() < 2 {
                continue;
            }
            self.draw_seg.color = s.color;
            let last = s.points.len() - 1;
            let mut prev_y: Option<f64> = None;
            for c in 0..cols {
                let t = c as f64 / (cols - 1) as f64;
                // Linear interpolation between the two nearest samples.
                let fpos = t * last as f64;
                let i = (fpos.floor() as usize).min(last);
                let j = (i + 1).min(last);
                let frac = fpos - i as f64;
                let value = s.points[i] * (1.0 - frac) + s.points[j] * frac;

                let x = rect.pos.x + rect.size.x * t;
                let ny = ((value - lo) / span).clamp(0.0, 1.0);
                let y = rect.pos.y + rect.size.y * (1.0 - ny);
                let (top, h) = match prev_y {
                    Some(p) if (p - y).abs() > self.weight => (p.min(y), (p - y).abs()),
                    _ => (y - self.weight * 0.5, self.weight),
                };
                self.draw_seg.draw_abs(
                    cx,
                    Rect { pos: dvec2(x, top), size: dvec2(self.weight, h) },
                );
                prev_y = Some(y);
            }
        }
        DrawStep::done()
    }
}

impl Plot {
    fn bounds(&self) -> (f64, f64) {
        if let Some(r) = self.range {
            return r;
        }
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for s in &self.series {
            for v in &s.points {
                lo = lo.min(*v);
                hi = hi.max(*v);
            }
        }
        if !lo.is_finite() || !hi.is_finite() {
            return (0.0, 1.0);
        }
        // A flat series would otherwise divide by zero and draw nothing.
        if (hi - lo).abs() < 1e-9 {
            return (lo - 0.5, hi + 0.5);
        }
        let pad = (hi - lo) * 0.06;
        (lo - pad, hi + pad)
    }
}

impl PlotRef {
    pub fn set_series(&self, cx: &mut Cx, series: Vec<Series>) {
        let Some(mut inner) = self.borrow_mut() else { return };
        inner.series = series;
        inner.view.redraw(cx);
    }

    pub fn set_range(&self, cx: &mut Cx, lo: f64, hi: f64) {
        let Some(mut inner) = self.borrow_mut() else { return };
        inner.range = Some((lo, hi));
        inner.view.redraw(cx);
    }

    pub fn set_weight(&self, weight: f64) {
        if let Some(mut inner) = self.borrow_mut() {
            inner.weight = weight;
        }
    }

    pub fn set_grid_rows(&self, rows: usize) {
        if let Some(mut inner) = self.borrow_mut() {
            inner.grid_rows = rows;
        }
    }
}
