//! Interface emulator. Composites the entire interface group into a single Pix2D buffer
//! using the ported software renderer:
//!
//! - **Rects, lines, gradients** → `Pix2D::fill_rect` / `draw_rect` / `line`.
//! - **Sprites** → `Pix32::plot` / `scale_plot` (palette pre-resolved from the cache sheet).
//! - **Models** → `Pix3D` + `ModelRenderer::obj_render` rasterising directly into the
//!   canvas under a sub-clipped region.
//! - **Text** → still painted as an egui overlay on top of the uploaded canvas, until
//!   the cache-loaded `PixFont` is wired (task #50). Geometry positions match what the
//!   software renderer produced.
//!
//! The whole composite is one egui texture per frame, painted as a single image. Hover
//! / click / selection use the same per-component rects in egui space.

use std::collections::HashMap;

use cache::Cache;
use cache::iftype::IfType;
use cache::model::Model;
use cache::sprite::SpriteSheet;
use eframe::egui;

use pix::pix3d::{cos_table, sin_table};
use pix::{model_light, LitModel, ModelRenderer, Pix2D, Pix3D, Pix32, sheet_to_pix32};

use crate::Selection;
use crate::pix_bridge;

const INTERFACES_ARCHIVE: u8 = 3;
const SPRITES_ARCHIVE: u8 = 8;
const MODELS_ARCHIVE: u8 = 7;

/// Standard OSRS game resolution. Most interfaces are designed to fit within this.
const GAME_W: i32 = 765;
const GAME_H: i32 = 503;

