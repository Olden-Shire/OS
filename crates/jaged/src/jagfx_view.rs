//! Sound-effect (JagFX) player — the audio card for archive 4
//! selections, mirroring the vorbis/song players. Decodes through the
//! CLIENT's JagFX additive-synth port (the exact in-game path: up to 10
//! tones → 22050 Hz Wave) and plays the result as a one-shot voice on
//! the bridged PcmPlayer.

use std::sync::Arc;

use cache::Cache;
use client::sound::jagfx::JagFX;
use client::sound::pcm_player::PcmPlayer;
use eframe::egui;

pub fn draw(
    ui: &mut egui::Ui,
    _cache: &mut Cache,
    group: u32,
    bytes: &[u8],
    player: Option<&PcmPlayer>,
    player_error: &mut Option<String>,
) {
    if bytes.is_empty() {
        ui.label("(empty)");
        return;
    }

    let Ok(fx) = std::panic::catch_unwind(|| JagFX::decode(bytes)) else {
        ui.colored_label(egui::Color32::LIGHT_RED, "synth record failed to decode");
        return;
    };
    let tone_count = fx.tones.iter().flatten().count();
    let duration_ms = fx
        .tones
        .iter()
        .flatten()
        .map(|t| t.start + t.length)
        .max()
        .unwrap_or(0);
    let name = crate::music::pack_name("jagfx", group, &group.to_string());

    egui::Frame::group(ui.style())
        .corner_radius(6.0)
        .inner_margin(egui::Margin::same(14))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("🔊")
                        .size(42.0)
                        .color(egui::Color32::from_rgb(230, 190, 140)),
                );
                ui.add_space(8.0);
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new(&name).heading());
                    ui.label(
                        egui::RichText::new(format!("{} bytes (jagfx synth)", bytes.len()))
                            .weak()
                            .small(),
                    );
                });
            });

            ui.add_space(6.0);
            egui::Grid::new("jagfx_stats").num_columns(2).striped(true).show(ui, |ui| {
                kv(ui, "tones", &format!("{tone_count} / 10"));
                kv(ui, "duration", &format!("{} ms", duration_ms));
                kv(
                    ui,
                    "loop",
                    &if fx.loop_begin < fx.loop_end {
                        format!("{} .. {} ms", fx.loop_begin, fx.loop_end)
                    } else {
                        "none".to_string()
                    },
                );
                kv(ui, "output", "22050 Hz · 8-bit");
            });

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("▶  Play").clicked() {
                    play_fx(bytes, player, player_error);
                }
                if ui.button("⏹  Stop").clicked() {
                    if let Some(p) = player {
                        p.manager().lock().stop_waves();
                    }
                }
                if let Some(err) = player_error.as_ref() {
                    ui.colored_label(egui::Color32::LIGHT_RED, err);
                }
            });
        });
}

fn play_fx(bytes: &[u8], player: Option<&PcmPlayer>, player_error: &mut Option<String>) {
    let Some(p) = player else {
        if player_error.is_none() {
            *player_error = Some("audio device unavailable (failed at init)".into());
        }
        return;
    };
    let wave = match std::panic::catch_unwind(|| {
        let mut fx = JagFX::decode(bytes);
        fx.optimise_start();
        fx.to_wave()
    }) {
        Ok(w) if !w.samples.is_empty() => Arc::new(w),
        Ok(_) => {
            *player_error = Some("synth rendered no samples".into());
            return;
        }
        Err(_) => {
            *player_error = Some("synth render panicked".into());
            return;
        }
    };
    p.manager().lock().play_wave(wave, 255);
    *player_error = None;
}

fn kv(ui: &mut egui::Ui, k: &str, v: &str) {
    ui.label(egui::RichText::new(k).weak());
    ui.label(egui::RichText::new(v).monospace());
    ui.end_row();
}
