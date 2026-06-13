//! Software-rendered model viewer — runs the CLIENT's 1:1 dash3d
//! pipeline (ModelUnlit decode → ModelLit light → `world_render`), the
//! exact path the in-game SCENE renders models with. The scene's
//! perspective orbit camera gives every face a well-separated depth, so
//! the painter sort is stable (the icon/`obj_render_icon` path collapses
//! coplanar faces into depth ties and z-fights on arbitrary models).
//! The finished client Pix2D frame uploads to an egui texture.

use client::dash3d::model_lit::ModelLit;
use client::dash3d::model_unlit::ModelUnlit;
use client::dash3d::pix3d;
use client::graphics::pix2d;
use eframe::egui;

use crate::pix_bridge;

/// Viewer state — a free turntable orbit. `yaw`/`pitch` 0..2047 (full
/// rotation both axes), `zoom` is the camera distance (0 = auto from the
/// model's extent). Drag-x orbits yaw, drag-y pitch. The model's authored
/// origin is shifted to its bounds centre so it orbits around its middle.
/// Rendering goes through the scene's `world_render` pipeline (not the
/// icon path) for stable, scene-identical depth sorting.
pub struct ModelView {
    pub yaw: i32,
    pub camera_pitch: i32,
    pub zoom: i32,
    pub lighting: bool,
}

impl Default for ModelView {
    fn default() -> Self {
        // Tilted 3/4 view like the scene camera (never straight-on,
        // where coplanar faces would tie in the painter sort).
        Self { yaw: 0, camera_pitch: 280, zoom: 0, lighting: true }
    }
}

pub fn draw(ui: &mut egui::Ui, group_id: u32, bytes: &[u8], state: &mut ModelView) {
    if bytes.is_empty() {
        ui.label("(empty)");
        return;
    }
    // Decode + light through the client pipeline. light() consumes the
    // unlit normals, so build fresh per frame (cheap at tool scale —
    // same as the old per-frame model_light bake).
    let lit = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut unlit = ModelUnlit::from_bytes(bytes.to_vec());
        unlit.calculate_normals();
        if state.lighting {
            // IfType model lighting params (IfType.java:1100).
            ModelLit::light(&mut unlit, 64, 768, -50, -10, -50)
        } else {
            ModelLit::from_unlit_flat(&unlit, 64, 768, -50, -10, -50)
        }
    }));
    let mut lit = match lit {
        Ok(l) => l,
        Err(_) => {
            ui.colored_label(egui::Color32::LIGHT_RED, "model decode failed");
            return;
        }
    };

    ui.horizontal(|ui| {
        ui.checkbox(&mut state.lighting, "lighting");
        if ui.button("reset view").clicked() {
            state.yaw = 0;
            state.camera_pitch = 280;
            state.zoom = 0;
        }
        ui.label(
            egui::RichText::new("· drag = orbit · scroll = zoom · client dash3d (1:1)")
                .weak()
                .small(),
        );
    });

    let avail = ui.available_size_before_wrap();
    if avail.x < 16.0 || avail.y < 16.0 {
        return;
    }
    let (rect, response) = ui.allocate_exact_size(avail, egui::Sense::click_and_drag());

    if response.dragged() {
        let drag = response.drag_delta();
        // Free turntable orbit (full yaw + pitch). The model orbits its
        // own centre at `zoom` distance, so it's always in front of the
        // camera at any angle.
        state.yaw = (state.yaw + (drag.x * 4.0) as i32).rem_euclid(2048);
        state.camera_pitch = (state.camera_pitch + (drag.y * 4.0) as i32).rem_euclid(2048);
    }

    if lit.num_points == 0 || lit.num_faces == 0 {
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

    // Bounds-centre so arbitrary models render (and orbit) around their
    // geometric middle regardless of authored origin.
    let n = lit.num_points as usize;
    let (mut mnx, mut mny, mut mnz) = (i32::MAX, i32::MAX, i32::MAX);
    let (mut mxx, mut mxy, mut mxz) = (i32::MIN, i32::MIN, i32::MIN);
    for i in 0..n {
        mnx = mnx.min(lit.point_x[i]);
        mny = mny.min(lit.point_y[i]);
        mnz = mnz.min(lit.point_z[i]);
        mxx = mxx.max(lit.point_x[i]);
        mxy = mxy.max(lit.point_y[i]);
        mxz = mxz.max(lit.point_z[i]);
    }
    let extent = ((mxx - mnx).max(mxy - mny).max(mxz - mnz)).max(1);
    lit.translate(-(mnx + mxx) / 2, -(mny + mxy) / 2, -(mnz + mxz) / 2);

    let scroll = ui.input(|i| i.smooth_scroll_delta.y);
    if scroll != 0.0 && response.hovered() {
        let zoom_step = (extent as f32 * scroll * -0.5) as i32;
        let base = if state.zoom > 0 { state.zoom } else { extent * 3 };
        state.zoom = (base + zoom_step).max(extent);
    }
    let zoom = if state.zoom > 0 { state.zoom } else { extent * 3 };

    let ppp = ui.ctx().pixels_per_point();
    let tex_w = (rect.width() * ppp) as i32;
    let tex_h = (rect.height() * ppp) as i32;
    if tex_w < 4 || tex_h < 4 {
        return;
    }

    // Render exactly as the scene does: a perspective orbit camera at
    // `zoom` around the (recentred) model at the world origin, through
    // ModelLit::world_render. The camera world position mirrors
    // scene.rs's orbit fallback; rel = model(0) - cam = -cam.
    let sin_t = pix3d::sin_table();
    let cos_t = pix3d::cos_table();
    let pitch = (state.camera_pitch & 0x7FF) as usize;
    let yaw = (state.yaw & 0x7FF) as usize;
    let sin_p = sin_t[pitch];
    let cos_p = cos_t[pitch];
    let sin_y = sin_t[yaw];
    let cos_y = cos_t[yaw];
    let h_radius = (zoom * cos_p) >> 16;
    let rel_x = -((h_radius * sin_y) >> 16);
    let rel_y = (zoom * sin_p) >> 16;
    let rel_z = (h_radius * cos_y) >> 16;

    // Bind a scratch frame on the client's global Pix2D, render, then
    // swap the real buffer back (the client pipeline draws through its
    // process-wide raster state; jaged is single-threaded here).
    let (prev, pw, ph) = pix2d::swap_pixels(vec![0x001C1E24; (tex_w * tex_h) as usize], tex_w, tex_h);
    pix2d::set_clipping(0, 0, tex_w, tex_h);
    pix3d::set_origin(tex_w / 2, tex_h / 2);
    pix3d::set_render_clipping();
    // world_render culls past `model_far_clip` (the scene's distance
    // cull); push it beyond this model's camera distance + reach so big
    // or zoomed-out models aren't culled.
    pix3d::set_model_far_clip((zoom + extent - 3500).max(0));
    let render = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        lit.world_render(0, sin_p, cos_p, sin_y, cos_y, rel_x, rel_y, rel_z, 0);
    }));
    pix3d::set_model_far_clip(0);
    let (frame, _, _) = pix2d::swap_pixels(prev, pw, ph);
    if render.is_err() {
        ui.colored_label(egui::Color32::LIGHT_RED, "render panicked");
        return;
    }

    let texture = pix_bridge::upload_rgb(
        ui.ctx(),
        format!("model_{group_id}"),
        &frame,
        tex_w as usize,
        tex_h as usize,
    );
    ui.painter_at(rect).image(
        texture.id(),
        rect,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );
}