pub fn draw(ui: &mut egui::Ui, cache: &mut Cache, group_id: u32, sel: &mut Selection) {
    let files = match cache.read_files(INTERFACES_ARCHIVE, group_id) {
        Ok(Some(f)) => f,
        _ => {
            ui.label("interface group missing");
            return;
        }
    };

    let mut components: Vec<IfType> = Vec::with_capacity(files.len());
    for (fid, bytes) in &files {
        let parent_id = ((group_id as i32) << 16) | fid;
        if let Ok(c) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            IfType::decode(parent_id, *fid, bytes)
        })) {
            components.push(c);
        }
    }

    // Parent-layer offset table. Each component's (x, y) is relative to its parent
    // layer's origin; walking the `layer_id` chain gives the cumulative absolute offset
    // (matches `Client.drawLayer`'s recursive `renderx = com.x + childX`).
    let by_sub_id: HashMap<i32, &IfType> = components.iter().map(|c| (c.sub_id, c)).collect();
    let offsets: HashMap<i32, (i32, i32)> = components
        .iter()
        .map(|c| (c.sub_id, parent_offset(c, &by_sub_id)))
        .collect();

    // Pre-load every required sprite as a Pix32. Palette is resolved up-front so the
    // composite loop just blits.
    let mut sprite_cache: HashMap<i32, Option<Pix32>> = HashMap::new();
    for c in &components {
        if c.type_ == 5 && c.graphic >= 0 && !sprite_cache.contains_key(&c.graphic) {
            sprite_cache.insert(c.graphic, load_sprite_pix32(cache, c.graphic as u32));
        }
    }

    // Single master canvas. Background colour matches the dark egui panel so the texture
    // blends seamlessly into the surrounding UI.
    let mut canvas = Pix2D::new(GAME_W, GAME_H);
    canvas.fill_rect(0, 0, GAME_W, GAME_H, 0xFF_18_1A_20);
    canvas.draw_rect(0, 0, GAME_W, GAME_H, 0xFF_3C_40_48);

    let mut sorted: Vec<&IfType> = components.iter().collect();
    sorted.sort_by_key(|c| c.sub_id);

    // Bake lit models for every model component up-front (Java does this when the
    // ObjType/Player/etc. constructs its ModelLit). Standard IfType lighting params from
    // `IfType.java:1100` — `light(64, 768, -50, -10, -50)`.
    let mut lit_cache: HashMap<i32, (Model, LitModel)> = HashMap::new();
    for comp in &components {
        if comp.type_ != 6 || comp.hide || comp.model1_id < 0 {
            continue;
        }
        if lit_cache.contains_key(&comp.model1_id) {
            continue;
        }
        if let Some(model) = load_model(cache, comp.model1_id as u32) {
            let lit = model_light(&model, 64, 768, -50, -10, -50);
            lit_cache.insert(comp.model1_id, (model, lit));
        }
    }

    let mut model_renderer = ModelRenderer::new();
    let mut text_overlays: Vec<TextOverlay> = Vec::new();
    for comp in &sorted {
        if comp.hide {
            continue;
        }
        let (ox, oy) = offsets.get(&comp.sub_id).copied().unwrap_or((0, 0));
        let x = comp.x + ox;
        let y = comp.y + oy;
        let w = comp.width.max(1);
        let h = comp.height.max(1);

        // Java's drawLayer sub-clips the Pix2D to the parent layer's bbox before
        // recursing into children. Reproduce that as the intersection of every
        // ancestor-layer's rect (in canvas coords). Components without a parent layer
        // use the full canvas as their clip.
        let parent_clip_rect = parent_clip(comp, &by_sub_id, &offsets);
        let saved = canvas.save_clipping();
        canvas.set_sub_clipping(
            parent_clip_rect.0,
            parent_clip_rect.1,
            parent_clip_rect.2,
            parent_clip_rect.3,
        );
        render_component(
            &mut canvas,
            x,
            y,
            w,
            h,
            comp,
            &sprite_cache,
            &lit_cache,
            &mut model_renderer,
            &mut text_overlays,
        );
        canvas.restore_clipping(saved);
    }

    let canvas_tex = pix_bridge::upload(ui.ctx(), format!("ifx_canvas_{group_id}"), &canvas);

    section(ui, "interface", |ui| {
        egui::Grid::new("if_meta").num_columns(2).striped(true).show(ui, |ui| {
            kv(ui, "group", &group_id.to_string());
            kv(ui, "components", &components.len().to_string());
            let layer_count = components.iter().filter(|c| c.type_ == 0).count();
            kv(ui, "layers", &layer_count.to_string());
        });
    });

    section(ui, "preview", |ui| {
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(GAME_W as f32, GAME_H as f32), egui::Sense::click());
        let painter = ui.painter_at(rect);
        painter.image(
            canvas_tex.id(),
            rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );

        for t in &text_overlays {
            paint_overlay(&painter, rect, t);
        }

        let click_pos = response.clicked().then(|| response.interact_pointer_pos()).flatten();
        let hover_pos = response.hover_pos();
        let mut hovered: Option<&IfType> = None;
        let mut newly_selected: Option<i32> = None;
        for comp in &sorted {
            if comp.hide {
                continue;
            }
            let (ox, oy) = offsets.get(&comp.sub_id).copied().unwrap_or((0, 0));
            let cx = rect.min.x + (comp.x + ox) as f32;
            let cy = rect.min.y + (comp.y + oy) as f32;
            let cw = comp.width.max(1) as f32;
            let ch = comp.height.max(1) as f32;
            let c_rect = egui::Rect::from_min_size(egui::pos2(cx, cy), egui::vec2(cw, ch));
            if !rect.intersects(c_rect) {
                continue;
            }
            if let Some(p) = hover_pos {
                if c_rect.contains(p) {
                    hovered = Some(*comp);
                }
            }
            if let Some(p) = click_pos {
                if c_rect.contains(p) {
                    newly_selected = Some(comp.sub_id);
                }
            }
        }

        if let Some(fid) = sel.file_id {
            if let Some(comp) = sorted.iter().find(|c| c.sub_id == fid) {
                let (ox, oy) = offsets.get(&comp.sub_id).copied().unwrap_or((0, 0));
                let cx = rect.min.x + (comp.x + ox) as f32;
                let cy = rect.min.y + (comp.y + oy) as f32;
                let cw = comp.width.max(1) as f32;
                let ch = comp.height.max(1) as f32;
                painter.rect_stroke(
                    egui::Rect::from_min_size(egui::pos2(cx, cy), egui::vec2(cw, ch)),
                    0.0,
                    egui::Stroke::new(2.0, egui::Color32::from_rgb(255, 200, 50)),
                    egui::StrokeKind::Outside,
                );
            }
        }
        if let Some(c) = hovered {
            response.clone().on_hover_ui_at_pointer(|ui| draw_hover_tooltip(ui, c));
        }
        if let Some(sid) = newly_selected {
            sel.file_id = Some(sid);
        }
    });

    section(ui, "components", |ui| {
        egui::ScrollArea::vertical().max_height(200.0).id_salt("if_comp_list").show(ui, |ui| {
            for comp in &components {
                let label = format!(
                    "#{:>3}  {:<10} {:>4},{:<4}  {:>4}×{:<4}  layer={:<4}  {}",
                    comp.sub_id,
                    component_type_label(comp.type_),
                    comp.x,
                    comp.y,
                    comp.width,
                    comp.height,
                    comp.layer_id,
                    if comp.hide { "hide" } else { "" }
                );
                ui.label(egui::RichText::new(label).monospace().small());
            }
        });
    });
}

