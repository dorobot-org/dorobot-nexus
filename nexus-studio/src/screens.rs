//! All eight workspaces: DSL for the stage, rails, timeline, home page,
//! composer, toasts and the modal host — plus the sync/route functions the
//! app calls. Left rails render through generic slots (`LSlot` in main.rs);
//! right rails are mode-specific sections, visibility-switched.

use crate::i18n::{tr, trf};
use crate::kit::SegTone;
use crate::state::*;
use crate::App;
use makepad_widgets::*;

script_mod! {
    use mod.prelude.widgets.*
    use mod.widgets.*
    use mod.widgets.nx.*

    mod.widgets.screens = {}

    // ------------------------------------------------------------ stage --
    mod.widgets.screens.Stage = View{
        width: Fill height: Fill
        flow: Overlay
        show_bg: true
        draw_bg +: {
            light: instance(0.0)
            pixel: fn() {
                // deep viewport with a faint floor grid + vignette
                let base = mix(#x0E0D0C, #xF6F4F1, self.light)
                let liner = mix(#x1F1D1C, #xE2DED9, self.light)
                let uv = self.pos
                let px = uv * self.rect_size
                // floor grid in the lower half, fading upward
                let gx = step(fract(px.x / 46.0), 0.028)
                let gy = step(fract(px.y / 46.0), 0.028)
                let grid = max(gx, gy) * smoothstep(0.35, 1.0, uv.y) * 0.5
                let c = mix(base, liner, grid)
                // vignette
                let d = length(uv - vec2(0.5, 0.55))
                let vig = smoothstep(0.9, 0.35, d) * 0.06 + 0.94
                let edge = mix(#x2A2725, #xD8D4CF, self.light)
                let bx = px.x
                let by = px.y
                let on_edge = max(
                    max(step(bx, 1.0), step(self.rect_size.x - 1.0, bx)),
                    max(step(by, 1.0), step(self.rect_size.y - 1.0, by)))
                // corner tick brackets, 11px arms inset 7px
                let ix = min(bx - 7.0, self.rect_size.x - 7.0 - bx)
                let iy = min(by - 7.0, self.rect_size.y - 7.0 - by)
                let armx = step(abs(iy), 1.1) * step(0.0, ix) * step(ix, 11.0)
                let army = step(abs(ix), 1.1) * step(0.0, iy) * step(iy, 11.0)
                let tick = max(armx, army)
                let vio = mix(#xEF6F2E, #xD15010, self.light)
                let c2 = mix(c * vec4(vig, vig, vig, 1.0), vio, tick * 0.85)
                return mix(c2, edge, on_edge)
            }
        }

        urdf_wrap := View{
            width: Fill height: Fill
            visible: false
            padding: Inset{left: 2. right: 2. top: 2. bottom: 2.}
            rview := RobotView{ width: Fill height: Fill }
        }

        robot := View{
            width: Fill height: Fill
            align: Align{x: 0.5 y: 0.62}
            fig := View{
                width: 170 height: 212
                show_bg: true
                draw_bg +: {
                    light: instance(0.0)
                    crouch: instance(0.0)
                    pixel: fn() {
                        let sdf = Sdf2d.viewport(self.pos * self.rect_size)
                        let vio = mix(#xEF6F2E, #xD15010, self.light)
                        let cyc = mix(#x6FA8B8, #x3E7A8C, self.light)
                        let drop = self.crouch * 14.0
                        // ghost target pose (cyan, offset up by drop)
                        sdf.circle(85.0, 56.0 - 8.0, 10.5)
                        sdf.stroke(vec4(cyc.x, cyc.y, cyc.z, 0.3), 2.2)
                        sdf.move_to(85.0, 70.0 - 8.0) sdf.line_to(85.0, 104.0 - 8.0)
                        sdf.stroke(vec4(cyc.x, cyc.y, cyc.z, 0.3), 2.2)
                        // actual body (orange)
                        sdf.circle(85.0, 42.0 + drop, 11.5)
                        sdf.fill_keep(mix(#x2A1708, #xF7E4D8, self.light))
                        sdf.stroke(vio, 3.0)
                        sdf.move_to(85.0, 56.0 + drop) sdf.line_to(85.0, 90.0 + drop)
                        sdf.stroke(vio, 3.2)
                        sdf.move_to(85.0, 64.0 + drop) sdf.line_to(63.0, 82.0 + drop)
                        sdf.stroke(vio, 3.2)
                        sdf.move_to(85.0, 64.0 + drop) sdf.line_to(107.0, 82.0 + drop)
                        sdf.stroke(vio, 3.2)
                        sdf.move_to(85.0, 90.0 + drop) sdf.line_to(68.0, 91.0 + drop)
                        sdf.stroke(vio, 3.2)
                        sdf.move_to(68.0, 91.0 + drop) sdf.line_to(61.0, 131.0 + drop * 0.5)
                        sdf.stroke(vio, 3.2)
                        sdf.move_to(61.0, 131.0 + drop * 0.5) sdf.line_to(55.0, 172.0)
                        sdf.stroke(vio, 3.2)
                        sdf.move_to(85.0, 90.0 + drop) sdf.line_to(102.0, 91.0 + drop)
                        sdf.stroke(vio, 3.2)
                        sdf.move_to(102.0, 91.0 + drop) sdf.line_to(109.0, 131.0 + drop * 0.5)
                        sdf.stroke(vio, 3.2)
                        sdf.move_to(109.0, 131.0 + drop * 0.5) sdf.line_to(115.0, 172.0)
                        sdf.stroke(vio, 3.2)
                        sdf.move_to(55.0, 172.0) sdf.line_to(43.0, 172.0)
                        sdf.stroke(vio, 4.0)
                        sdf.move_to(115.0, 172.0) sdf.line_to(127.0, 172.0)
                        sdf.stroke(vio, 4.0)
                        // joints
                        sdf.circle(85.0, 64.0 + drop, 2.6) sdf.fill(vio)
                        sdf.circle(85.0, 90.0 + drop, 3.0) sdf.fill(vio)
                        sdf.circle(61.0, 131.0 + drop * 0.5, 2.7) sdf.fill(vio)
                        sdf.circle(109.0, 131.0 + drop * 0.5, 2.7) sdf.fill(vio)
                        return sdf.result
                    }
                }
            }
        }

        glines := View{
            width: Fill height: Fill
            flow: Down
            line_t := View{
                width: Fill height: Fit
                align: Align{x: 1.0}
                padding: Inset{right: 10.}
                lbl_t := Label{
                    text: "target 0.62"
                    draw_text +: {
                        light: instance(0.0)
                        text_style: mod.widgets.nx.T_MONO_S{}
                        get_color: fn() { return mix(#x6FA8B8, #x3E7A8C, self.light) }
                    }
                }
            }
            line_a := View{
                width: Fill height: Fit
                align: Align{x: 1.0}
                padding: Inset{right: 10.}
                lbl_a := Label{
                    text: "actual 0.641"
                    draw_text +: {
                        light: instance(0.0)
                        text_style: mod.widgets.nx.T_MONO_S{}
                        get_color: fn() { return mix(#xEF6F2E, #xD15010, self.light) }
                    }
                }
            }
        }

        stage_info := View{
            width: Fill height: Fit
            align: Align{x: 0.0 y: 0.0}
            padding: Inset{left: 10. top: 8.}
            info := mod.widgets.nx.InkLbl{}
        }
        banner := View{
            width: Fill height: Fit
            visible: false
            align: Align{x: 0.5}
            padding: Inset{top: 34.}
            bl := View{
                width: Fit height: Fit
                flow: Right
                spacing: 8.0
                align: Align{y: 0.5}
                padding: Inset{left: 10. right: 8. top: 4. bottom: 4.}
                show_bg: true
                draw_bg +: {
                    light: instance(0.0)
                    pixel: fn() {
                        let sdf = Sdf2d.viewport(self.pos * self.rect_size)
                        let fill = mix(#x1B1917, #xFFFFFF, self.light)
                        let amb = mix(#xF0A330, #xB07514, self.light)
                        sdf.rect(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0)
                        sdf.fill_keep(fill)
                        sdf.stroke(amb, 1.0)
                        return sdf.result
                    }
                }
                lbl := mod.widgets.nx.BodyLbl{ width: Fit }
                clr := mod.widgets.nx.Mini{ text: "clear" }
            }
        }
    }

    // --------------------------------------------------------- timeline --
    mod.widgets.screens.TimelinePanel = mod.widgets.nx.Panel{
        height: Fit
        padding: Inset{left: 10. right: 10. top: 8. bottom: 8.}

        // transport (scenes / inspect)
        sec_frames := View{
            width: Fill height: Fit
            visible: false
            flow: Down
            spacing: 6.0
            scrub := Scrub{}
            row := View{
                width: Fill height: Fit
                flow: Right
                spacing: 4.0
                align: Align{y: 0.5}
                b_start := mod.widgets.nx.NxBtn{ text: "|◀" }
                b_back := mod.widgets.nx.NxBtn{ text: "◀" }
                b_play := mod.widgets.nx.NxBtn{ text: "▮▮" }
                b_fwd := mod.widgets.nx.NxBtn{ text: "▶" }
                b_resim := mod.widgets.nx.NxBtn{ text: "re-simulate" }
                Filler{}
                frame_lbl := mod.widgets.nx.MonoLbl{}
                b_record := mod.widgets.nx.NxBtn{ text: "● record" }
            }
        }

        // run progress (train / runs)
        sec_run := View{
            width: Fill height: Fit
            visible: false
            flow: Down
            spacing: 4.0
            segs := Segsbar{}
            ticks := mod.widgets.nx.MonoLbl{}
            tiles := View{
                width: Fill height: Fit
                visible: false
                flow: Right
                spacing: 5.0
                tile0 := mod.widgets.nx.Tile{} tile1 := mod.widgets.nx.Tile{} tile2 := mod.widgets.nx.Tile{}
                tile3 := mod.widgets.nx.Tile{} tile4 := mod.widgets.nx.Tile{} tile5 := mod.widgets.nx.Tile{}
            }
            ro_note := mod.widgets.nx.BodyLbl{ visible: false }
        }

        // sweep surface (validate)
        sec_sweep := View{
            width: Fill height: Fit
            visible: false
            flow: Down
            spacing: 6.0
            heat := Heat{ height: 150 }
            ax := mod.widgets.nx.MonoLbl{}
            chips := View{
                width: Fill height: Fit
                flow: Right
                spacing: 6.0
                align: Align{y: 0.5}
                st_chip := mod.widgets.nx.ChipV{}
                pass_chip := mod.widgets.nx.ChipV{}
                cell_chip := mod.widgets.nx.ChipV{ visible: false }
                b_cell_inspect := mod.widgets.nx.Mini{ visible: false text: "open in Inspect" }
                b_cell_scene := mod.widgets.nx.Mini{ visible: false text: "save as scene" }
                Filler{}
                grey_note := mod.widgets.nx.BodyLbl{ width: Fit }
            }
        }

        // gate ladder (deploy)
        sec_deploy := View{
            width: Fill height: Fit
            visible: false
            flow: Down
            spacing: 4.0
            gsegs := Segsbar{}
            glabels := mod.widgets.nx.MonoLbl{}
            gnote := mod.widgets.nx.BodyLbl{}
        }

        // note (robots)
        sec_note := View{
            width: Fill height: Fit
            visible: false
            nl := mod.widgets.nx.BodyLbl{}
        }
    }

    // ------------------------------------------------------- right rail --
    // One generic right-rail block: caption row, then N polymorphic lines.
    // Modes are built from stacked blocks in Rust via the RSlot system.
    let RLine = View{
        width: Fill height: Fit
        flow: Down
        visible: false
        // key/value tile row (up to 3 tiles)
        tiles := View{
            width: Fill height: Fit
            visible: false
            flow: Right
            spacing: 5.0
            t0 := mod.widgets.nx.Tile{ visible: false }
            t1 := mod.widgets.nx.Tile{ visible: false }
            t2 := mod.widgets.nx.Tile{ visible: false }
        }
        // check row
        chk := mod.widgets.nx.ChkRow{ visible: false }
        // slider row
        sld := View{
            width: Fill height: Fit
            visible: false
            flow: Right
            align: Align{y: 0.5}
            spacing: 7.0
            lb := mod.widgets.nx.BodyLbl{ width: 64 }
            sl := Slider{
                width: Fill height: 22
                text: ""
            }
            val := mod.widgets.nx.MonoLbl{}
        }
        // plain rich text
        txt := mod.widgets.nx.BodyLbl{ visible: false }
        // verdict
        ver := mod.widgets.nx.Verdict{ visible: false }
        // selectable row
        row := mod.widgets.nx.NxRow{ visible: false }
        // action strip
        btns := View{
            width: Fill height: Fit
            visible: false
            flow: Right
            spacing: 4.0
            padding: Inset{top: 2. bottom: 2.}
            b0 := mod.widgets.nx.Mini{ visible: false }
            b1 := mod.widgets.nx.Mini{ visible: false }
            b2 := mod.widgets.nx.Mini{ visible: false }
            b3 := mod.widgets.nx.MiniHot{ visible: false }
        }
        // caption
        cap := View{
            width: Fill height: Fit
            visible: false
            flow: Right
            align: Align{y: 0.5}
            padding: Inset{top: 6.}
            cl := mod.widgets.nx.Cap{}
            Filler{}
            cb := mod.widgets.nx.Mini{ visible: false }
        }
        // checkbox row
        cbx := View{
            width: Fill height: Fit
            visible: false
            flow: Right
            align: Align{y: 0.5}
            spacing: 7.0
            cbox := CheckBox{ text: "" }
            cl2 := mod.widgets.nx.InkLbl{}
        }
        // spark chart
        sparkw := View{ width: Fill height: Fit visible: false
            spark := Spark{ height: 40 }
        }
        // segsbar
        segw := View{ width: Fill height: Fit visible: false
            seg := Segsbar{}
        }
        // big hot button row
        hot := View{
            width: Fill height: Fit
            visible: false
            flow: Right
            spacing: 6.0
            h0 := mod.widgets.nx.NxBtn{ visible: false }
            h1 := mod.widgets.nx.HotBtn{ visible: false text: "E-STOP" }
            h2 := mod.widgets.nx.NxBtn{ visible: false }
        }
    }

    mod.widgets.screens.RailR = View{
        width: Fill height: Fit
        flow: Down
        padding: Inset{left: 9. right: 9. top: 6. bottom: 10.}
        spacing: 2.0
        r0 := RLine{} r1 := RLine{} r2 := RLine{} r3 := RLine{} r4 := RLine{}
        r5 := RLine{} r6 := RLine{} r7 := RLine{} r8 := RLine{} r9 := RLine{}
        r10 := RLine{} r11 := RLine{} r12 := RLine{} r13 := RLine{} r14 := RLine{}
        r15 := RLine{} r16 := RLine{} r17 := RLine{} r18 := RLine{} r19 := RLine{}
        r20 := RLine{} r21 := RLine{} r22 := RLine{} r23 := RLine{} r24 := RLine{}
        r25 := RLine{} r26 := RLine{} r27 := RLine{}
    }

    // ------------------------------------------------------------- home --
    let HCard = View{
        width: Fill height: Fit
        flow: Down
        spacing: 3.0
        padding: Inset{left: 12. right: 12. top: 9. bottom: 10.}
        show_bg: true
        draw_bg +: {
            light: instance(0.0)
            pixel: fn() {
                let sdf = Sdf2d.viewport(self.pos * self.rect_size)
                let fill = mix(#x141312, #xFBFAF8, self.light)
                let edge = mix(#x2A2725, #xD8D4CF, self.light)
                sdf.rect(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0)
                sdf.fill_keep(fill)
                sdf.stroke(edge, 1.0)
                return sdf.result
            }
        }
        head := View{
            width: Fill height: Fit
            flow: Right
            align: Align{y: 0.5}
            cap := mod.widgets.nx.Cap{}
            Filler{}
            open := mod.widgets.nx.Mini{ text: "open →" }
        }
        row0 := mod.widgets.nx.NxRow{ visible: false }
        row1 := mod.widgets.nx.NxRow{ visible: false }
        row2 := mod.widgets.nx.NxRow{ visible: false }
        row3 := mod.widgets.nx.NxRow{ visible: false }
        extra := mod.widgets.nx.BodyLbl{ visible: false }
        mini_heatw := View{ width: Fill height: Fit visible: false
            mini_heat := Heat{ height: 64 }
        }
    }

    mod.widgets.screens.HomePage = View{
        width: Fill height: Fill
        flow: Down
        spacing: 8.0
        padding: Inset{left: 10. right: 10. top: 8. bottom: 8.}
        scroll_bars: ScrollBars{show_scroll_x: false show_scroll_y: true}

        statrow := View{
            width: Fill height: Fit
            flow: Right
            spacing: 6.0
            s0 := mod.widgets.nx.Tile{} s1 := mod.widgets.nx.Tile{} s2 := mod.widgets.nx.Tile{}
            s3 := mod.widgets.nx.Tile{} s4 := mod.widgets.nx.Tile{} s5 := mod.widgets.nx.Tile{}
            s6 := mod.widgets.nx.Tile{} s7 := mod.widgets.nx.Tile{} s8 := mod.widgets.nx.Tile{}
        }

        hero := HCard{
            bits := View{
                width: Fill height: Fit
                flow: Right
                spacing: 16.0
                align: Align{y: 1.0}
                b0 := View{ width: Fit height: Fit flow: Down k := mod.widgets.nx.Cap{} v := mod.widgets.nx.MonoLbl{} }
                b1 := View{ width: Fit height: Fit flow: Down k := mod.widgets.nx.Cap{} v := Label{
                    draw_text +: {
                        light: instance(0.0)
                        text_style: mod.widgets.nx.T_BIG{}
                        get_color: fn() { return mix(#x6FA8B8, #x3E7A8C, self.light) }
                    }
                } }
                b2 := View{ width: Fit height: Fit flow: Down k := mod.widgets.nx.Cap{} v := Label{
                    draw_text +: {
                        light: instance(0.0)
                        text_style: mod.widgets.nx.T_BIG{}
                        get_color: fn() { return mix(#xEF6F2E, #xD15010, self.light) }
                    }
                } }
                b3 := View{ width: Fit height: Fit flow: Down k := mod.widgets.nx.Cap{} v := mod.widgets.nx.MonoLbl{} }
            }
            goal_chart := Spark{ height: 46 }
            goal_note := mod.widgets.nx.BodyLbl{}
            hsegs := Segsbar{ height: 7 }
            hseg_note := mod.widgets.nx.MonoLbl{}
            rew_spark := Spark{ height: 46 }
            rew_note := mod.widgets.nx.BodyLbl{}
            htiles := View{
                width: Fill height: Fit
                flow: Right
                spacing: 5.0
                h0 := mod.widgets.nx.Tile{} h1 := mod.widgets.nx.Tile{} h2 := mod.widgets.nx.Tile{}
            }
        }

        cards := View{
            width: Fill height: Fit
            flow: Right
            spacing: 8.0
            colA := View{ width: Fill height: Fit flow: Down spacing: 8.0
                card_runs := HCard{}
                card_sets := HCard{}
            }
            colB := View{ width: Fill height: Fit flow: Down spacing: 8.0
                card_robots := HCard{}
                card_val := HCard{}
            }
            colC := View{ width: Fill height: Fit flow: Down spacing: 8.0
                card_deps := HCard{}
            }
        }
    }

    // --------------------------------------------------------- composer --
    mod.widgets.screens.ComposerStrip = mod.widgets.nx.Panel{
        height: Fit
        flow: Right
        align: Align{y: 0.5}
        spacing: 6.0
        padding: Inset{left: 10. right: 10. top: 7. bottom: 7.}
        cap := mod.widgets.nx.Cap{}
        rung0 := View{ width: Fit height: Fit flow: Right spacing: 2.0 align: Align{y: 0.5} visible: false
            l := mod.widgets.nx.Mini{ text: "◀" } n := mod.widgets.nx.InkLbl{} h := mod.widgets.nx.MonoLbl{}
            r := mod.widgets.nx.Mini{ text: "▶" } x := mod.widgets.nx.MiniHot{ text: "✕" } }
        rung1 := View{ width: Fit height: Fit flow: Right spacing: 2.0 align: Align{y: 0.5} visible: false
            l := mod.widgets.nx.Mini{ text: "◀" } n := mod.widgets.nx.InkLbl{} h := mod.widgets.nx.MonoLbl{}
            r := mod.widgets.nx.Mini{ text: "▶" } x := mod.widgets.nx.MiniHot{ text: "✕" } }
        rung2 := View{ width: Fit height: Fit flow: Right spacing: 2.0 align: Align{y: 0.5} visible: false
            l := mod.widgets.nx.Mini{ text: "◀" } n := mod.widgets.nx.InkLbl{} h := mod.widgets.nx.MonoLbl{}
            r := mod.widgets.nx.Mini{ text: "▶" } x := mod.widgets.nx.MiniHot{ text: "✕" } }
        rung3 := View{ width: Fit height: Fit flow: Right spacing: 2.0 align: Align{y: 0.5} visible: false
            l := mod.widgets.nx.Mini{ text: "◀" } n := mod.widgets.nx.InkLbl{} h := mod.widgets.nx.MonoLbl{}
            r := mod.widgets.nx.Mini{ text: "▶" } x := mod.widgets.nx.MiniHot{ text: "✕" } }
        rung4 := View{ width: Fit height: Fit flow: Right spacing: 2.0 align: Align{y: 0.5} visible: false
            l := mod.widgets.nx.Mini{ text: "◀" } n := mod.widgets.nx.InkLbl{} h := mod.widgets.nx.MonoLbl{}
            r := mod.widgets.nx.Mini{ text: "▶" } x := mod.widgets.nx.MiniHot{ text: "✕" } }
        rung5 := View{ width: Fit height: Fit flow: Right spacing: 2.0 align: Align{y: 0.5} visible: false
            l := mod.widgets.nx.Mini{ text: "◀" } n := mod.widgets.nx.InkLbl{} h := mod.widgets.nx.MonoLbl{}
            r := mod.widgets.nx.Mini{ text: "▶" } x := mod.widgets.nx.MiniHot{ text: "✕" } }
        b_add := mod.widgets.nx.Mini{ text: "+ add selected" }
        Filler{}
        b_rename := mod.widgets.nx.Mini{ text: "rename" }
        b_dup := mod.widgets.nx.Mini{ text: "duplicate set" }
        budget := mod.widgets.nx.ChipV{}
        b_train := mod.widgets.nx.NxBtn{ text: "Train this set →" }
    }

    // ------------------------------------------------------------ toast --
    mod.widgets.screens.ToastV = View{
        width: Fit height: Fit
        flow: Right
        align: Align{y: 0.5}
        spacing: 8.0
        padding: Inset{left: 12. right: 10. top: 6. bottom: 6.}
        show_bg: true
        draw_bg +: {
            light: instance(0.0)
            pixel: fn() {
                let sdf = Sdf2d.viewport(self.pos * self.rect_size)
                let fill = mix(#x1B1917, #xFFFFFF, self.light)
                let vio = mix(#xEF6F2E, #xD15010, self.light)
                let edge = mix(#x2A2725, #xD8D4CF, self.light)
                sdf.rect(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0)
                sdf.fill_keep(fill)
                sdf.stroke(edge, 1.0)
                sdf.rect(0.0, 0.0, 2.0, self.rect_size.y)
                sdf.fill(vio)
                return sdf.result
            }
        }
        lbl := mod.widgets.nx.InkLbl{}
        undo := mod.widgets.nx.Mini{ visible: false text: "Undo" }
    }

    // ------------------------------------------------------------ modal --
    mod.widgets.screens.ModalHost = View{
        width: Fill height: Fill
        flow: Overlay
        align: Align{x: 0.5 y: 0.5}
        show_bg: true
        draw_bg +: {
            pixel: fn() { return vec4(0.0, 0.0, 0.0, 0.55) }
        }
        panel := View{
            width: 560 height: Fit
            flow: Down
            spacing: 8.0
            padding: Inset{left: 16. right: 16. top: 14. bottom: 14.}
            show_bg: true
            draw_bg +: {
                light: instance(0.0)
                pixel: fn() {
                    let sdf = Sdf2d.viewport(self.pos * self.rect_size)
                    let fill = mix(#x141312, #xFBFAF8, self.light)
                    let edge = mix(#x2A2725, #xD8D4CF, self.light)
                    let vio = mix(#xEF6F2E, #xD15010, self.light)
                    sdf.rect(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0)
                    sdf.fill_keep(fill)
                    sdf.stroke(edge, 1.0)
                    sdf.rect(0.0, 0.0, self.rect_size.x, 2.0)
                    sdf.fill(vio)
                    return sdf.result
                }
            }
            title := Label{
                text: ""
                draw_text +: {
                    light: instance(0.0)
                    text_style: mod.widgets.nx.T_TITLE{}
                    get_color: fn() { return mix(#xF2F0EC, #x161413, self.light) }
                }
            }
            sub := mod.widgets.nx.BodyLbl{ visible: false }
            blast := mod.widgets.nx.Verdict{ visible: false }
            wsteps := mod.widgets.nx.MonoLbl{ visible: false }
            // Each field is wrapped: TextInput carries no `visible` property in
            // this makepad rev, so `set_visible` on it is a silent no-op and the
            // field would draw in every modal — including the ones that ask for
            // no input at all. The wrapper is a View, which does honour it.
            // `empty_text` is blanked because makepad's default is the stock
            // "Your text here", and the caption above already names the field.
            in_cap0 := mod.widgets.nx.Cap{ visible: false }
            in_wrap0 := View{
                width: Fill height: Fit visible: false
                input0 := TextInput{ width: Fill text: "" empty_text: "" }
            }
            in_cap1 := mod.widgets.nx.Cap{ visible: false }
            in_wrap1 := View{
                width: Fill height: Fit visible: false
                input1 := TextInput{ width: Fill text: "" empty_text: "" }
            }
            in_cap2 := mod.widgets.nx.Cap{ visible: false }
            in_wrap2 := View{
                width: Fill height: Fit visible: false
                input2 := TextInput{ width: Fill text: "" empty_text: "" }
            }
            opt0 := mod.widgets.nx.NxRow{ visible: false }
            opt1 := mod.widgets.nx.NxRow{ visible: false }
            opt2 := mod.widgets.nx.NxRow{ visible: false }
            opt3 := mod.widgets.nx.NxRow{ visible: false }
            opt4 := mod.widgets.nx.NxRow{ visible: false }
            opt5 := mod.widgets.nx.NxRow{ visible: false }
            opt6 := mod.widgets.nx.NxRow{ visible: false }
            opt7 := mod.widgets.nx.NxRow{ visible: false }
            chk0 := mod.widgets.nx.ChkRow{ visible: false }
            chk1 := mod.widgets.nx.ChkRow{ visible: false }
            chk2 := mod.widgets.nx.ChkRow{ visible: false }
            chk3 := mod.widgets.nx.ChkRow{ visible: false }
            chk4 := mod.widgets.nx.ChkRow{ visible: false }
            info := mod.widgets.nx.BodyLbl{ visible: false }
            foot := View{
                width: Fill height: Fit
                flow: Right
                align: Align{x: 1.0}
                spacing: 6.0
                b_back := mod.widgets.nx.Mini{ visible: false text: "Back" }
                b_cancel := mod.widgets.nx.NxBtn{ text: "Cancel" }
                b_ok := mod.widgets.nx.NxBtn{ visible: false }
                b_danger := mod.widgets.nx.MiniHot{ visible: false }
            }
        }
    }
}

// ==========================================================================
// Rust glue
// ==========================================================================

/// True when a View-like widget was released under the pointer.
pub fn view_clicked(actions: &Actions, uid: WidgetUid) -> bool {
    actions.filter_widget_actions(uid).any(|a| {
        matches!(a.cast::<ViewAction>(), ViewAction::FingerUp(fe) if fe.is_over)
    })
}

fn t(lang: usize, k: &str) -> String {
    tr(lang, k).to_string()
}

fn caps(s: &str) -> String {
    s.to_uppercase()
}

pub fn set_chip(cx: &mut Cx, root: &WidgetRef, path: &[LiveId], text: &str, dot_tone: Option<f64>, _pulse: bool, l: f32) {
    let chip = root.widget(cx, path);
    if chip.is_empty() {
        return;
    }
    let mut c = chip.clone();
    script_apply_eval!(cx, c, { draw_bg +: { light: #(l as f64) } });
    let lbl = chip.label(cx, ids!(lbl));
    if !lbl.is_empty() {
        lbl.set_text(cx, text);
        let mut w = chip.widget(cx, ids!(lbl));
        script_apply_eval!(cx, w, { draw_text +: { light: #(l as f64) } });
    }
    let dot = chip.view(cx, ids!(dot));
    if !dot.is_empty() {
        dot.set_visible(cx, dot_tone.is_some());
        if let Some(tone) = dot_tone {
            let mut d = chip.widget(cx, ids!(dot));
            script_apply_eval!(cx, d, { draw_bg +: { tone: #(tone) } });
        }
    }
}

fn set_pill(cx: &mut Cx, holder: &WidgetRef, text: &str, tone: f64, l: f32) {
    let pill = holder.widget(cx, ids!(pill));
    if pill.is_empty() {
        return;
    }
    pill.set_visible(cx, true);
    let mut p = pill.clone();
    script_apply_eval!(cx, p, { draw_bg +: { tone: #(tone) light: #(l as f64) } });
    let lbl = pill.label(cx, ids!(lbl));
    lbl.set_text(cx, text);
    let mut w = pill.widget(cx, ids!(lbl));
    script_apply_eval!(cx, w, { draw_text +: { tone: #(tone) light: #(l as f64) } });
}

fn set_label(cx: &mut Cx, holder: &WidgetRef, path: &[LiveId], text: &str, l: f32) {
    let lb = holder.label(cx, path);
    if lb.is_empty() {
        return;
    }
    lb.set_text(cx, text);
    let mut w = holder.widget(cx, path);
    script_apply_eval!(cx, w, { draw_text +: { light: #(l as f64) } });
}

fn set_btn(cx: &mut Cx, holder: &WidgetRef, path: &[LiveId], text: &str, vis: bool, on: bool, enabled: bool, l: f32) {
    let b = holder.button(cx, path);
    if b.is_empty() {
        return;
    }
    b.set_visible(cx, vis);
    if !vis {
        return;
    }
    b.set_text(cx, text);
    b.set_enabled(cx, enabled);
    let onf = if on { 1.0 } else { 0.0 };
    let mut w = holder.widget(cx, path);
    script_apply_eval!(cx, w, {
        draw_bg +: { on: #(onf) light: #(l as f64) }
        draw_text +: { on: #(onf) light: #(l as f64) }
    });
}

// -------------------------------------------------------------- left rail --

/// What a click on a left slot (row or its action minis) means.
#[derive(Clone, Debug, PartialEq)]
pub enum LAct {
    None,
    SelScene(String),
    SelRobot(String),
    SelRun(String),
    PickCk(String, String),
    DepPickCk(String, String),
    SelTarget(String),
    Replay(String),
    SelRecipe(usize),
}

#[derive(Clone, Debug, Default)]
pub struct LSpec {
    pub grp: Option<String>,
    pub row: Option<(String, String, Option<(String, f64)>, bool, bool)>, // name, tag, pill, on, dimmed
    pub acts: Vec<(String, bool)>, // label, hot
    pub note: Option<String>,
    pub click: LAct,
    pub act_ids: Vec<&'static str>, // symbolic action names for minis
}

impl Default for LAct {
    fn default() -> Self {
        LAct::None
    }
}

fn lslot_paths() -> [&'static [LiveId]; N_LSLOTS_PUB] {
    [
        ids!(lslot0), ids!(lslot1), ids!(lslot2), ids!(lslot3), ids!(lslot4), ids!(lslot5),
        ids!(lslot6), ids!(lslot7), ids!(lslot8), ids!(lslot9), ids!(lslot10), ids!(lslot11),
        ids!(lslot12), ids!(lslot13), ids!(lslot14), ids!(lslot15), ids!(lslot16), ids!(lslot17),
        ids!(lslot18), ids!(lslot19), ids!(lslot20), ids!(lslot21),
    ]
}
pub const N_LSLOTS_PUB: usize = 22;

fn build_left_specs(app: &App) -> (String, Option<(String, String)>, Vec<LSpec>) {
    let st = &app.st;
    let lang = st.lang;
    let mut specs: Vec<LSpec> = vec![];
    let tt = |k: &str| t(lang, k);
    let tf = |k: &str, a: &[&str]| trf(lang, k, a);
    match st.mode {
        Mode::Scenes => {
            let head = format!("{} · {}", caps(&tt("Library")), st.scenes.len());
            for set in &st.sets {
                specs.push(LSpec { grp: Some(format!("{} · {}", caps(&tt("Set")), set.name)), ..Default::default() });
                for id in &set.members {
                    match st.scene(id) {
                        Some(sc) => {
                            let on = st.sel.scene.as_deref() == Some(id.as_str());
                            let terrain = if sc.terrain.is_empty() { tt("flat") } else { tt(&sc.terrain) };
                            let mut sp = LSpec {
                                row: Some((sc.name.clone(), format!("{} · {:.2}", terrain, sc.stance), None, on, false)),
                                click: LAct::SelScene(id.clone()),
                                ..Default::default()
                            };
                            if on {
                                sp.acts = vec![(tt("Duplicate"), false), (tt("Add to set ▾"), false), (tt("Delete"), true)];
                                sp.act_ids = vec!["dup-scene", "addto-set", "del-scene"];
                            }
                            specs.push(sp);
                        }
                        None => specs.push(LSpec {
                            row: Some((tt("deleted"), tt("Remove slot"), None, false, true)),
                            ..Default::default()
                        }),
                    }
                }
            }
            let unfiled: Vec<&Scene> = st
                .scenes
                .iter()
                .filter(|s| st.sets_of(&s.id).is_empty() && (st.filter.is_empty() || s.name.contains(&st.filter)))
                .collect();
            specs.push(LSpec { grp: Some(caps(&tt("Unfiled"))), ..Default::default() });
            for sc in unfiled {
                let on = st.sel.scene.as_deref() == Some(sc.id.as_str());
                let terrain = if sc.terrain.is_empty() { tt("flat") } else { tt(&sc.terrain) };
                let mut sp = LSpec {
                    row: Some((sc.name.clone(), format!("{} · {:.2}", terrain, sc.stance), None, on, false)),
                    click: LAct::SelScene(sc.id.clone()),
                    ..Default::default()
                };
                if on {
                    sp.acts = vec![(tt("Duplicate"), false), (tt("Add to set ▾"), false), (tt("Delete"), true)];
                    sp.act_ids = vec!["dup-scene", "addto-set", "del-scene"];
                }
                specs.push(sp);
            }
            specs.push(LSpec { grp: Some(caps(&tt("Recordings"))), ..Default::default() });
            for r in &st.recordings {
                specs.push(LSpec {
                    row: Some((r.name.clone(), tf("{0} fr · {1} m", &[&r.frames.to_string(), &format!("{:.1}", r.dist)]), None, false, false)),
                    click: LAct::Replay(r.id.clone()),
                    acts: vec![(tt("Delete"), true)],
                    act_ids: vec!["del-rec"],
                    ..Default::default()
                });
            }
            (head, Some(("new-scene".to_string(), tt("+ New"))), specs)
        }
        Mode::Robots => {
            let head = format!("{} · {}", caps(&tt("Robots")), st.robots.len());
            for r in &st.robots {
                let on = st.sel.robot.as_deref() == Some(r.id.as_str());
                specs.push(LSpec {
                    row: Some((r.name.clone(), tf("{0} dof", &[&r.movable.to_string()]), None, on, false)),
                    click: LAct::SelRobot(r.id.clone()),
                    ..Default::default()
                });
            }
            specs.push(LSpec { grp: Some(caps(&tt("Lifecycle"))), ..Default::default() });
            specs.push(LSpec {
                note: Some(tt("Import runs a six-step gate: parse, assets, structure, rehearse, map, commit. A URDF that loads is not a URDF that trains.")),
                ..Default::default()
            });
            (head, Some(("import-robot".to_string(), tt("+ Import"))), specs)
        }
        Mode::Train => {
            let head = caps(&tt("Stages"));
            if let Some(r) = st.sel.run.as_ref().and_then(|id| st.run(id)) {
                for (i, sname) in r.stages.iter().enumerate() {
                    let mark = if i < r.stage { "✓ " } else { "" };
                    specs.push(LSpec {
                        row: Some((format!("{mark}{sname}"), String::new(), None, i == r.stage, i > r.stage)),
                        ..Default::default()
                    });
                }
                specs.push(LSpec { grp: Some(caps(&tt("Round"))), ..Default::default() });
                specs.push(LSpec { note: Some(format!("{} {} / 6 · {} {} / {}", tt("round"), r.round, tt("iter"), r.iter, r.iters_per)), ..Default::default() });
                specs.push(LSpec { grp: Some(caps(&tt("Controls"))), ..Default::default() });
                let mut sp = LSpec::default();
                match r.state {
                    RunState::Running => {
                        sp.acts = vec![(tt("▮▮ pause"), false), (tt("■ stop"), false)];
                        sp.act_ids = vec!["pause", "stop"];
                        if crate::nexus::bin_exists() && app.real_proc.is_none() {
                            sp.acts.push((tt("▶ real train"), false));
                            sp.act_ids.push("real-train");
                        }
                    }
                    RunState::Paused => {
                        sp.acts = vec![(tt("▶ resume"), false), (tt("■ stop"), false)];
                        sp.act_ids = vec!["resume", "stop"];
                    }
                    _ => {
                        sp.note = Some(tt(r.state.key()));
                    }
                }
                specs.push(sp);
            }
            (head, None, specs)
        }
        Mode::Inspect => {
            let head = caps(&tt("Checkpoint"));
            for r in &st.runs {
                for c in &r.ckpts {
                    let on = st.sel.ck.as_deref().unwrap_or("ck-2100k") == c.label
                        && st.sel.ck_run.as_deref().map(|x| x == r.name).unwrap_or(true);
                    let star = if c.st == CkSt::Prom { " · ★" } else { "" };
                    specs.push(LSpec {
                        row: Some((c.label.clone(), format!("{}{star}", r.name), None, on, false)),
                        click: LAct::PickCk(r.name.clone(), c.label.clone()),
                        ..Default::default()
                    });
                }
            }
            specs.push(LSpec { grp: Some(caps(&tt("Probe"))), ..Default::default() });
            let mut sp = LSpec::default();
            sp.acts = vec![(tt("push →"), false), (tt("restart"), false)];
            sp.act_ids = vec!["push", "restart-probe"];
            specs.push(sp);
            specs.push(LSpec {
                note: Some(tt("Probes are deliberately ephemeral — a probe is a question, not an artifact. record promotes one to a Recording.")),
                ..Default::default()
            });
            (head, None, specs)
        }
        Mode::Validate => {
            let head = caps(&tt("Recipe"));
            for (i, rc) in RECIPES.iter().enumerate() {
                specs.push(LSpec {
                    row: Some((tt(rc.name), String::new(), None, st.sel.recipe == i, false)),
                    click: LAct::SelRecipe(i),
                    ..Default::default()
                });
            }
            specs.push(LSpec { grp: Some(caps(&tt("Axes"))), ..Default::default() });
            let rc = &RECIPES[st.sel.recipe];
            specs.push(LSpec { note: Some(format!("{} · {} — {}", tt(rc.xl), rc.xa, rc.xb)), ..Default::default() });
            specs.push(LSpec { note: Some(format!("{} · {} — {}", tt(rc.yl), rc.ya, rc.yb)), ..Default::default() });
            specs.push(LSpec { grp: Some(caps(&tt("Base scene"))), ..Default::default() });
            specs.push(LSpec { row: Some(("target-60".into(), String::new(), None, true, false)), ..Default::default() });
            specs.push(LSpec { grp: Some(caps(&tt("Checkpoint"))), ..Default::default() });
            specs.push(LSpec { row: Some((st.sweep_ckpt.clone(), String::new(), None, true, false)), ..Default::default() });
            let mut sp = LSpec::default();
            match st.sweep_state {
                SweepState::Running => {
                    sp.acts = vec![(tt("■ stop"), false)];
                    sp.act_ids = vec!["sweep-stop"];
                }
                _ => {
                    sp.acts = vec![(tt("▶ run sweep"), false)];
                    sp.act_ids = vec!["sweep-run"];
                    if crate::nexus::bin_exists() && app.real_proc.is_none() {
                        sp.acts.push((tt("▶ real sweep"), false));
                        sp.act_ids.push("real-sweep");
                    }
                    if st.sweep_grid.is_some() {
                        sp.acts.push((tt("delete result"), true));
                        sp.act_ids.push("sweep-del");
                    }
                }
            }
            specs.push(sp);
            (head, None, specs)
        }
        Mode::Runs => {
            let head = caps(&trf(lang, "Ledger · {0} runs", &[&st.runs.len().to_string()]));
            for r in &st.runs {
                let on = st.sel.run.as_deref() == Some(r.id.as_str());
                let name = if r.id == "kept" { tt("(kept checkpoints)") } else { r.name.clone() };
                let (ptxt, ptone) = if r.archived {
                    (tt("archived"), 4.0)
                } else {
                    (tt(r.state.key()), match r.state {
                        RunState::Running => 0.0,
                        RunState::Completed => 1.0,
                        RunState::Paused | RunState::Stopped => 2.0,
                        RunState::Failed => 3.0,
                    })
                };
                let best = r.best.map(|b| format!("{b:.3}")).unwrap_or_else(|| "—".into());
                specs.push(LSpec {
                    row: Some((name, format!("{:.2}M · {} {}", r.steps, tt("best"), best), Some((ptxt, ptone)), on, r.archived)),
                    click: LAct::SelRun(r.id.clone()),
                    ..Default::default()
                });
            }
            (head, None, specs)
        }
        Mode::Deploy => {
            let head = caps(&tt("Policy"));
            let mut any = false;
            for r in &st.runs {
                for c in r.ckpts.iter().filter(|c| c.st == CkSt::Prom) {
                    any = true;
                    let on = st.dg.ck.as_deref() == Some(c.label.as_str()) && st.dg.ck_run.as_deref() == Some(r.name.as_str());
                    specs.push(LSpec {
                        row: Some((
                            format!("★ {}", c.pname.clone().unwrap_or_else(|| c.label.clone())),
                            format!("{} · {}", c.label, r.name),
                            None,
                            on,
                            false,
                        )),
                        click: LAct::DepPickCk(r.name.clone(), c.label.clone()),
                        ..Default::default()
                    });
                }
            }
            if !any {
                specs.push(LSpec { note: Some(tt("Only promoted checkpoints deploy — promote one in Train first.")), ..Default::default() });
            }
            let nonprom: usize = st.runs.iter().map(|r| r.ckpts.iter().filter(|c| c.st != CkSt::Prom).count()).sum();
            if nonprom > 0 {
                specs.push(LSpec { note: Some(tf("{0} unpromoted checkpoints are not eligible", &[&nonprom.to_string()])), ..Default::default() });
            }
            specs.push(LSpec { grp: Some(format!("{} · {}", caps(&tt("Targets")), st.targets.len())), ..Default::default() });
            for tg in &st.targets {
                let on = st.dg.target.as_deref() == Some(tg.id.as_str());
                let (ptxt, ptone) = match tg.ping {
                    Ping::Ok(ms) => (tf("{0} ms", &[&ms.to_string()]), 1.0),
                    Ping::Bad => (tt("unreachable"), 3.0),
                    Ping::Probing => ("…".into(), 2.0),
                    Ping::Unprobed => (tt("unprobed"), 4.0),
                };
                let mut sp = LSpec {
                    row: Some((tg.name.clone(), format!("{} · {}", tg.model, tg.iface), Some((ptxt, ptone)), on, false)),
                    click: LAct::SelTarget(tg.id.clone()),
                    ..Default::default()
                };
                if on {
                    sp.acts = vec![(tt("ping"), false), (tt("edit"), false), (tt("Delete"), true)];
                    sp.act_ids = vec!["target-ping", "target-edit", "target-del"];
                }
                specs.push(sp);
            }
            specs.push(LSpec { grp: Some(caps(&tt("Lifecycle"))), ..Default::default() });
            specs.push(LSpec {
                note: Some(tt("A deployment certifies one (policy, target) pair — change either and the gates reset.")),
                ..Default::default()
            });
            (head, Some(("target-new".to_string(), tt("+ Add"))), specs)
        }
        Mode::Home => (String::new(), None, specs),
    }
}

pub fn sync_left_rail(app: &mut App, cx: &mut Cx) {
    let l = app.light();
    let (head, head_btn, specs) = build_left_specs(app);
    {
        let railw0 = app.ui.widget(cx, ids!(rail_l));
        let fw = railw0.view(cx, ids!(filter_wrap));
        if !fw.is_empty() {
            fw.set_visible(cx, app.st.mode == Mode::Scenes);
        }
    }
    app.lmap = specs.clone();
    let railw = app.ui.widget(cx, ids!(rail_l));
    if railw.is_empty() {
        return;
    }
    {
        let mut p = app.ui.view(cx, ids!(rail_l));
        script_apply_eval!(cx, p, { draw_bg +: { light: #(l as f64) } });
    }
    set_label(cx, &railw, ids!(cap_head.cap), &head, l);
    app.lhead_act = head_btn.as_ref().map(|(a, _)| a.clone());
    match &head_btn {
        Some((_, label)) => set_btn(cx, &railw, ids!(cap_head.cap_btn), label, true, false, true, l),
        None => {
            let b = railw.button(cx, ids!(cap_head.cap_btn));
            if !b.is_empty() {
                b.set_visible(cx, false);
            }
        }
    }
    for (i, path) in lslot_paths().iter().enumerate() {
        let slot = railw.widget(cx, path);
        if slot.is_empty() {
            continue;
        }
        let Some(spec) = specs.get(i) else {
            slot.set_visible(cx, false);
            continue;
        };
        slot.set_visible(cx, true);
        // group
        let grp = slot.view(cx, ids!(grp));
        grp.set_visible(cx, spec.grp.is_some());
        if let Some(g) = &spec.grp {
            set_label(cx, &slot, ids!(grp.lbl), g, l);
            let mut gv = slot.widget(cx, ids!(grp));
            script_apply_eval!(cx, gv, { draw_bg +: { light: #(l as f64) } });
        }
        // row
        let rowv = slot.view(cx, ids!(row));
        rowv.set_visible(cx, spec.row.is_some());
        if let Some((nm, tg, pill, on, dim)) = &spec.row {
            set_label(cx, &slot, ids!(row.nm), nm, l);
            set_label(cx, &slot, ids!(row.tg), tg, l);
            let onf = if *on { 1.0 } else { 0.0 };
            let dimf = if *dim { 1.0 } else { 0.0 };
            let mut rv = slot.widget(cx, ids!(row));
            script_apply_eval!(cx, rv, { draw_bg +: { light: #(l as f64) on: #(onf) dimmed: #(dimf) } });
            let mut nmw = slot.widget(cx, ids!(row.nm));
            script_apply_eval!(cx, nmw, { draw_text +: { dimmed: #(dimf) } });
            let rowref = slot.widget(cx, ids!(row));
            match pill {
                Some((ptxt, ptone)) => set_pill(cx, &rowref, ptxt, *ptone, l),
                None => {
                    let pw = rowref.widget(cx, ids!(pill));
                    if !pw.is_empty() {
                        pw.set_visible(cx, false);
                    }
                }
            }
        }
        // acts
        let acts = slot.view(cx, ids!(acts));
        acts.set_visible(cx, !spec.acts.is_empty());
        let apaths: [&[LiveId]; 3] = [ids!(acts.a0), ids!(acts.a1), ids!(acts.a2)];
        for (j, ap) in apaths.iter().enumerate() {
            match spec.acts.get(j) {
                Some((label, hot)) => {
                    // hot actions land on a2 (MiniHot); place non-hot on a0/a1
                    let _ = hot;
                    set_btn(cx, &slot, ap, label, true, false, true, l);
                }
                None => {
                    let b = slot.button(cx, ap);
                    if !b.is_empty() {
                        b.set_visible(cx, false);
                    }
                }
            }
        }
        // note
        let note = slot.view(cx, ids!(note));
        note.set_visible(cx, spec.note.is_some());
        if let Some(n) = &spec.note {
            set_label(cx, &slot, ids!(note.lbl), n, l);
        }
    }
}

pub fn route_left_rail(app: &mut App, cx: &mut Cx, actions: &Actions) {
    let railw = app.ui.widget(cx, ids!(rail_l));
    if railw.is_empty() {
        return;
    }
    {
        let fi = railw.text_input(cx, ids!(filter_wrap.filter_in));
        if !fi.is_empty() {
            if let Some(v) = fi.changed(actions) {
                app.st.filter = v;
                sync_left_rail(app, cx);
                app.redraw_ui(cx);
                return;
            }
        }
    }
    if let Some(act) = app.lhead_act.clone() {
        if railw.button(cx, ids!(cap_head.cap_btn)).clicked(actions) {
            app.dispatch(cx, &act, 0);
            return;
        }
    }
    let specs = app.lmap.clone();
    for (i, path) in lslot_paths().iter().enumerate() {
        let Some(spec) = specs.get(i) else { continue };
        let slot = railw.widget(cx, path);
        if slot.is_empty() {
            continue;
        }
        if spec.row.is_some() {
            let uid = slot.widget(cx, ids!(row)).widget_uid();
            if view_clicked(actions, uid) {
                app.dispatch_l(cx, &spec.click);
                return;
            }
        }
        let apaths: [&[LiveId]; 3] = [ids!(acts.a0), ids!(acts.a1), ids!(acts.a2)];
        for (j, ap) in apaths.iter().enumerate() {
            if j < spec.act_ids.len() && slot.button(cx, ap).clicked(actions) {
                let act = spec.act_ids[j].to_string();
                // recording rows carry their own id through click target
                let arg = match &spec.click {
                    LAct::Replay(id) => id.clone(),
                    _ => String::new(),
                };
                app.dispatch_named(cx, &act, &arg);
                return;
            }
        }
    }
}

// The remaining sync/route surfaces live in sibling modules for size.
mod home_sync;
mod modal_sync;
mod rail_r;
mod stage_tl;

pub use home_sync::{route_home, sync_home};
pub use modal_sync::{modal_back, modal_danger, modal_ok, modal_opt, route_modal, sync_modal};
pub use rail_r::{route_right_rail, sync_right_rail, RSpec};
pub use stage_tl::{route_composer, route_stage_tl, route_toasts, sync_mode_frame, sync_stage_tl, sync_sweep_fast, sync_toasts, sync_transport_fast};
