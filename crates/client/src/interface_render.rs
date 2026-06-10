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

    // overCom/tooltipCom are maintained per tick by the
    // interface_loop pass (Java's loopInterface), exactly like Java —
    // the draw below just reads c.over_com_id / c.tooltip_com_id.

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
        // The interface walk needs &mut Client for the hover menu
        // options while also reading fonts/subinterfaces — clone the
        // cheap bits out first (the font clone is glyph-vec copies;
        // acceptable per frame until fonts move behind Arc).
        let p11_owned = c.p11.clone();
        let subs_owned = c.subinterfaces.clone();
        if toplevel >= 0 {
            // Java: drawInterface(toplevelinterface, 0, 0, 765, 503, 0, 0, -1)
            let mut client_opt: Option<&mut crate::client::Client> = Some(c);
            draw_interface(toplevel, 0, 0, 765, 503, 0, 0,
                           p11_owned.as_ref(), &subs_owned, &mut client_opt);
        }

        // Position overlay so the localPlayer state is visible even
        // though entities don't render in the scene yet.
        if let (Some(lp), Some(font)) = (c.local_player.as_ref(), p11_owned.as_ref()) {
            pix2d::set_clipping(0, 0, 765, 503);
            // lp.x/z are fine coords (Java convention); >> 7 = tile.
            let tile_x = c.map_build_base_x + (lp.x >> 7);
            let tile_z = c.map_build_base_z + (lp.z >> 7);
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

    // Java gameDraw :2799 — a completed draw resets the tick counter
    // the per-tick-unit motion scaled by.
    c.world_update_num = 0;

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

// @ObfuscatedName(— Client.drawScrollbar). Verbatim port of
// Client.java:10754-10779: 16px track between two arrow caps, the
// proportional grip (min 8px) with the bevel highlight/lowlight edges.
fn draw_scrollbar(y: i32, x: i32, scroll_pos: i32, height: i32, scroll_height: i32) {
    const TRACK: i32 = 2301979;
    const GRIP: i32 = 5063219;
    const LOWLIGHT: i32 = 3353893;
    const HIGHLIGHT: i32 = 7759444;
    {
        let o = crate::overlays::OVERLAYS.lock().unwrap();
        if let Some(caps) = o.scrollbar.as_ref() {
            if let Some(top) = caps.first() {
                top.plot_sprite(x, y);
            }
            if let Some(bottom) = caps.get(1) {
                bottom.plot_sprite(x, y + height - 16);
            }
        }
    }
    pix2d::fill_rect(x, y + 16, 16, height - 32, TRACK);
    let mut grip = (height - 32) * height / scroll_height.max(1);
    if grip < 8 {
        grip = 8;
    }
    let off = (height - 32 - grip) * scroll_pos / (scroll_height - height).max(1);
    pix2d::fill_rect(x, y + 16 + off, 16, grip, GRIP);
    // Bevel: light top/left, dark bottom/right (vline = 1-wide rect,
    // hline = 1-tall rect; both clip).
    pix2d::fill_rect(x, y + 16 + off, 1, grip, HIGHLIGHT);
    pix2d::fill_rect(x + 1, y + 16 + off, 1, grip, HIGHLIGHT);
    pix2d::fill_rect(x, y + 16 + off, 16, 1, HIGHLIGHT);
    pix2d::fill_rect(x, y + 17 + off, 16, 1, HIGHLIGHT);
    pix2d::fill_rect(x + 15, y + 16 + off, 1, grip, LOWLIGHT);
    pix2d::fill_rect(x + 14, y + 17 + off, 1, grip - 1, LOWLIGHT);
    pix2d::fill_rect(x, y + 15 + off + grip, 16, 1, LOWLIGHT);
    pix2d::fill_rect(x + 1, y + 14 + off + grip, 15, 1, LOWLIGHT);
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
    client: &mut Option<&mut crate::client::Client>,
) {
    let components: Vec<IfType> = {
        let s = if_type::STORE.lock().unwrap();
        match s.list.get(id as usize).and_then(|o| o.as_ref()) {
            Some(v) => v.iter().filter_map(|c| c.clone()).collect(),
            None => return,
        }
    };
    // Java drawInterface 10020-10026: a dragged IF3 component is
    // deferred out of its layer and re-drawn last (over everything)
    // with the 0xabcdabcd sentinel unlocking it in drawLayer's
    // filter.
    *DRAG_CHILDREN.lock().unwrap() = None;
    draw_layer(&components, -1, x, y, w, h, child_x, child_y, p11, subinterfaces, client);
    let deferred = DRAG_CHILDREN.lock().unwrap().take();
    if let Some((drag_children, drag_x, drag_y)) = deferred {
        draw_layer(&drag_children, DRAG_LAYER_ID, x, y, w, h, drag_x, drag_y,
                   p11, subinterfaces, client);
    }
}

// Java's dragChildren/dragChildX/dragChildY statics + the layerid
// sentinel that re-admits only the dragged component.
pub const DRAG_LAYER_ID: i32 = 0xabcdabcd_u32 as i32;
static DRAG_CHILDREN: std::sync::Mutex<Option<(Vec<IfType>, i32, i32)>> =
    std::sync::Mutex::new(None);

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
    client: &mut Option<&mut crate::client::Client>,
) {
    pix2d::set_clipping(x, y, w, h);

    for com in children {
        // Java drawLayer 10044: skip components outside this layer —
        // unless this is the deferred drag pass and the component is
        // the dragged one.
        if com.layer_id != layer_id {
            let drag_pass = layer_id == DRAG_LAYER_ID
                && client.as_deref().map_or(false, |c| {
                    c.drag_com != crate::script_runner::ComRef::None
                        && c.drag_com == crate::interface_loop::com_ref_of(com)
                });
            if !drag_pass { continue; }
        }
        if com.v3 && com.hide { continue; }

        // Java drawLayer 10066-10068 — clientComponent per-frame
        // mutation (character-design gender buttons + spinning
        // preview avatars). Applied to a frame-local copy; the store
        // copy stays pristine like Java's statics-driven rewrite.
        let mut cc_owned: Option<IfType> = None;
        if com.client_code > 0 {
            if let Some(c) = client.as_deref_mut() {
                let mut copy = com.clone();
                crate::client::client_component(c, &mut copy);
                cc_owned = Some(copy);
            }
        }
        let com: &IfType = cc_owned.as_ref().unwrap_or(com);

        let mut renderx = com.x + child_x;
        let mut rendery = com.y + child_y;

        // Java drawLayer 10074-10112 — the dragged IF3 component:
        // non-draggablebehavior drags defer to the end-of-interface
        // pass (so they draw over everything) and render half-trans
        // at the clamped mouse position.
        let mut drag_owned: Option<IfType> = None;
        if let Some(c) = client.as_deref() {
            use crate::script_runner::ComRef;
            if c.drag_com != ComRef::None
                && c.drag_com == crate::interface_loop::com_ref_of(com)
            {
                if layer_id != DRAG_LAYER_ID && !com.draggable_behavior {
                    *DRAG_CHILDREN.lock().unwrap() =
                        Some((children.to_vec(), child_x, child_y));
                    continue;
                }

                if c.drag_alive && c.drag_parent_found {
                    if let Some(drag_layer) = c.drag_layer.resolve() {
                        let (mouse_x, mouse_y) = {
                            let m = crate::input::MOUSE.lock().unwrap();
                            (m.mouse_x, m.mouse_y)
                        };
                        let mut dx = mouse_x - c.drag_pickup_x;
                        let mut dy = mouse_y - c.drag_pickup_y;
                        if dx < c.drag_parent_x {
                            dx = c.drag_parent_x;
                        }
                        if com.width + dx > c.drag_parent_x + drag_layer.width {
                            dx = c.drag_parent_x + drag_layer.width - com.width;
                        }
                        if dy < c.drag_parent_y {
                            dy = c.drag_parent_y;
                        }
                        if com.height + dy > c.drag_parent_y + drag_layer.height {
                            dy = c.drag_parent_y + drag_layer.height - com.height;
                        }
                        renderx = dx;
                        rendery = dy;
                    }
                }

                if !com.draggable_behavior {
                    let mut clone = com.clone();
                    clone.trans = 128;
                    drag_owned = Some(clone);
                }
            }
        }
        let com: &IfType = drag_owned.as_ref().unwrap_or(com);

        // jagex3.Client.clientComponent — special-case render targets.
        // 1337 = main 3D viewport, 1338 = minimap. Both replace the
        // normal component draw entirely.
        if com.client_code == crate::scene::CLIENT_CODE_VIEWPORT {
            // Java gameDrawMain 4096-4101 — players/NPCs/projectiles/
            // spotanims enter the sprite grid before renderAll.
            if let Some(c) = client.as_deref_mut() {
                crate::scene::push_entities(c);
            }
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

        // Java drawLayer 10173-10177: hovering a component adds its
        // menu options (and the type-2 hovered-slot tracking) to the
        // right-click menu being rebuilt this frame.
        if let Some(c) = client.as_deref_mut() {
            let (mouse_x, mouse_y) = {
                let m = crate::input::MOUSE.lock().unwrap();
                (m.mouse_x, m.mouse_y)
            };
            if !c.is_menu_open
                && mouse_x >= var19 && mouse_y >= var20
                && mouse_x < var21 && mouse_y < var22
            {
                crate::client::add_component_options(c, com,
                                                     mouse_x - renderx,
                                                     mouse_y - rendery);
            }

            // v1 hover (overCom/tooltipCom) and scrollbar input moved
            // to interface_loop's per-tick pass — Java handles them in
            // loopInterface, not drawLayer.
        }

        match com.type_ {
            0 => {
                // Java drawLayer 10181: hidden v1 layers stay skipped
                // unless they're the current overCom — that's how
                // hover-revealed submenus (overLayerId) work.
                if !com.v3 && com.hide {
                    let over = client.as_deref().map_or(false, |c| {
                        c.over_com_id != -1 && c.over_com_id == com.parent_id
                    });
                    if !over {
                        continue;
                    }
                }

                // Java drawLayer 10185-10192: clamp the v1 scroll
                // offset into [0, scrollHeight - height]. The store
                // copy is updated too so the scrollbar input pass
                // sees the clamped value next tick.
                let mut scroll_pos_y = com.scroll_pos_y;
                if !com.v3 {
                    if scroll_pos_y > com.scroll_height - com.height {
                        scroll_pos_y = com.scroll_height - com.height;
                    }
                    if scroll_pos_y < 0 {
                        scroll_pos_y = 0;
                    }
                    if scroll_pos_y != com.scroll_pos_y {
                        if_type::modify(com.parent_id,
                                        |t| t.scroll_pos_y = scroll_pos_y);
                    }
                }

                // layer — recurse into siblings with this layer as parent,
                // then into any attached sub-interface.
                draw_layer(
                    children,
                    com.parent_id,
                    var19, var20, var21, var22,
                    renderx - com.scroll_pos_x,
                    rendery - scroll_pos_y,
                    p11,
                    subinterfaces,
                    client,
                );
                if !com.subcomponents.is_empty() {
                    let subs: Vec<IfType> = com.subcomponents.iter().filter_map(|c| c.clone()).collect();
                    draw_layer(
                        &subs,
                        com.parent_id,
                        var19, var20, var21, var22,
                        renderx - com.scroll_pos_x,
                        rendery - scroll_pos_y,
                        p11,
                        subinterfaces,
                        client,
                    );
                }
                if let Some(sub) = subinterfaces.get(&com.parent_id) {
                    // Java drawLayer 10202-10208 — hovering a modal
                    // (type 0) subinterface resets the menu being
                    // built to just Cancel, so options accumulated
                    // from the interfaces underneath don't leak into
                    // the right-click menu.
                    if sub.type_ == 0 {
                        if let Some(c) = client.as_deref_mut() {
                            let (mx, my) = {
                                let m = crate::input::MOUSE.lock().unwrap();
                                (m.mouse_x, m.mouse_y)
                            };
                            if mx >= var19 && my >= var20
                                && mx < var21 && my < var22
                                && !c.is_menu_open
                            {
                                c.menu_verb[0] = crate::text::CANCEL.to_string();
                                c.menu_subject[0] = String::new();
                                c.menu_action[0] = 1006;
                                c.menu_num_entries = 1;
                            }
                        }
                    }
                    if sub.id >= 0 {
                        draw_interface(
                            sub.id,
                            var19, var20, var21, var22,
                            renderx, rendery,
                            p11, subinterfaces,
                            client,
                        );
                    }
                }
                pix2d::set_clipping(x, y, w, h);
                // Java drawLayer 10221-10223: legacy scrolling layers
                // get the 16px scrollbar on their right edge.
                if !com.v3 && com.scroll_height > com.height {
                    draw_scrollbar(rendery, com.width + renderx,
                                   scroll_pos_y, com.height, com.scroll_height);
                }
            }
            3 => {
                // rect — Java drawInterface 10321-10346: colour comes
                // from the comparator scripts (getIfActive → colour2)
                // with the overCom hover override on whichever side
                // was picked.
                let (active, over) = match client.as_deref() {
                    Some(c) => (crate::client::get_if_active(c, com),
                                c.over_com_id != -1 && c.over_com_id == com.parent_id),
                    None => (false, false),
                };
                let col = if active {
                    let mut col = com.colour2;
                    if over && com.colour2_over != 0 { col = com.colour2_over; }
                    col & 0xFFFFFF
                } else {
                    let mut col = com.colour;
                    if over && com.colour_over != 0 { col = com.colour_over; }
                    col & 0xFFFFFF
                };
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
            1 => {
                // Java drawInterface 10224: type 1 has an empty branch
                // — nothing is drawn.
            }
            4 => {
                // text — Java drawInterface 10347-10396: active state
                // swaps in colour2/text2, overCom applies the hover
                // colour, v3 invobject components display the item
                // name (+ "x <count>" for stacks), the resume-pause
                // component shows "Please wait...", and v1 text runs
                // the %1-%5/%dns variable substitution.
                let font_arc = com.get_font();
                let font_ref = font_arc.as_deref().or(p11);
                if let Some(font) = font_ref {
                    let mut text = com.text.clone();
                    let (active, over, resume) = match client.as_deref() {
                        Some(c) => (crate::client::get_if_active(c, com),
                                    c.over_com_id != -1 && c.over_com_id == com.parent_id,
                                    c.resume_pause_com == com.parent_id),
                        None => (false, false, false),
                    };
                    let mut colour;
                    if active {
                        colour = com.colour2;
                        if over && com.colour2_over != 0 {
                            colour = com.colour2_over;
                        }
                        if !com.text2.is_empty() {
                            text = com.text2.clone();
                        }
                    } else {
                        colour = com.colour;
                        if over && com.colour_over != 0 {
                            colour = com.colour_over;
                        }
                    }

                    if com.v3 && com.invobject != -1 {
                        let obj = crate::config::obj_type::list(com.invobject);
                        // Java: text = obj.name, "null" when unset.
                        text = obj.as_ref()
                            .map(|o| o.name.clone())
                            .filter(|n| !n.is_empty())
                            .unwrap_or_else(|| "null".to_string());
                        if let Some(obj) = obj.as_ref() {
                            if (obj.stackable == 1 || com.invcount != 1) && com.invcount != -1 {
                                text = format!(
                                    "{}{}{} x{}",
                                    crate::string_constants::tag_colour(0xff9040),
                                    text,
                                    crate::string_constants::TAG_COLOURCLOSE,
                                    crate::client::nice_number(com.invcount),
                                );
                            }
                        }
                    }

                    if resume {
                        text = crate::text::PLEASEWAIT.to_string();
                        colour = com.colour;
                    }

                    if !com.v3 {
                        if let Some(c) = client.as_deref() {
                            text = crate::client::substitute_vars(c, &text, com);
                        }
                    }

                    font.base.draw_string_multiline(
                        &text, renderx, rendery, com.width, com.height,
                        colour & 0xFFFFFF, if com.shadow { 0 } else { -1 },
                        com.h_align, com.v_align, com.line_height,
                    );
                }
            }
            5 => {
                // graphic — Java drawInterface 10397-10452. v3 sources
                // the image from invobject (ObjType.getSprite) or the
                // inactive graphic and applies tiling × rotate × trans
                // × scale; v1 plots getGraphic(getIfActive) directly.
                let image = if com.v3 {
                    if com.invobject == -1 {
                        com.get_graphic(false)
                    } else {
                        crate::config::obj_type::get_sprite(
                            com.invobject, com.invcount,
                            com.outline, com.shadow_colour, false)
                    }
                } else {
                    let active = client.as_deref()
                        .map_or(false, |c| crate::client::get_if_active(c, com));
                    com.get_graphic(active)
                };
                if let Some(pix) = image {
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
                // 10227-10268): ObjType.getSprite per filled slot —
                // the use-selected white-outline variant, the dragged
                // item rendered half-translucent at the mouse offset
                // (5px dead zone, 5-cycle delay), the selection flash,
                // or the plain blit.
                let drag_state = client.as_deref().map(|c| {
                    (c.use_mode, c.obj_selected_slot, c.obj_selected_com_id,
                     c.obj_drag_com, c.obj_drag_slot, c.obj_grab_x, c.obj_grab_y,
                     c.obj_drag_cycles, c.selected_com, c.selected_item)
                });
                let (mouse_x, mouse_y) = {
                    let m = crate::input::MOUSE.lock().unwrap();
                    (m.mouse_x, m.mouse_y)
                };
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
                            let (use_sel, dragged, sel_flash, drag_dx, drag_dy) =
                                match drag_state {
                                    Some((use_mode, sel_slot, sel_com,
                                          drag_com, drag_slot, grab_x, grab_y,
                                          drag_cycles, selected_com, selected_item)) => {
                                        let use_sel = use_mode == 1
                                            && sel_slot == slot as i32
                                            && sel_com == com.parent_id;
                                        let dragged = drag_com == com.parent_id
                                            && drag_slot == slot as i32;
                                        let sel_flash = selected_com == com.parent_id
                                            && selected_item == slot as i32;
                                        let mut dx = mouse_x - grab_x;
                                        let mut dy = mouse_y - grab_y;
                                        if dx < 5 && dx > -5 { dx = 0; }
                                        if dy < 5 && dy > -5 { dy = 0; }
                                        if drag_cycles < 5 { dx = 0; dy = 0; }
                                        (use_sel, dragged, sel_flash, dx, dy)
                                    }
                                    None => (false, false, false, 0, 0),
                                };
                            let sprite = if use_sel {
                                crate::config::obj_type::get_sprite(
                                    obj_id - 1, count, 2, 0, false)
                            } else {
                                crate::config::obj_type::get_sprite(
                                    obj_id - 1, count, 1, 0x302020, false)
                            };
                            if let Some(sprite) = sprite {
                                if dragged {
                                    sprite.trans_plot_sprite(slot_x + drag_dx,
                                                             slot_y + drag_dy, 128);

                                    // Java drawLayer 10270-10301:
                                    // dragging an item against the
                                    // top/bottom clip edge of the
                                    // scrolling parent layer
                                    // autoscrolls it (rate capped at
                                    // 10px/tick, clamped to the
                                    // remaining scroll range) and
                                    // shifts the grab anchor so the
                                    // item stays under the cursor.
                                    if layer_id != -1 {
                                        if let Some(parent) =
                                            children.get((layer_id & 0xFFFF) as usize)
                                        {
                                            let (clip_min_y, clip_max_y) = {
                                                let p = pix2d::STATE.lock().unwrap();
                                                (p.clip_min_y, p.clip_max_y)
                                            };
                                            let wun = client.as_deref()
                                                .map_or(1, |c| c.world_update_num);
                                            if slot_y + drag_dy < clip_min_y
                                                && parent.scroll_pos_y > 0
                                            {
                                                let mut auto = wun
                                                    * (clip_min_y - slot_y - drag_dy) / 3;
                                                if auto > wun * 10 {
                                                    auto = wun * 10;
                                                }
                                                if auto > parent.scroll_pos_y {
                                                    auto = parent.scroll_pos_y;
                                                }
                                                if_type::modify(parent.parent_id,
                                                    |t| t.scroll_pos_y -= auto);
                                                if let Some(c) = client.as_deref_mut() {
                                                    c.obj_grab_y += auto;
                                                }
                                            }
                                            if slot_y + drag_dy + 32 > clip_max_y
                                                && parent.scroll_pos_y
                                                    < parent.scroll_height - parent.height
                                            {
                                                let mut auto = wun
                                                    * (slot_y + drag_dy + 32 - clip_max_y) / 3;
                                                if auto > wun * 10 {
                                                    auto = wun * 10;
                                                }
                                                let room = parent.scroll_height
                                                    - parent.height
                                                    - parent.scroll_pos_y;
                                                if auto > room {
                                                    auto = room;
                                                }
                                                if_type::modify(parent.parent_id,
                                                    |t| t.scroll_pos_y += auto);
                                                if let Some(c) = client.as_deref_mut() {
                                                    c.obj_grab_y -= auto;
                                                }
                                            }
                                        }
                                    }
                                } else if sel_flash {
                                    sprite.trans_plot_sprite(slot_x, slot_y, 128);
                                } else {
                                    sprite.plot_sprite(slot_x, slot_y);
                                }
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
                // tooltip — Java drawInterface 10557-10613: only drawn
                // for the hovered tooltipCom once the dwell counter has
                // saturated (tooltipNum == tooltipRedraw). The box is
                // measured with the p12 font over <br>-separated lines
                // of the var-substituted text, sized (maxWidth+6) ×
                // (lines*(ascent+1)+7), right-anchored under the
                // component and clamped to the clip rect, filled
                // 0xFFFFE0 with a black outline and black 13px lines.
                let (gate, text) = match client.as_deref() {
                    Some(c) => (
                        c.tooltip_com_id != -1
                            && c.tooltip_com_id == com.parent_id
                            && c.tooltip_num == c.tooltip_redraw,
                        crate::client::substitute_vars(c, &com.text, com),
                    ),
                    None => (false, com.text.clone()),
                };
                let p12_owned = client.as_deref().and_then(|c| c.p12.clone());
                let font_ref = p12_owned.as_ref().or(p11);
                if gate && !text.is_empty() {
                    if let Some(font) = font_ref {
                        let line_h = font.base.ascent + 1;
                        let lines: Vec<&str> =
                            text.split(crate::string_constants::TAG_BREAK).collect();
                        let mut box_w = 0i32;
                        for line in &lines {
                            let wid = font.base.string_wid(line);
                            if wid > box_w { box_w = wid; }
                        }
                        box_w += 6;
                        let box_h = lines.len() as i32 * line_h + 7;
                        let (bx, by) = tooltip_box_pos(
                            renderx, rendery, com.width, com.height,
                            box_w, box_h, w, h);
                        pix2d::fill_rect(bx, by, box_w, box_h, 16777120);
                        pix2d::draw_rect(bx, by, box_w, box_h, 0);
                        let mut ty = by + font.base.ascent + 2;
                        for line in &lines {
                            font.base.draw_string(line, bx + 3, ty, 0, -1);
                            ty += line_h;
                        }
                    }
                }
            }
            6 => {
                // model — Java drawInterface 10453-10517: resolve the
                // base model (invobject item model, or the component's
                // own type-6 model via getTempModel with the active
                // variant + optional seq animation), centre the Pix3D
                // origin in the component, then objRender with the
                // model orbit derived from modelZoom/modelXAn. The
                // model1Type == 5 player-avatar branch (idkDesign /
                // localPlayer.getTempModel) needs the PlayerModel
                // appearance port and renders nothing until it lands,
                // matching Java's null-model fall-through.
                let active = client.as_deref()
                    .map_or(false, |c| crate::client::get_if_active(c, com));
                let anim = if active { com.model_anim2 } else { com.model_anim };

                let mut model = None;
                let mut min_y_off = 0i32;
                if com.invobject != -1 {
                    if let Some(obj) = crate::config::obj_type::list(com.invobject) {
                        let counted = obj.get_stack_size_alt(com.invcount).unwrap_or(obj);
                        model = counted.get_model_lit(1);
                        if let Some(m) = model.as_ref() {
                            // bounds pre-computed by get_model_lit
                            min_y_off = m.min_y / 2;
                        }
                    }
                } else if com.model1_type == 5 {
                    // Java 10479-10484 — the character-design preview:
                    // model1Id 0 spins the idkDesign composition, 1 the
                    // local player's live avatar.
                    if let Some(c) = client.as_deref_mut() {
                        crate::client::ensure_idk_design_init(c);
                        model = if com.model1_id == 0 {
                            c.idk_design.get_temp_model(None, -1, None, -1)
                                .map(std::sync::Arc::new)
                        } else {
                            let lc = c.loop_cycle;
                            c.local_player.as_mut()
                                .and_then(|lp| lp.get_temp_model(lc))
                                .map(std::sync::Arc::new)
                        };
                    }
                } else {
                    let player_model = client.as_deref()
                        .and_then(|c| c.local_player.as_ref())
                        .map(|lp| lp.model.clone());
                    if anim == -1 {
                        model = com.get_temp_model(None, -1, active, player_model.as_ref());
                    } else {
                        let seq = crate::config::seq_type::list(anim);
                        model = com.get_temp_model(Some(&seq), com.anim_frame, active,
                                                   player_model.as_ref());
                    }
                }

                crate::dash3d::pix3d::set_origin(com.width / 2 + renderx,
                                                 com.height / 2 + rendery);
                let sin_t = crate::dash3d::pix3d::sin_table();
                let cos_t = crate::dash3d::pix3d::cos_table();
                let eye_y = com.model_zoom * sin_t[(com.model_x_an & 0x7FF) as usize] >> 16;
                let eye_z = com.model_zoom * cos_t[(com.model_x_an & 0x7FF) as usize] >> 16;

                if let Some(model) = model {
                    if com.v3 {
                        if com.orthog {
                            model.obj_render_icon_orthog(
                                0, com.model_y_an, com.model_z_an, com.model_x_an,
                                com.model_x_of,
                                com.model_y_of + min_y_off + eye_y,
                                com.model_y_of + eye_z,
                                com.model_zoom);
                        } else {
                            model.obj_render_icon(
                                0, com.model_y_an, com.model_z_an, com.model_x_an,
                                com.model_x_of,
                                com.model_y_of + min_y_off + eye_y,
                                com.model_y_of + eye_z);
                        }
                    } else {
                        model.obj_render_icon(
                            0, com.model_y_an, 0, com.model_x_an,
                            0, eye_y, eye_z);
                    }
                }
                crate::dash3d::pix3d::reset_origin();
            }
            _ => {}
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