#[allow(clippy::too_many_arguments)]
fn render_component(
    canvas: &mut Pix2D,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    c: &IfType,
    sprites: &HashMap<i32, Option<Pix32>>,
    lit_cache: &HashMap<i32, (Model, LitModel)>,
    model_renderer: &mut ModelRenderer,
    text_overlays: &mut Vec<TextOverlay>,
) {
    match c.type_ {
        0 => {
            // Invisible layer container — children render at the layer's origin (handled
            // by parent_offset). Java's drawLayer also skips drawing here.
        }
        2 => {
            // Inventory slot grid. Java iterates `com.width × com.height` slots and blits
            // each item's lit-model sprite + stack count text per
            // `Client.java:10225-10440`. Item-sprite loading isn't wired yet; leave the
            // area as the canvas bg until it is. (No placeholder fill — placeholders
            // accumulate when stacked and read as a white-ish backdrop.)
        }
        3 => {
            // Rectangle. Filled vs outlined; per-pixel alpha if trans != 0.
            let col = i32_to_argb(c.colour);
            if c.fill {
                if c.trans == 0 {
                    canvas.fill_rect(x, y, w, h, col);
                } else {
                    let alpha = 256 - (c.trans as u32).min(255);
                    canvas.fill_rect_trans(x, y, w, h, col & 0x00FF_FFFF, alpha);
                }
            } else {
                let lw = c.line_width.max(1);
                for i in 0..lw {
                    if w - 2 * i <= 0 || h - 2 * i <= 0 {
                        break;
                    }
                    canvas.draw_rect(x + i, y + i, w - 2 * i, h - 2 * i, col);
                }
            }
        }
        4 => {
            // Text. Position into canvas-space rect; the overlay pass paints with egui's
            // font system until PixFont cache loading is wired.
            if c.text.is_empty() {
                return;
            }
            text_overlays.push(TextOverlay {
                text: c.text.clone(),
                color: i32_to_egui(c.colour, c.trans),
                rect_x: x,
                rect_y: y,
                rect_w: w,
                rect_h: h,
                h_align: c.h_align,
                v_align: c.v_align,
                shadow: c.shadow,
            });
        }
        5 => {
            // Graphic. Java picks native plot vs scale plot based on v3 + size match
            // (Client.java:10446-10451). When the sprite group fails to decode, Java just
            // doesn't draw — DON'T placeholder-fill, because stacked placeholders blend
            // into a bright wash that the user (correctly) reads as "white bg".
            if let Some(Some(sprite)) = sprites.get(&c.graphic) {
                let stretch = c.v3 && (w != sprite.owi || h != sprite.ohi);
                if stretch {
                    sprite.scale_plot(canvas, x, y, w, h);
                } else {
                    sprite.plot(canvas, x, y);
                }
            }
        }
        6 => {
            // Model. Render into the master canvas under the parent-layer clip.
            // (No per-model sub-clip — Java's drawInterface type=6 doesn't add one.)
            if c.model1_id < 0 {
                return;
            }
            let Some((model, lit)) = lit_cache.get(&c.model1_id) else {
                return;
            };
            render_model_into(canvas, x, y, w, h, model, lit, c, model_renderer);
        }
        9 => {
            canvas.line(x, y, x + w - 1, y + h - 1, i32_to_argb(c.colour));
        }
        _ => {}
    }
}

