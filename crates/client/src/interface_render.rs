// Port of jagex3.client.Client::drawInterface + drawLayer. Walks the
// IfType.list components for an open interface group, filtering by
// layerId so the layer-tree recursion matches Java exactly. Layers
// (type 0) recurse into same-list components whose layerId points at
// them, plus any attached sub-interface (a different group). Rect (3),
// text (4), graphic (5), and line (9) all render. Inv/model/tooltip
// remain placeholders pending those subsystems.

#![allow(dead_code)]

use std::collections::HashMap;

use crate::client::SubInterface;
use crate::config::if_type::{self, IfType};
use crate::game_shell::{Framebuffer, SHELL};
use crate::graphics::{pix2d, pix_font_generic::PixFontGeneric};

// Java drawLayer sentinel — when the active layer == 0xabcdabcd we're
// rendering the "drag children" pass for a held interface component
// (Client.java:10024). Not yet exercised by our port; preserved so the
// recursion signature matches Java when the drag path comes online.
const DRAG_LAYER_MARKER: i32 = 0xabcdabcdu32 as i32;

// custom — Entry point from Client.mainredraw at state==30 (Java's
// `TitleScreen.draw` analogue for the gameplay state). Java just calls
// `drawInterface(toplevelinterface, 0, 0, 765, 503, 0, 0, -1)` and
// `Pix2D.drawArea.draw(g, 0, 0)` directly inside mainredraw; we wrap
// both plus the position-overlay HUD into one helper since the
// surrounding Framebuffer plumbing differs.
pub fn draw_chrome(fb: &mut Framebuffer, c: &mut crate::client::Client) {
    let toplevel = c.toplevelinterface;
    // Retry the interfaces archive fetches each frame until they land.
    if toplevel >= 0 {
        let _ = if_type::open_interface(toplevel, 3);
    }
    for sub in c.subinterfaces.values() {
        if sub.id >= 0 {
            let _ = if_type::open_interface(sub.id, 3);
        }
    }
    // Java gameDraw head (Client.java:2748-2753): when the right-click
    // menu isn't open, the frame rebuilds it from scratch starting
    // with the Cancel entry; the interface walk + scene pick add the
    // rest below and sortMinimenu orders them.
    if !c.is_menu_open {
        c.menu_num_entries = 0;
        crate::client::add_menu_option(c, crate::text::CANCEL, "", 1006, 0, 0, 0);
    }

    {
        let mut pix = pix2d::STATE.lock().unwrap();
        let shell = SHELL.lock().unwrap();
        let need = (shell.s_wid * shell.s_hei) as usize;
        if pix.pixels.len() != need {
            pix.pixels = vec![0i32; need];
            pix.width = shell.s_wid as i32;
            pix.height = shell.s_hei as i32;
            pix.clip_min_x = 0;
            pix.clip_min_y = 0;
            pix.clip_max_x = pix.width;
            pix.clip_max_y = pix.height;
        }
    }
    pix2d::cls();
    pix2d::fill_rect(0, 0, 765, 503, 0x000000);

    {
        let p11 = c.p11.as_ref();
        if toplevel >= 0 {
            // Java: drawInterface(toplevelinterface, 0, 0, 765, 503, 0, 0, -1)
            draw_interface(toplevel, 0, 0, 765, 503, 0, 0, p11, &c.subinterfaces);
        }

        // Position overlay so the localPlayer state is visible even
        // though entities don't render in the scene yet.
        if let (Some(lp), Some(font)) = (c.local_player.as_ref(), p11) {
            pix2d::set_clipping(0, 0, 765, 503);
            let tile_x = c.map_build_base_x + lp.x;
            let tile_z = c.map_build_base_z + lp.z;
            let line = format!("You: ({tile_x}, {tile_z}) level {}", c.minusedlevel);
            let y = 16;
            font.base.draw_string(&line, 6, y, 0x000000, 0);
            font.base.draw_string(&line, 5, y - 1, 0xFFFF00, 0);
        }
    }

    // Java gameDrawMain tail (Client.java:4198-4200 + gameDraw 2781):
    // mouse over the 3D viewport adds the scene actions from this
    // frame's pick results, then the menu sorts game-ops-first.
    let (vx, vy, vw, vh) = {
        let o = crate::overlays::OVERLAYS.lock().unwrap();
        (o.viewport_x, o.viewport_y, o.viewport_w, o.viewport_h)
    };
    let (mouse_x, mouse_y) = {
        let m = crate::input::MOUSE.lock().unwrap();
        (m.mouse_x, m.mouse_y)
    };
    if !c.is_menu_open
        && mouse_x >= vx && mouse_x < vx + vw
        && mouse_y >= vy && mouse_y < vy + vh
    {
        crate::client::minimenu_build_scene_actions(c, vx, vy, mouse_x, mouse_y);
    }
    crate::client::sort_minimenu(c);

    // The open right-click menu draws over everything; otherwise the
    // anti-macro feedback line sits at the viewport top-left.
    pix2d::set_clipping(0, 0, 765, 503);
    if c.is_menu_open {
        crate::client::draw_minimenu(c);
    } else {
        crate::client::draw_feedback(c, vx, vy);
    }

    let pix = pix2d::STATE.lock().unwrap();
    let copy_w = pix.width.min(fb.width);
    let copy_h = pix.height.min(fb.height);
    for y in 0..copy_h {
        for x in 0..copy_w {
            let src_idx = (y * pix.width + x) as usize;
            let dst_idx = (y * fb.width + x) as usize;
            if let (Some(&p), Some(d)) = (pix.pixels.get(src_idx), fb.pixels.get_mut(dst_idx)) {
                *d = (p as u32) | 0xFF00_0000;
            }
        }
    }
}

