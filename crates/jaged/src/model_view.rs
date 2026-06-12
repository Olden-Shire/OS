//! Software-rendered model viewer. Uses the ported `pix::ModelRenderer` + `Pix3D` exactly
//! the way the rev1 Java client does, then uploads the resulting `Pix2D` buffer to an
//! egui texture for display.

use cache::model::Model;
use eframe::egui;
use pix::pix3d::{cos_table, sin_table};
use pix::{model_light, ModelRenderer, Pix2D, Pix3D};

use crate::pix_bridge;

/// Viewer state. Angles are Jagex units (0..2047 = full turn); `zoom` is the camera
/// distance term (multiplied by sin/cos of cam_pitch per Java's interface convention).
///
/// Drag-x orbits yaw, drag-y orbits camera pitch. The model's authored origin is shifted
/// to its bounds centre so arbitrary cache models appear centred at all camera angles.
pub struct ModelView {
    pub yaw: i32,
    pub roll: i32,
    pub camera_pitch: i32,
    pub zoom: i32,
    pub lighting: bool,
}

impl Default for ModelView {
    fn default() -> Self {
        // Head-on default (no camera tilt, no spin). Zoom = 0 → auto-fit to bounds at a
        // slightly tighter framing than the original (set in `draw` from model extent).
        Self { yaw: 0, roll: 0, camera_pitch: 0, zoom: 0, lighting: true }
    }
}

pub fn draw(ui: &mut egui::Ui, group_id: u32, bytes: &[u8], state: &mut ModelView) {
    if bytes.is_empty() {
        ui.label("(empty)");
        return;
    }
    let model = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| Model::decode(bytes))) {
        Ok(m) => m,
        Err(_) => {
            ui.colored_label(egui::Color32::LIGHT_RED, "model decode failed");
            return;
        }
    };

    // Small toolbar above the canvas — keeps interactive controls (lighting/reset)
    // attached to the viewport, but mesh stats (point/face counts) live in the right
    // `details` panel next to the summary card.
    ui.horizontal(|ui| {
        ui.checkbox(&mut state.lighting, "lighting");
        if ui.button("reset view").clicked() {
            state.yaw = 0;
            state.roll = 0;
            state.camera_pitch = 0;
            state.zoom = 0;
        }
        ui.label(
            egui::RichText::new("· drag = orbit · scroll = zoom · software pix3d")
                .weak()
                .small(),
        );
    });

    // Fill the rest of the panel.
    let avail = ui.available_size_before_wrap();
    if avail.x < 16.0 || avail.y < 16.0 {
        return;
    }
    let (rect, response) =
        ui.allocate_exact_size(avail, egui::Sense::click_and_drag());

    // Orbit input — drag to rotate, scroll to zoom. Convert pixel deltas to Jagex angle
    // units (2048 = full turn).
    if response.dragged() {
        let drag = response.drag_delta();
        state.yaw = (state.yaw + (drag.x * 4.0) as i32).rem_euclid(2048);
        state.camera_pitch =
            (state.camera_pitch + (drag.y * 4.0) as i32).clamp(-512, 512);
    }
    let scroll = ui.input(|i| i.smooth_scroll_delta.y);
    if scroll != 0.0 && response.hovered() {
        let (_mn, mx) = model.bounds().unwrap_or(((0, 0, 0), (1, 1, 1)));
        let extent = mx.0.max(mx.1).max(mx.2).max(1);
        let zoom_step = (extent as f32 * scroll * -0.5) as i32;
        state.zoom = (state.zoom + zoom_step).max(extent * 2);
    }

    if model.num_points == 0 || model.num_faces == 0 {
        ui.painter_at(rect).rect_filled(rect, 4.0, egui::Color32::from_rgb(28, 30, 36));
        ui.painter_at(rect).text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "no geometry",
            egui::FontId::proportional(14.0),
            egui::Color32::GRAY,
        );
        return;
    }

    // Bounds-centre so arbitrary models render centred regardless of their authored
    // origin (the rev1 IfType-driven path assumes models are authored at origin; we
    // can't assume that for a free cache browser).
    let (mn, mx) = model.bounds().unwrap_or(((0, 0, 0), (1, 1, 1)));
    let cx = (mn.0 + mx.0) / 2;
    let cy = (mn.1 + mx.1) / 2;
    let cz = (mn.2 + mx.2) / 2;
    let extent = ((mx.0 - mn.0).max(mx.1 - mn.1).max(mx.2 - mn.2)).max(1);
    // extent*3 → model ~67% of viewport; "a bit closer" than the old extent*4.
    let zoom = if state.zoom > 0 { state.zoom } else { extent * 3 };

    // Allocate a Pix2D the same size as the viewport rect (in physical pixels).
    let ppp = ui.ctx().pixels_per_point();
    let tex_w = (rect.width() * ppp) as i32;
    let tex_h = (rect.height() * ppp) as i32;
    if tex_w < 4 || tex_h < 4 {
        return;
    }
    let mut p2 = Pix2D::new(tex_w, tex_h);
    p2.fill_rect(0, 0, tex_w, tex_h, 0xFF1C_1E24); // background

    let mut p3 = Pix3D::new(&p2);
    p3.set_origin(&p2, tex_w / 2, tex_h / 2);

    // Java interface convention: origin = (0, zoom*sin(cam_pitch), zoom*cos(cam_pitch))
    // places the model on the tilted camera's optical axis so the projection lands at
    // Pix3D.origin (the viewport centre). We extend with -cx/-cy/-cz so the model's
    // bounds centre — not its authored origin — is what the camera focuses on.
    let var207 = (zoom * sin_table(state.camera_pitch)) >> 16;
    let var208 = (zoom * cos_table(state.camera_pitch)) >> 16;
    let mut renderer = ModelRenderer::new();
    // Bake lighting per-frame with the standard interface params from
    // `IfType.java:1100`. Cheap relative to render; can be cached on the model in
    // future. When `lighting` is off, pass `None` for the unlit raw-colour path.
    let lit_owned = if state.lighting {
        Some(model_light(&model, 64, 768, -50, -10, -50))
    } else {
        None
    };
    renderer.obj_render(
        &model,
        lit_owned.as_ref(),
        &mut p2,
        &p3,
        /*model_pitch=*/ 0,
        state.yaw,
        state.roll,
        state.camera_pitch,
        -cx,
        -cy + var207,
        -cz + var208,
    );

    let texture = pix_bridge::upload(ui.ctx(), format!("model_{group_id}"), &p2);
    ui.painter_at(rect).image(
        texture.id(),
        rect,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );
}