fn render_model_into(
    canvas: &mut Pix2D,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    model: &Model,
    lit: &LitModel,
    c: &IfType,
    model_renderer: &mut ModelRenderer,
) {
    // Direct port of Java `Client.drawInterface` type=6 (non-v3) path
    // (Client.java:10498-10514):
    //
    //   Pix3D.setOrigin(com.width / 2 + renderx, com.height / 2 + rendery);
    //   int var207 = com.modelZoom * Pix3D.sinTable[com.modelXAn] >> 16;
    //   int var208 = com.modelZoom * Pix3D.cosTable[com.modelXAn] >> 16;
    //   model.objRender(0, com.modelYAn, 0, com.modelXAn, 0, var207, var208);
    //
    // - `modelXAn` is the camera pitch (arg3), not local model pitch.
    // - `modelZoom * sin/cos(modelXAn)` puts the model on the tilted camera's optical
    //   axis so the model's authored origin (0,0,0) projects to the component centre.
    // - NO sub-clipping to component bounds. Models can extend past the component;
    //   only the parent layer clip (set by the caller) bounds the rasterizer.
    let var207 = (c.model_zoom * sin_table(c.model_x_an)) >> 16;
    let var208 = (c.model_zoom * cos_table(c.model_x_an)) >> 16;

    let mut p3 = Pix3D::new(canvas);
    p3.set_origin(canvas, x + w / 2, y + h / 2);
    model_renderer.obj_render(
        model,
        Some(lit),
        canvas,
        &p3,
        /*model_pitch=*/ 0,
        /*yaw=*/ c.model_y_an,
        /*roll=*/ 0,
        /*camera_pitch=*/ c.model_x_an,
        /*origin_x=*/ 0,
        /*origin_y=*/ var207,
        /*origin_z=*/ var208,
    );
}

struct TextOverlay {
    text: String,
    color: egui::Color32,
    rect_x: i32,
    rect_y: i32,
    rect_w: i32,
    rect_h: i32,
    h_align: i32,
    v_align: i32,
    shadow: bool,
}