// @ObfuscatedName("fg.fj(IIIIIIIII)V") — jag::oldscape::Client::DrawInterface
//
// x/y/w/h are clip-rect bounds (Java's setClipping(x,y,w,h) where w/h are
// the *right/bottom* edges). child_x/child_y is the origin offset for
// rendering children of the current layer.
pub fn draw_interface(
    id: i32,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    child_x: i32,
    child_y: i32,
    p11: Option<&PixFontGeneric>,
    subinterfaces: &HashMap<i32, SubInterface>,
) {
    let components: Vec<IfType> = {
        let s = if_type::STORE.lock().unwrap();
        match s.list.get(id as usize).and_then(|o| o.as_ref()) {
            Some(v) => v.iter().filter_map(|c| c.clone()).collect(),
            None => return,
        }
    };
    draw_layer(&components, -1, x, y, w, h, child_x, child_y, p11, subinterfaces);
}

// @ObfuscatedName("g.fv([Leg;IIIIIIIIB)V") — jag::oldscape::Client::DrawLayer
//
// Walks `children`, drawing every component whose `layerId == layer_id`.
// Recursion into type-0 layers passes the layer's own `parent_id` as the
// new `layer_id`, switching focus to the layer's children.
fn draw_layer(
    children: &[IfType],
    layer_id: i32,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    child_x: i32,
    child_y: i32,
    p11: Option<&PixFontGeneric>,
    subinterfaces: &HashMap<i32, SubInterface>,
) {
    pix2d::set_clipping(x, y, w, h);

    for com in children {
        // Java's filter: skip if layerId != layer_id (drag-layer branch
        // not yet wired).
        if com.layer_id != layer_id { continue; }
        if com.v3 && com.hide { continue; }

        let renderx = com.x + child_x;
        let rendery = com.y + child_y;

        // jagex3.Client.clientComponent — special-case render targets.
        // 1337 = main 3D viewport, 1338 = minimap. Both replace the
        // normal component draw entirely.
        if com.client_code == crate::scene::CLIENT_CODE_VIEWPORT {
            crate::scene::draw_viewport(renderx, rendery, com.width, com.height);
            pix2d::set_clipping(x, y, w, h);
            continue;
        }
        if com.client_code == crate::scene::CLIENT_CODE_MINIMAP {
            crate::scene::draw_minimap(renderx, rendery, com.width, com.height);
            pix2d::set_clipping(x, y, w, h);
            continue;
        }

        // Viewport for this component (intersection of (renderx..renderx+w,
        // rendery..rendery+h) with the inherited clip).
        let (var19, var20, var21, var22) = if com.type_ == 9 {
            // line — Java treats line bounds as renderx..renderx+w+1
            let mut var23 = renderx;
            let mut var24 = rendery;
            let mut var25 = renderx + com.width;
            let mut var26 = rendery + com.height;
            if var25 < renderx { std::mem::swap(&mut var23, &mut var25); }
            if var26 < rendery { std::mem::swap(&mut var24, &mut var26); }
            var25 += 1; var26 += 1;
            (var23.max(x), var24.max(y), var25.min(w), var26.min(h))
        } else {
            (
                renderx.max(x),
                rendery.max(y),
                (renderx + com.width).min(w),
                (rendery + com.height).min(h),
            )
        };

        if com.v3 && (var19 >= var21 || var20 >= var22) { continue; }

        match com.type_ {
            0 => {
                // layer — recurse into siblings with this layer as parent,
                // then into any attached sub-interface.
                draw_layer(
                    children,
                    com.parent_id,
                    var19, var20, var21, var22,
                    renderx - com.scroll_pos_x,
                    rendery - com.scroll_pos_y,
                    p11,
                    subinterfaces,
                );
                if !com.subcomponents.is_empty() {
                    let subs: Vec<IfType> = com.subcomponents.iter().filter_map(|c| c.clone()).collect();
                    draw_layer(
                        &subs,
                        com.parent_id,
                        var19, var20, var21, var22,
                        renderx - com.scroll_pos_x,
                        rendery - com.scroll_pos_y,
                        p11,
                        subinterfaces,
                    );
                }
                if let Some(sub) = subinterfaces.get(&com.parent_id) {
                    if sub.id >= 0 {
                        draw_interface(
                            sub.id,
                            var19, var20, var21, var22,
                            renderx, rendery,
                            p11, subinterfaces,
                        );
                    }
                }
                pix2d::set_clipping(x, y, w, h);
            }
            3 => {
                // rect — colour + optional alpha (trans 0..255, 0 = opaque).
                // Java's drawComponent picks colour2/colourOver via active +
                // hover state. We don't yet track per-frame hover state,
                // so we approximate: when colour2 is set and the component
                // appears "active" via a non-zero anim_frame, pick colour2.
                // The full active/hover dispatch (overCom + getIfActive)
                // lands with the input layer.
                let is_active = com.anim_frame != 0 && com.colour2 != 0;
                let col = if is_active { com.colour2 & 0xFFFFFF }
                          else { com.colour & 0xFFFFFF };
                let trans = com.trans;
                if trans == 0 {
                    if com.fill {
                        pix2d::fill_rect(renderx, rendery, com.width, com.height, col);
                    } else {
                        pix2d::draw_rect(renderx, rendery, com.width, com.height, col);
                    }
                } else {
                    let alpha = 256 - (trans & 0xFF);
                    if com.fill {
                        pix2d::fill_rect_trans(renderx, rendery, com.width, com.height, col, alpha);
                    } else {
                        pix2d::draw_rect_trans(renderx, rendery, com.width, com.height, col, alpha);
                    }
                }
            }
            1 | 4 => {
                // text — IfType.getFont depacks the per-component font
                // from the sprites + fontMetrics archives. Multi-line via
                // '|' (legacy) and '<br>' (v3). h_align: 0 left, 1 centre,
                // 2 right. v_align: 0 top, 1 centre, 2 bot.
                let font_arc = com.get_font();
                let font_ref = font_arc.as_deref().or(p11);
                if let Some(font) = font_ref {
                    let text_src = if !com.text.is_empty() { &com.text } else { &com.text2 };
                    let col = com.colour & 0xFFFFFF;
                    let line_h = if com.line_height > 0 { com.line_height } else { font.base.ascent + 2 };
                    let lines: Vec<String> = text_src
                        .replace("<br>", "|")
                        .split('|')
                        .map(|s| s.to_string())
                        .collect();
                    let total_h = (lines.len() as i32) * line_h;
                    let mut cy_top = match com.v_align {
                        1 => rendery + (com.height - total_h) / 2 + line_h,
                        2 => rendery + com.height - total_h + line_h,
                        _ => rendery + line_h,
                    };
                    for line in &lines {
                        match com.h_align {
                            1 => font.base.centre_string(line, renderx + com.width / 2, cy_top, col, 0),
                            2 => {
                                let wpx = font.base.string_wid(line);
                                font.base.draw_string(line, renderx + com.width - wpx, cy_top, col, 0);
                            }
                            _ => font.base.draw_string(line, renderx, cy_top, col, 0),
                        }
                        cy_top += line_h;
                    }
                }
            }
            5 => {
                // graphic — full v3 path: tiling × rotate × trans × scale.
                // Java's branch also supports invobject (ObjType sprite)
                // which we defer until ObjType decodes.
                if let Some(pix) = com.get_graphic(false) {
                    let width = pix.owi.max(1);
                    let height = pix.ohi.max(1);
                    if com.v3 {
                        if com.tiling {
                            pix2d::set_clipping(renderx, rendery, com.width + renderx, com.height + rendery);
                            let tiles_x = (com.width + (width - 1)) / width;
                            let tiles_z = (com.height + (height - 1)) / height;
                            for tx in 0..tiles_x {
                                for tz in 0..tiles_z {
                                    let plot_x = width * tx + renderx;
                                    let plot_y = height * tz + rendery;
                                    if com.trans != 0 {
                                        pix.trans_plot_sprite(plot_x, plot_y, 256 - (com.trans & 0xFF));
                                    } else {
                                        pix.plot_sprite(plot_x, plot_y);
                                    }
                                }
                            }
                            pix2d::set_clipping(x, y, w, h);
                        } else if com.rotate != 0 {
                            // v3 rotated graphic — `rotate` is in Java's
                            // 0..2047 brad units. Pix32.rotateTransPlotSprite
                            // takes radians; convert and centre the sprite
                            // in the component bbox.
                            let theta = (com.rotate as f64) * std::f64::consts::TAU / 2048.0;
                            let anchor_x = width / 2;
                            let anchor_y = height / 2;
                            pix.rotate_trans_plot_sprite(
                                renderx, rendery, com.width, com.height,
                                anchor_x, anchor_y,
                                theta, 256,
                            );
                        } else if com.width != width || com.height != height {
                            // Scaled — pick the translucent variant
                            // when alpha is set. Pix32.transScalePlotSprite
                            // was added in the round-12 pass.
                            if com.trans != 0 {
                                pix.trans_scale_plot_sprite(
                                    renderx, rendery, com.width, com.height,
                                    256 - (com.trans & 0xFF),
                                );
                            } else {
                                pix.scale_plot_sprite(renderx, rendery, com.width, com.height);
                            }
                        } else if com.trans != 0 {
                            pix.trans_plot_sprite(renderx, rendery, 256 - (com.trans & 0xFF));
                        } else {
                            pix.plot_sprite(renderx, rendery);
                        }
                    } else {
                        // v1 graphic — just plot.
                        pix.plot_sprite(renderx, rendery);
                    }
                } else {
                    pix2d::draw_rect(renderx, rendery, com.width.max(1), com.height.max(1), 0x303030);
                }
            }
            9 => {
                // line — Bresenham via Pix2D::line. lineWidth > 1 follows
                // Java's DrawLineWithStrokeWidth approach but with our
                // axis-stepping stroker; close enough for chrome borders.
                let col = com.colour & 0xFFFFFF;
                let x1 = renderx;
                let y1 = rendery;
                let x2 = renderx + com.width;
                let y2 = rendery + com.height;
                if com.line_width <= 1 {
                    pix2d::line(x1, y1, x2, y2, col);
                } else {
                    let stride = com.line_width.max(1);
                    let aw = com.width.abs();
                    let ah = com.height.abs();
                    if ah >= aw {
                        for i in 0..stride {
                            pix2d::line(x1 + i - stride / 2, y1, x2 + i - stride / 2, y2, col);
                        }
                    } else {
                        for i in 0..stride {
                            pix2d::line(x1, y1 + i - stride / 2, x2, y2 + i - stride / 2, col);
                        }
                    }
                }
            }
            2 => {
                // Inventory grid — 32×32 slots with marginX/marginY gap.
                // Pre-pass: blit any background sprites (Java's
                // invBackground[20] / invBackgroundX[20] / invBackgroundY[20]
                // pinned to the component's origin).
                for i in 0..com.inv_background.len() {
                    let bg_id = com.inv_background[i];
                    if bg_id <= 0 { continue; }
                    let mut bg_proxy = com.clone();
                    bg_proxy.graphic = bg_id;
                    if let Some(pix) = bg_proxy.get_graphic(false) {
                        pix.plot_sprite(
                            renderx + com.inv_background_x[i],
                            rendery + com.inv_background_y[i],
                        );
                    }
                }
                // Item sprites (Java drawLayer type-2, Client.java:
                // 10227-10268): ObjType.getSprite per filled slot with
                // the use-selected highlight variant; empty slots show
                // nothing (the background sprites above carry the
                // chrome). The drag-offset ghost render lands with the
                // hovered-slot wiring.
                let mut slot = 0usize;
                for row in 0..com.height {
                    for col in 0..com.width {
                        let mut slot_x = renderx + col * (com.margin_x + 32);
                        let mut slot_y = rendery + row * (com.margin_y + 32);
                        if slot < 20 {
                            slot_x += com.inv_background_x.get(slot).copied().unwrap_or(0);
                            slot_y += com.inv_background_y.get(slot).copied().unwrap_or(0);
                        }
                        let obj_id = com.link_obj_type.get(slot).copied().unwrap_or(0);
                        if obj_id > 0 {
                            let count = com.link_obj_number.get(slot).copied().unwrap_or(0);
                            if let Some(sprite) = crate::config::obj_type::get_sprite(
                                obj_id - 1, count, 1, 0x302020, false)
                            {
                                sprite.plot_sprite(slot_x, slot_y);
                            }
                        }
                        slot += 1;
                    }
                }
            }
            7 => {
                // invtext — overlay text in 115×12 cells. com.link_obj_type
                // / link_obj_number drive the cell contents; ObjType.list
                // resolves the item name.
                use crate::config::obj_type;
                let font_arc = com.get_font();
                let font_ref = font_arc.as_deref().or(p11);
                if let Some(font) = font_ref {
                    let mut slot = 0usize;
                    for row in 0..com.height {
                        for col in 0..com.width {
                            let cx = renderx + col * (com.margin_x + 115);
                            let cy = rendery + row * (com.margin_y + 12);
                            if slot < com.link_obj_type.len() && com.link_obj_type[slot] > 0 {
                                if let Some(obj) = obj_type::list(com.link_obj_type[slot] - 1) {
                                    let count = com.link_obj_number.get(slot).copied().unwrap_or(0);
                                    let label = if obj.stackable == 1 || count != 1 {
                                        format!("{} x{}", obj.name, count)
                                    } else {
                                        obj.name.clone()
                                    };
                                    let col_rgb = com.colour & 0xFFFFFF;
                                    match com.h_align {
                                        1 => font.base.centre_string(&label, cx + 115 / 2, cy + font.base.ascent, col_rgb, 0),
                                        2 => {
                                            let w_px = font.base.string_wid(&label);
                                            font.base.draw_string(&label, cx + 115 - w_px - 1, cy + font.base.ascent, col_rgb, 0);
                                        }
                                        _ => font.base.draw_string(&label, cx, cy + font.base.ascent, col_rgb, 0),
                                    }
                                }
                            }
                            slot += 1;
                        }
                    }
                }
            }
            8 => {
                // tooltip — Java IfType.java/Client.java:10557. The
                // tooltip surface is normally only rendered when
                // `tooltipCom == com`, which our input layer hasn't
                // wired yet. When rendered, Java:
                //   * measures every <br>-separated line with the p12
                //     font fallback,
                //   * sizes the box to (maxWidth+6) x (lineCount*lineH+5),
                //     positioned at mouseX..mouseY,
                //   * fills 0xFFFFE0 (16777120), outlines 0x000000,
                //   * draws each line in 0x000000 with 1-px shadow.
                //
                // Until tooltipCom tracking lands, we draw the box
                // in-place at the component's own (x, y) so cs2-
                // triggered tooltips (set via SET_TOOLTIP_TEXT op)
                // still appear. The box auto-sizes to text content.
                let text_src = if !com.text.is_empty() { &com.text } else { &com.text2 };
                if !text_src.is_empty() {
                    let font_arc = com.get_font();
                    let font_ref = font_arc.as_deref().or(p11);
                    if let Some(font) = font_ref {
                        let lines: Vec<String> = text_src
                            .replace("<br>", "|")
                            .split('|')
                            .map(|s| s.to_string())
                            .collect();
                        let line_h = font.base.ascent + 2;
                        let mut max_w = 0i32;
                        for line in &lines {
                            let w = font.base.string_wid(line);
                            if w > max_w { max_w = w; }
                        }
                        let box_w = (max_w + 6).max(com.width);
                        let box_h = (lines.len() as i32 * line_h + 5).max(com.height);
                        pix2d::fill_rect(renderx, rendery, box_w, box_h, 0xFFFFE0);
                        pix2d::draw_rect(renderx, rendery, box_w, box_h, 0x000000);
                        let mut y_off = rendery + line_h;
                        for line in &lines {
                            font.base.draw_string(line, renderx + 3, y_off, 0x000000, 0);
                            y_off += line_h;
                        }
                    }
                }
            }
            _ => {
                // Type 1 / 6 (unknown / model). Model rendering needs
                // Pix3D + ModelUnlit/Lit which haven't landed yet.
                pix2d::draw_rect(renderx, rendery, com.width.max(1), com.height.max(1), 0x202040);
            }
        }
    }
}

