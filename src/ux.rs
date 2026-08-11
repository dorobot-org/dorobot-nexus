//! Design tokens and chrome.
//!
//! Ported from DoRobot Studio's `ui::frame` rather than depended on: the two
//! products share a visual language, not a codebase. The accent moves from blue
//! to violet, which is the only signal that you are in the simulator and not on
//! the floor.

use makepad_widgets::*;

script_mod! {
    use mod.prelude.widgets.*
    use mod.widgets.*

    // The root `mod` is sealed, so the namespace has to exist before anything
    // can be hung off it.
    mod.widgets.ux = {}

    // ---- palette -----------------------------------------------------------
    mod.widgets.ux.GROUND   = #x0E1119
    mod.widgets.ux.SURFACE  = #x161A24
    mod.widgets.ux.RAISE    = #x131822
    mod.widgets.ux.LINE     = #x252C3A
    mod.widgets.ux.INK      = #xE7EAF3
    mod.widgets.ux.INK_2    = #x98A1B8
    mod.widgets.ux.INK_3    = #x6C7591
    mod.widgets.ux.ACCENT   = #xA28BEA
    mod.widgets.ux.OK       = #x5CBE8B
    mod.widgets.ux.WARN     = #xD6A254
    mod.widgets.ux.STOP     = #xE2726B

    // ---- type --------------------------------------------------------------
    mod.widgets.ux.FONT_REGULAR = TextStyle{
        font_family: FontFamily{
            latin := FontMember{res: crate_resource("self:resources/IBMPlexSans-Text.ttf") asc: -0.1 desc: 0.0}
        }
        font_size: 11.5
        line_spacing: 1.3
    }
    mod.widgets.ux.FONT_MEDIUM = TextStyle{
        font_family: FontFamily{
            latin := FontMember{res: crate_resource("self:resources/IBMPlexSans-SemiBold.ttf") asc: -0.1 desc: 0.0}
        }
        font_size: 11.5
        line_spacing: 1.3
    }
    mod.widgets.ux.TEXT_H1    = mod.widgets.ux.FONT_MEDIUM{font_size: 19.0}
    mod.widgets.ux.TEXT_TITLE = mod.widgets.ux.FONT_MEDIUM{font_size: 13.0}
    mod.widgets.ux.TEXT_BODY  = mod.widgets.ux.FONT_REGULAR{font_size: 11.5}
    mod.widgets.ux.TEXT_META  = mod.widgets.ux.FONT_REGULAR{font_size: 10.5}
    mod.widgets.ux.TEXT_CHIP  = mod.widgets.ux.FONT_MEDIUM{font_size: 9.5}
    mod.widgets.ux.TEXT_NAV   = mod.widgets.ux.FONT_REGULAR{font_size: 10.5}
    // Numbers that must line up in columns, and headline run statistics.
    mod.widgets.ux.TEXT_NUM   = mod.widgets.ux.FONT_MEDIUM{font_size: 22.0}

    mod.widgets.ux.Filler = View{ width: Fill height: Fill }

    // ---- surfaces ----------------------------------------------------------
    mod.widgets.ux.Card = RoundedView{
        width: Fill height: Fill
        draw_bg +: {
            color: #x131822
            border_color: #x252C3A
            border_size: 1.0
            border_radius: 8.0
        }
    }

    mod.widgets.ux.PanelHead = View{
        width: Fill height: 32
        flow: Right
        align: Align{y: 0.5}
        padding: Inset{left: 12. right: 10.}
        spacing: 8.0
        show_bg: true
        draw_bg +: {
            pixel: fn() {
                // Hairline along the bottom edge only.
                let t = step(self.rect_size.y - 1.0, self.pos.y * self.rect_size.y)
                return mix(#x161A24, #x252C3A, t)
            }
        }
        title := Label{
            text: "Panel"
            draw_text +: {
                text_style: mod.widgets.ux.TEXT_CHIP{}
                get_color: fn() { return #x8B94AA }
            }
        }
        mod.widgets.ux.Filler{}
        value := Label{
            text: ""
            draw_text +: {
                text_style: mod.widgets.ux.TEXT_CHIP{}
                get_color: fn() { return #x6C7591 }
            }
        }
    }

    // Status pill. `tone`: 0 neutral, 1 ok, 2 warn, 3 stop, 4 accent.
    mod.widgets.ux.Chip = RoundedView{
        width: Fit height: Fit
        padding: Inset{left: 7. right: 7. top: 4. bottom: 4.}
        draw_bg +: {
            tone: instance(0.0)
            border_radius: 4.0
            pixel: fn() {
                let sdf = Sdf2d.viewport(self.pos * self.rect_size)
                let c0 = mix(#x222836, #x152B20, step(0.5, self.tone))
                let c1 = mix(c0, #x302510, step(1.5, self.tone))
                let c2 = mix(c1, #x341C1A, step(2.5, self.tone))
                let fill = mix(c2, #x221C3A, step(3.5, self.tone))
                sdf.box(0.0, 0.0, self.rect_size.x, self.rect_size.y, 4.0)
                sdf.fill(fill)
                return sdf.result
            }
        }
        label := Label{
            text: "chip"
            draw_text +: {
                tone: instance(0.0)
                text_style: mod.widgets.ux.TEXT_CHIP{}
                get_color: fn() {
                    let c0 = mix(#x98A1B8, #x5CBE8B, step(0.5, self.tone))
                    let c1 = mix(c0, #xD6A254, step(1.5, self.tone))
                    let c2 = mix(c1, #xE2726B, step(2.5, self.tone))
                    return mix(c2, #xA28BEA, step(3.5, self.tone))
                }
            }
        }
    }

    // ---- chrome ------------------------------------------------------------
    mod.widgets.ux.AppBar = View{
        width: Fill height: 52
        flow: Right
        align: Align{y: 0.5}
        padding: Inset{left: 16. right: 16.}
        spacing: 12.0
        show_bg: true
        draw_bg +: {
            pixel: fn() {
                let t = step(self.rect_size.y - 1.0, self.pos.y * self.rect_size.y)
                return mix(#x12151C, #x252C3A, t)
            }
        }
        mark := View{
            width: 22 height: 22
            show_bg: true
            draw_bg +: {
                pixel: fn() {
                    let sdf = Sdf2d.viewport(self.pos * self.rect_size)
                    // A cube in outline: the simulator's mark.
                    sdf.box(4.0, 5.0, 14.0, 12.0, 2.0)
                    sdf.stroke(#xA28BEA, 1.4)
                    sdf.move_to(4.0, 9.0)
                    sdf.line_to(11.0, 3.0)
                    sdf.line_to(18.0, 9.0)
                    sdf.stroke(#xA28BEA, 1.4)
                    return sdf.result
                }
            }
        }
        title := Label{
            text: "dorobot-nexus"
            draw_text +: {
                text_style: mod.widgets.ux.TEXT_TITLE{}
                get_color: fn() { return #xE7EAF3 }
            }
        }
        mod.widgets.ux.Filler{}
        device := Label{
            text: "metal · idle"
            draw_text +: {
                text_style: mod.widgets.ux.TEXT_CHIP{}
                get_color: fn() { return #x6C7591 }
            }
        }
    }

    // A nav destination. `icon` selects the glyph, `sel` the selected state.
    mod.widgets.ux.NavItem = View{
        width: Fill height: 54
        flow: Down
        align: Align{x: 0.5, y: 0.5}
        spacing: 4.0
        cursor: MouseCursor.Hand
        show_bg: true
        draw_bg +: {
            sel: instance(0.0)
            pixel: fn() {
                let sdf = Sdf2d.viewport(self.pos * self.rect_size)
                sdf.box(6.0, 3.0, self.rect_size.x - 12.0, self.rect_size.y - 6.0, 6.0)
                sdf.fill(mix(#x0E111900, #x221C3A, self.sel))
                // Selected items carry a rail on the leading edge.
                let bar = (1.0 - step(3.0, self.pos.x * self.rect_size.x)) * self.sel
                return mix(sdf.result, vec4(0.64, 0.55, 0.92, 1.0), bar)
            }
        }
        glyph := View{
            width: 20 height: 20
            show_bg: true
            draw_bg +: {
                icon: instance(0.0)
                sel: instance(0.0)
                pixel: fn() {
                    let sdf = Sdf2d.viewport(self.pos * self.rect_size)
                    let c = mix(#x6C7591, #xBCA9F5, self.sel)
                    // 0 runs, 1 scene, 2 task, 3 train, 4 inspect, 5 validate
                    let i = self.icon
                    if i < 0.5 {
                        sdf.circle(10.0, 10.0, 7.0)
                        sdf.stroke(c, 1.4)
                        sdf.move_to(8.0, 6.5) sdf.line_to(14.0, 10.0) sdf.line_to(8.0, 13.5)
                        sdf.close_path() sdf.fill(c)
                    } else {
                        if i < 1.5 {
                            sdf.box(3.0, 5.0, 14.0, 11.0, 2.0)
                            sdf.stroke(c, 1.4)
                            sdf.move_to(3.0, 9.0) sdf.line_to(10.0, 3.5) sdf.line_to(17.0, 9.0)
                            sdf.stroke(c, 1.4)
                        } else {
                            if i < 2.5 {
                                sdf.box(4.0, 3.0, 12.0, 14.0, 2.0)
                                sdf.stroke(c, 1.4)
                                sdf.move_to(7.0, 8.0) sdf.line_to(13.0, 8.0)
                                sdf.stroke(c, 1.3)
                                sdf.move_to(7.0, 12.0) sdf.line_to(13.0, 12.0)
                                sdf.stroke(c, 1.3)
                            } else {
                                if i < 3.5 {
                                    sdf.move_to(3.0, 15.0) sdf.line_to(8.0, 9.0)
                                    sdf.line_to(12.0, 12.0) sdf.line_to(17.0, 4.0)
                                    sdf.stroke(c, 1.5)
                                } else {
                                    if i < 4.5 {
                                        sdf.circle(9.0, 9.0, 5.5)
                                        sdf.stroke(c, 1.4)
                                        sdf.move_to(13.0, 13.0) sdf.line_to(17.0, 17.0)
                                        sdf.stroke(c, 1.5)
                                    } else {
                                        sdf.move_to(10.0, 3.0) sdf.line_to(16.0, 6.0)
                                        sdf.line_to(16.0, 11.0) sdf.line_to(10.0, 17.0)
                                        sdf.line_to(4.0, 11.0) sdf.line_to(4.0, 6.0)
                                        sdf.close_path()
                                        sdf.stroke(c, 1.4)
                                    }
                                }
                            }
                        }
                    }
                    return sdf.result
                }
            }
        }
        caption := Label{
            text: "Nav"
            draw_text +: {
                sel: instance(0.0)
                text_style: mod.widgets.ux.TEXT_NAV{}
                get_color: fn() { return mix(#x6C7591, #xE7EAF3, self.sel) }
            }
        }
    }

    mod.widgets.ux.NavRail = View{
        width: 88 height: Fill
        flow: Down
        padding: Inset{top: 8.}
        spacing: 2.0
        show_bg: true
        draw_bg +: {
            pixel: fn() {
                let t = step(self.rect_size.x - 1.0, self.pos.x * self.rect_size.x)
                return mix(#x12151C, #x252C3A, t)
            }
        }
        nav_runs     := mod.widgets.ux.NavItem{ caption +: {text: "Runs"}     glyph +: {draw_bg +: {icon: 0.0}} }
        nav_scene    := mod.widgets.ux.NavItem{ caption +: {text: "Scene"}    glyph +: {draw_bg +: {icon: 1.0}} }
        nav_task     := mod.widgets.ux.NavItem{ caption +: {text: "Task"}     glyph +: {draw_bg +: {icon: 2.0}} }
        nav_train    := mod.widgets.ux.NavItem{ caption +: {text: "Train"}    glyph +: {draw_bg +: {icon: 3.0}} }
        nav_inspect  := mod.widgets.ux.NavItem{ caption +: {text: "Inspect"}  glyph +: {draw_bg +: {icon: 4.0}} }
        nav_validate := mod.widgets.ux.NavItem{ caption +: {text: "Validate"} glyph +: {draw_bg +: {icon: 5.0}} }
        mod.widgets.ux.Filler{}
    }
}

/// The six destinations, in the order the operator's questions arrive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Runs,
    Scene,
    Task,
    Train,
    Inspect,
    Validate,
}

impl Screen {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Runs => "Runs",
            Self::Scene => "Scene",
            Self::Task => "Task",
            Self::Train => "Train",
            Self::Inspect => "Inspect",
            Self::Validate => "Validate",
        }
    }
}

pub const NAV_ITEMS: [(&[LiveId], Screen); 6] = [
    (ids!(nav_runs), Screen::Runs),
    (ids!(nav_scene), Screen::Scene),
    (ids!(nav_task), Screen::Task),
    (ids!(nav_train), Screen::Train),
    (ids!(nav_inspect), Screen::Inspect),
    (ids!(nav_validate), Screen::Validate),
];

/// Paint the rail's selected state.
pub fn sync_nav(cx: &mut Cx, rail: &WidgetRef, current: Screen) {
    for (path, screen) in NAV_ITEMS {
        let item = rail.widget(cx, path);
        if item.is_empty() {
            continue;
        }
        let sel = if screen == current { 1.0 } else { 0.0 };
        let mut w = item.clone();
        script_apply_eval!(cx, w, { draw_bg +: { sel: #(sel) } });
        let mut g = item.widget(cx, ids!(glyph));
        script_apply_eval!(cx, g, { draw_bg +: { sel: #(sel) } });
        let mut c = item.widget(cx, ids!(caption));
        script_apply_eval!(cx, c, { draw_text +: { sel: #(sel) } });
    }
}

/// True when a View-like widget was released under the pointer.
///
/// `find_widget_action` returns only the *first* action for a uid, and one press
/// delivers FingerDown and FingerUp in the same batch — so matching on that
/// first action never sees the release. Scan them all instead.
pub fn view_clicked(actions: &Actions, uid: WidgetUid) -> bool {
    actions.filter_widget_actions(uid).any(|a| {
        matches!(a.cast::<ViewAction>(), ViewAction::FingerUp(fe) if fe.is_over)
    })
}

/// Set a `PanelHead`'s title and right-hand value in one call.
pub fn head(cx: &mut Cx, root: &mut View, path: &[LiveId], title: &str, value: &str) {
    let h = root.widget(cx, path);
    if h.is_empty() {
        return;
    }
    h.label(cx, ids!(title)).set_text(cx, title);
    h.label(cx, ids!(value)).set_text(cx, value);
}