/// Paint a single text overlay over the composited canvas. Mirrors `IfType` alignment
/// (h_align ∈ {0=left, 1=center, 2=right}, v_align same vertically). Multi-line text
/// is supported via embedded `\n` and `<br>` (parsed inline).
fn paint_overlay(painter: &egui::Painter, base: egui::Rect, t: &TextOverlay) {
    let r = egui::Rect::from_min_size(
        egui::pos2(base.min.x + t.rect_x as f32, base.min.y + t.rect_y as f32),
        egui::vec2(t.rect_w as f32, t.rect_h as f32),
    );
    let font_id = egui::FontId::proportional(12.0);
    let lines = parse_tagged_lines(&t.text, t.color, t.shadow);
    let line_height = 14.0_f32;
    let total_h = lines.len() as f32 * line_height;
    let base_y = match t.v_align {
        0 => r.top() + 2.0,
        1 => r.center().y - total_h / 2.0,
        _ => r.bottom() - total_h - 2.0,
    };
    for (li, runs) in lines.iter().enumerate() {
        let y = base_y + li as f32 * line_height;
        let widths: Vec<f32> = runs
            .iter()
            .map(|r| measure_text_width(painter, &r.text, &font_id))
            .collect();
        let total_w: f32 = widths.iter().sum();
        let line_x = match t.h_align {
            1 => r.center().x - total_w / 2.0,
            2 => r.right() - total_w - 2.0,
            _ => r.left() + 2.0,
        };
        let mut x_cursor = line_x;
        for (run, w) in runs.iter().zip(widths.iter()) {
            if run.text.is_empty() {
                continue;
            }
            if run.shadow_visible {
                painter.text(
                    egui::pos2(x_cursor + 1.0, y + 1.0),
                    egui::Align2::LEFT_TOP,
                    &run.text,
                    font_id.clone(),
                    egui::Color32::BLACK,
                );
            }
            painter.text(
                egui::pos2(x_cursor, y),
                egui::Align2::LEFT_TOP,
                &run.text,
                font_id.clone(),
                run.color,
            );
            if let Some(strike) = run.strike {
                let sy = y + 6.0;
                painter.line_segment(
                    [egui::pos2(x_cursor, sy), egui::pos2(x_cursor + w, sy)],
                    egui::Stroke::new(1.0, strike),
                );
            }
            if let Some(underline) = run.underline {
                let uy = y + 12.0;
                painter.line_segment(
                    [egui::pos2(x_cursor, uy), egui::pos2(x_cursor + w, uy)],
                    egui::Stroke::new(1.0, underline),
                );
            }
            x_cursor += w;
        }
    }
}

struct TextRun {
    text: String,
    color: egui::Color32,
    strike: Option<egui::Color32>,
    underline: Option<egui::Color32>,
    shadow_visible: bool,
}

fn parse_tagged_lines(
    src: &str,
    default_color: egui::Color32,
    default_shadow: bool,
) -> Vec<Vec<TextRun>> {
    let mut lines: Vec<Vec<TextRun>> = vec![Vec::new()];
    let mut cur_color = default_color;
    let mut cur_strike: Option<egui::Color32> = None;
    let mut cur_under: Option<egui::Color32> = None;
    let mut shadow_visible = default_shadow;
    let mut buf = String::new();

    let mut iter = src.char_indices().peekable();
    let push_run =
        |buf: &mut String, lines: &mut Vec<Vec<TextRun>>, color, strike, underline, shadow| {
            if !buf.is_empty() {
                let line = lines.last_mut().unwrap();
                line.push(TextRun {
                    text: std::mem::take(buf),
                    color,
                    strike,
                    underline,
                    shadow_visible: shadow,
                });
            }
        };

    while let Some((_, ch)) = iter.next() {
        if ch == '\n' {
            push_run(&mut buf, &mut lines, cur_color, cur_strike, cur_under, shadow_visible);
            lines.push(Vec::new());
            continue;
        }
        if ch != '<' {
            buf.push(ch);
            continue;
        }
        let mut tag = String::new();
        let mut closed = false;
        for (_, c2) in iter.by_ref() {
            if c2 == '>' {
                closed = true;
                break;
            }
            tag.push(c2);
        }
        if !closed {
            buf.push('<');
            buf.push_str(&tag);
            continue;
        }
        push_run(&mut buf, &mut lines, cur_color, cur_strike, cur_under, shadow_visible);
        match tag.as_str() {
            "br" => {
                lines.push(Vec::new());
                cur_color = default_color;
                cur_strike = None;
                cur_under = None;
                shadow_visible = default_shadow;
            }
            "/col" => cur_color = default_color,
            "u" => cur_under = Some(egui::Color32::BLACK),
            "/u" => cur_under = None,
            "str" => cur_strike = Some(egui::Color32::from_rgb(0x80, 0, 0)),
            "/str" => cur_strike = None,
            "shad" => shadow_visible = true,
            "/shad" => shadow_visible = default_shadow,
            other if other.starts_with("col=") => {
                if let Ok(rgb) = i32::from_str_radix(&other[4..], 16) {
                    cur_color = i32_to_egui(rgb, 0);
                }
            }
            other if other.starts_with("u=") => {
                if let Ok(rgb) = i32::from_str_radix(&other[2..], 16) {
                    cur_under = Some(i32_to_egui(rgb, 0));
                }
            }
            other if other.starts_with("str=") => {
                if let Ok(rgb) = i32::from_str_radix(&other[4..], 16) {
                    cur_strike = Some(i32_to_egui(rgb, 0));
                }
            }
            other if other.starts_with("shad=") => {
                shadow_visible = true;
                let _ = other;
            }
            _ => {}
        }
    }
    push_run(&mut buf, &mut lines, cur_color, cur_strike, cur_under, shadow_visible);
    lines
}

