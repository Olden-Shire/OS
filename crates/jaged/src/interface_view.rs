//! Interface emulator — renders through the CLIENT engine.
//!
//! The visual is produced by the client's own `draw_interface` (the exact
//! in-game path: real PixFont text, sprites, and Pix3D model components
//! that ANIMATE via `animate_interface` + SeqType frames). The bridge's
//! `render_interface` opens the group, advances animations at 50Hz, and
//! returns the composited 765×503 frame; this view just uploads it and
//! draws an interaction overlay (hover / select / component list) using
//! the component geometry from the engine's `if_type::STORE`.

use std::collections::HashMap;

use client::config::if_type::{self, IfType};
use eframe::egui;

use crate::Selection;
use crate::client_bridge::{self, ClientSystems, GAME_H, GAME_W};
use crate::pix_bridge;

pub fn draw(ui: &mut egui::Ui, sys: &mut ClientSystems, group_id: u32, sel: &mut Selection) {
    // Engine render — opens + animates + composites the whole group over
    // the full game canvas.
    let pixels = client_bridge::render_interface(sys, group_id as i32);
    let tex = pix_bridge::upload_rgb(
        ui.ctx(),
        format!("ifx_engine_{group_id}"),
        &pixels,
        GAME_W as usize,
        GAME_H as usize,
    );

    // Snapshot the opened components for the interaction overlay.
    let components: Vec<IfType> = {
        let s = if_type::STORE.lock().unwrap();
        s.list
            .get(group_id as usize)
            .and_then(|o| o.as_ref())
            .map(|v| v.iter().filter_map(|c| c.clone()).collect())
            .unwrap_or_default()
    };
    if components.is_empty() {
        ui.label("interface group has no components (or failed to open).");
        return;
    }

    let by_sub_id: HashMap<i32, &IfType> = components.iter().map(|c| (c.sub_id, c)).collect();
    let offsets: HashMap<i32, (i32, i32)> = components
        .iter()
        .map(|c| (c.sub_id, parent_offset(c, &by_sub_id)))
        .collect();

    section(ui, "interface", |ui| {
        egui::Grid::new("if_meta").num_columns(2).striped(true).show(ui, |ui| {
            kv(ui, "group", &group_id.to_string());
            kv(ui, "components", &components.len().to_string());
            let layer_count = components.iter().filter(|c| c.type_ == 0).count();
            kv(ui, "layers", &layer_count.to_string());
            let model_count = components.iter().filter(|c| c.type_ == 6).count();
            kv(ui, "models", &model_count.to_string());
        });
    });

    section(ui, "preview", |ui| {
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(GAME_W as f32, GAME_H as f32), egui::Sense::click());
        let painter = ui.painter_at(rect);
        painter.image(
            tex.id(),
            rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );

        let click_pos = response.clicked().then(|| response.interact_pointer_pos()).flatten();
        let hover_pos = response.hover_pos();
        let mut hovered: Option<&IfType> = None;
        let mut newly_selected: Option<i32> = None;
        for comp in &components {
            if comp.hide || comp.type_ == 0 {
                continue;
            }
            let (ox, oy) = offsets.get(&comp.sub_id).copied().unwrap_or((0, 0));
            let c_rect = egui::Rect::from_min_size(
                egui::pos2(rect.min.x + (comp.x + ox) as f32, rect.min.y + (comp.y + oy) as f32),
                egui::vec2(comp.width.max(1) as f32, comp.height.max(1) as f32),
            );
            if !rect.intersects(c_rect) {
                continue;
            }
            if hover_pos.is_some_and(|p| c_rect.contains(p)) {
                hovered = Some(comp);
            }
            if click_pos.is_some_and(|p| c_rect.contains(p)) {
                newly_selected = Some(comp.sub_id);
            }
        }

        if let Some(fid) = sel.file_id {
            if let Some(comp) = components.iter().find(|c| c.sub_id == fid) {
                let (ox, oy) = offsets.get(&comp.sub_id).copied().unwrap_or((0, 0));
                painter.rect_stroke(
                    egui::Rect::from_min_size(
                        egui::pos2(
                            rect.min.x + (comp.x + ox) as f32,
                            rect.min.y + (comp.y + oy) as f32,
                        ),
                        egui::vec2(comp.width.max(1) as f32, comp.height.max(1) as f32),
                    ),
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
                let extra = match comp.type_ {
                    5 if comp.graphic >= 0 => format!("  spr#{}", comp.graphic),
                    6 if comp.model1_id >= 0 => {
                        format!("  model#{} anim#{}", comp.model1_id, comp.model_anim)
                    }
                    _ => String::new(),
                };
                let label = format!(
                    "#{:>3}  {:<8} {:>4},{:<4}  {:>4}×{:<4}  layer={:<4}{}{}",
                    comp.sub_id,
                    component_type_label(comp.type_),
                    comp.x,
                    comp.y,
                    comp.width,
                    comp.height,
                    comp.layer_id,
                    if comp.hide { "  hide" } else { "" },
                    extra,
                );
                ui.label(egui::RichText::new(label).monospace().small());
            }
        });
    });
}

/// Walk the `layer_id` parent chain summing each parent's (x - scrollPosX,
/// y - scrollPosY) — mirrors the engine's recursive `renderx` accumulation
/// so the overlay rects line up with what `draw_interface` painted.
/// `layer_id` is `(group << 16) | layer_sub_id`; mask off the group bits.
fn parent_offset(c: &IfType, by_sub_id: &HashMap<i32, &IfType>) -> (i32, i32) {
    let mut ox = 0i32;
    let mut oy = 0i32;
    let mut cur = c.layer_id;
    let mut visited: std::collections::HashSet<i32> = std::collections::HashSet::new();
    while cur >= 0 && visited.insert(cur) {
        let Some(parent) = by_sub_id.get(&(cur & 0xFFFF)) else { break };
        ox += parent.x - parent.scroll_pos_x;
        oy += parent.y - parent.scroll_pos_y;
        if parent.layer_id == cur {
            break;
        }
        cur = parent.layer_id;
    }
    (ox, oy)
}

fn draw_hover_tooltip(ui: &mut egui::Ui, c: &IfType) {
    ui.label(
        egui::RichText::new(format!("#{}  {}", c.sub_id, component_type_label(c.type_))).strong(),
    );
    ui.label(
        egui::RichText::new(format!("pos {},{}  size {}×{}", c.x, c.y, c.width, c.height))
            .monospace()
            .small(),
    );
    if c.layer_id >= 0 {
        ui.label(egui::RichText::new(format!("layer parent: #{}", c.layer_id & 0xFFFF)).small());
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
    let ops: Vec<String> = c
        .op_names
        .iter()
        .enumerate()
        .filter(|(_, o)| !o.is_empty())
        .map(|(i, o)| format!("{}:{o}", i + 1))
        .collect();
    if !ops.is_empty() {
        ui.label(format!("ops: {}", ops.join(", ")));
    }
    if c.hashook {
        ui.label(egui::RichText::new("has script hooks").weak());
    }
}

fn component_type_label(t: i32) -> &'static str {
    match t {
        0 => "layer",
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