// Pure tooltip-box placement clamp. Verbatim port of
// Client.java:10583-10593. Given the tooltip's measured size and the
// component's render rect, returns the (x, y) where the box should be
// plotted such that:
//   - x prefers right-anchor (renderx + com_w - 5 - box_w),
//     clamped to >= renderx + 5 and <= w - box_w
//   - y is below the component (rendery + com_h + 5), clamped to
//     <= h - box_h
// `(w, h)` is the framebuffer extent. No globals, no Mutex.
pub fn tooltip_box_pos(
    renderx: i32, rendery: i32, com_w: i32, com_h: i32,
    box_w: i32, box_h: i32,
    fb_w: i32, fb_h: i32,
) -> (i32, i32) {
    let mut x = com_w + renderx - 5 - box_w;
    let mut y = com_h + rendery + 5;
    if x < renderx + 5 { x = renderx + 5; }
    if x + box_w > fb_w { x = fb_w - box_w; }
    if y + box_h > fb_h { y = fb_h - box_h; }
    (x, y)
}

// Pure inv-slot bbox cull. Verbatim port of the per-slot guard from
// Client.java:10241. Returns true when the 32x32 slot box overlaps
// the layer's clip rect; false when it's fully outside (skip draw).
pub fn inv_slot_visible(
    slot_x: i32, slot_y: i32,
    clip_x: i32, clip_y: i32, clip_w: i32, clip_h: i32,
) -> bool {
    slot_x + 32 > clip_x && slot_x < clip_w && slot_y + 32 > clip_y && slot_y < clip_h
}