fn measure_text_width(painter: &egui::Painter, text: &str, font_id: &egui::FontId) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    painter
        .layout_no_wrap(text.to_string(), font_id.clone(), egui::Color32::WHITE)
        .size()
        .x
}

fn draw_hover_tooltip(ui: &mut egui::Ui, c: &IfType) {
    ui.label(
        egui::RichText::new(format!("#{}  {}", c.sub_id, component_type_label(c.type_)))
            .strong(),
    );
    ui.label(
        egui::RichText::new(format!("pos {},{}  size {}×{}", c.x, c.y, c.width, c.height))
            .monospace()
            .small(),
    );
    if c.layer_id >= 0 {
        ui.label(egui::RichText::new(format!("layer parent: #{}", c.layer_id)).small());
    }
    if c.type_ == 5 && c.graphic >= 0 {
        ui.label(format!("sprite #{}", c.graphic));
    }
    if c.type_ == 6 && c.model1_id >= 0 {
        ui.label(format!("model #{}  anim #{}", c.model1_id, c.model_anim));
    }
    if c.type_ == 4 && !c.text.is_empty() {
        ui.label(format!("text: {:?}", c.text));
    }
    if !c.op_names.is_empty() {
        let ops: Vec<String> = c
            .op_names
            .iter()
            .enumerate()
            .filter_map(|(i, o)| o.as_ref().map(|s| format!("{}:{s}", i + 1)))
            .collect();
        if !ops.is_empty() {
            ui.label(format!("ops: {}", ops.join(", ")));
        }
    }
    if c.hashook {
        ui.label(egui::RichText::new("has script hooks").weak());
    }
}

/// Walk the layer chain and return the intersection of every ancestor layer's bounding
/// box in canvas coords as `(min_x, min_y, max_x, max_y)`. Mirrors Java `drawLayer`'s
/// `Pix2D.setSubClipping(renderx, rendery, renderx+w, rendery+h)` accumulated across
/// nested layers. Components with no parent layer get the full canvas.
fn parent_clip(
    c: &IfType,
    by_sub_id: &HashMap<i32, &IfType>,
    offsets: &HashMap<i32, (i32, i32)>,
) -> (i32, i32, i32, i32) {
    let mut min_x = 0i32;
    let mut min_y = 0i32;
    let mut max_x = GAME_W;
    let mut max_y = GAME_H;
    let mut cur = c.layer_id;
    let mut visited: std::collections::HashSet<i32> = std::collections::HashSet::new();
    while cur >= 0 && visited.insert(cur) {
        let parent_sub = cur & 0xFFFF;
        let Some(parent) = by_sub_id.get(&parent_sub) else { break };
        let (pox, poy) = offsets.get(&parent.sub_id).copied().unwrap_or((0, 0));
        let px = parent.x + pox;
        let py = parent.y + poy;
        let pw = parent.width.max(0);
        let ph = parent.height.max(0);
        if px > min_x {
            min_x = px;
        }
        if py > min_y {
            min_y = py;
        }
        if px + pw < max_x {
            max_x = px + pw;
        }
        if py + ph < max_y {
            max_y = py + ph;
        }
        if parent.layer_id == cur {
            break;
        }
        cur = parent.layer_id;
    }
    (min_x, min_y, max_x, max_y)
}

