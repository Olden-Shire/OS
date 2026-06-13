//! Shared visual tokens + the app theme. One accent, consistent panel
//! tones, tight spacing — applied once at startup.

use eframe::egui;
use egui::Color32;

/// Primary accent — selections, the archive rail bar, focus rings.
pub const ACCENT: Color32 = Color32::from_rgb(86, 142, 230);
/// Accent tuned for text on the dark panels (lighter, higher contrast).
pub const ACCENT_TEXT: Color32 = Color32::from_rgb(150, 192, 255);

// Surface ramp — darkest (window) → panels → raised widgets.
const BG_WINDOW: Color32 = Color32::from_rgb(20, 22, 27);
const BG_PANEL: Color32 = Color32::from_rgb(26, 29, 35);
const BG_FAINT: Color32 = Color32::from_rgb(33, 37, 44);
const BG_RAISED: Color32 = Color32::from_rgb(40, 45, 54);
const STROKE_DIM: Color32 = Color32::from_rgb(48, 53, 62);

pub fn install(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    let v = &mut style.visuals;

    v.dark_mode = true;
    v.panel_fill = BG_PANEL;
    v.window_fill = BG_WINDOW;
    v.extreme_bg_color = BG_WINDOW;
    v.faint_bg_color = BG_FAINT;
    v.window_stroke = egui::Stroke::new(1.0, STROKE_DIM);

    v.selection.bg_fill = ACCENT.gamma_multiply(0.32);
    v.selection.stroke = egui::Stroke::new(1.0, ACCENT_TEXT);
    v.hyperlink_color = ACCENT_TEXT;

    let r = 5.0;
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = r.into();
    }
    v.widgets.noninteractive.bg_fill = BG_PANEL;
    v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, STROKE_DIM);
    v.widgets.inactive.bg_fill = BG_RAISED;
    v.widgets.inactive.weak_bg_fill = BG_FAINT;
    v.widgets.inactive.bg_stroke = egui::Stroke::NONE;
    v.widgets.hovered.bg_fill = Color32::from_rgb(52, 58, 70);
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(46, 52, 63);
    v.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, STROKE_DIM);
    v.widgets.active.bg_fill = ACCENT.gamma_multiply(0.5);
    v.widgets.active.weak_bg_fill = ACCENT.gamma_multiply(0.35);

    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(10.0, 5.0);
    style.spacing.interact_size.y = 24.0;
    style.spacing.window_margin = egui::Margin::same(10);

    ctx.set_style(style);
}

/// A compact section header used across the detail/inspector views.
pub fn section_label(ui: &mut egui::Ui, title: &str) {
    ui.add_space(6.0);
    ui.label(egui::RichText::new(title.to_uppercase()).size(11.0).weak().strong());
    ui.add_space(2.0);
}