/// Walk the `layer_id` parent chain and sum each parent layer's (x - scrollPosX,
/// y - scrollPosY). `layer_id` is stored as `(group_id << 16) | layer_sub_id`; mask off
/// the group bits before lookup.
fn parent_offset(c: &IfType, by_sub_id: &HashMap<i32, &IfType>) -> (i32, i32) {
    let mut ox = 0i32;
    let mut oy = 0i32;
    let mut cur = c.layer_id;
    let mut visited: std::collections::HashSet<i32> = std::collections::HashSet::new();
    while cur >= 0 && visited.insert(cur) {
        let parent_sub = cur & 0xFFFF;
        let Some(parent) = by_sub_id.get(&parent_sub) else { break };
        ox += parent.x - parent.scroll_pos_x;
        oy += parent.y - parent.scroll_pos_y;
        if parent.layer_id == cur {
            break;
        }
        cur = parent.layer_id;
    }
    (ox, oy)
}

fn load_model(cache: &mut Cache, group_id: u32) -> Option<Model> {
    let bytes = cache.read_group(MODELS_ARCHIVE, group_id).ok().flatten()?;
    if bytes.is_empty() {
        return None;
    }
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| Model::decode(&bytes))).ok()
}

fn load_sprite_pix32(cache: &mut Cache, group_id: u32) -> Option<Pix32> {
    let bytes = cache.read_group(SPRITES_ARCHIVE, group_id).ok().flatten()?;
    if bytes.is_empty() {
        return None;
    }
    let sheet = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        SpriteSheet::decode(&bytes)
    }))
    .ok()?;
    sheet_to_pix32(&sheet).into_iter().find(|p| p.wi > 0 && p.hi > 0)
}

/// Pack opaque `0x00RRGGBB` from an `IfType` colour into the ARGB layout the renderer
/// expects (alpha = 0xFF, full opaque).
fn i32_to_argb(rgb: i32) -> u32 {
    0xFF00_0000 | (rgb as u32 & 0x00FF_FFFF)
}

fn i32_to_egui(rgb: i32, trans: i32) -> egui::Color32 {
    let r = ((rgb >> 16) & 0xFF) as u8;
    let g = ((rgb >> 8) & 0xFF) as u8;
    let b = (rgb & 0xFF) as u8;
    let a = (255 - trans.clamp(0, 255)) as u8;
    egui::Color32::from_rgba_unmultiplied(r, g, b, a)
}

fn component_type_label(t: i32) -> &'static str {
    match t {
        0 => "layer",
        1 => "unknown1",
        2 => "inv",
        3 => "rect",
        4 => "text",
        5 => "graphic",
        6 => "model",
        7 => "invtext",
        8 => "tooltip",
        9 => "line",
        _ => "?",
    }
}

fn section(ui: &mut egui::Ui, title: &str, body: impl FnOnce(&mut egui::Ui)) {
    ui.add_space(6.0);
    ui.label(egui::RichText::new(title.to_uppercase()).small().weak());
    egui::Frame::group(ui.style())
        .corner_radius(4.0)
        .inner_margin(egui::Margin::same(10))
        .show(ui, body);
    ui.add_space(6.0);
}

fn kv(ui: &mut egui::Ui, k: &str, v: &str) {
    ui.label(egui::RichText::new(k).weak());
    ui.label(egui::RichText::new(v).monospace());
    ui.end_row();
}
