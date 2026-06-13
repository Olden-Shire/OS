//! Vorbis sample player — the audio card for archive 14 selections,
//! mirroring the song/jingle player in `music.rs`. Decodes through the
//! CLIENT's jagvorbis port (the exact in-game decode path: shared
//! headers from group 0 + per-sample packets) and plays the resulting
//! Wave as a one-shot voice on the bridged PcmPlayer.

use std::sync::{Arc, OnceLock};

use cache::Cache;
use client::sound::jagvorbis::{JagVorbis, VorbisHeaders};
use client::sound::pcm_player::PcmPlayer;
use eframe::egui;

const VORBIS_ARCHIVE: u8 = 14;

/// Shared setup header (group 0) — parsed once, reused for every sample.
static HEADERS: OnceLock<Option<Arc<VorbisHeaders>>> = OnceLock::new();

fn shared_headers(cache: &mut Cache) -> Option<Arc<VorbisHeaders>> {
    HEADERS
        .get_or_init(|| match cache.read_group(VORBIS_ARCHIVE, 0) {
            Ok(Some(b)) if !b.is_empty() => {
                std::panic::catch_unwind(|| VorbisHeaders::parse(&b)).ok().map(Arc::new)
            }
            _ => None,
        })
        .clone()
}

pub fn draw(
    ui: &mut egui::Ui,
    cache: &mut Cache,
    group: u32,
    bytes: &[u8],
    player: Option<&PcmPlayer>,
    player_error: &mut Option<String>,
) {
    if group == 0 {
        ui.vertical_centered(|ui| {
            ui.add_space(60.0);
            ui.label(egui::RichText::new("shared Vorbis setup header").strong());
            ui.label(
                egui::RichText::new(
                    "group 0 carries the codebooks / floors / residues every \
                     sample in this archive references — it is not audio itself.",
                )
                .weak(),
            );
        });
        return;
    }
    if bytes.is_empty() {
        ui.label("(empty)");
        return;
    }

    let Ok(sample) = std::panic::catch_unwind(|| JagVorbis::decode(bytes)) else {
        ui.colored_label(egui::Color32::LIGHT_RED, "sample failed to decode");
        return;
    };
    let name = crate::music::pack_name("vorbis", group, &group.to_string());

    egui::Frame::group(ui.style())
        .corner_radius(6.0)
        .inner_margin(egui::Margin::same(14))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("♬")
                        .size(46.0)
                        .color(egui::Color32::from_rgb(160, 220, 200)),
                );
                ui.add_space(8.0);
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new(&name).heading());
                    ui.label(
                        egui::RichText::new(format!("{} bytes (jag vorbis)", bytes.len()))
                            .weak()
                            .small(),
                    );
                });
            });

            ui.add_space(6.0);
            egui::Grid::new("vorbis_stats").num_columns(2).striped(true).show(ui, |ui| {
                kv(ui, "sample rate", &format!("{} Hz", sample.sample_rate));
                kv(ui, "samples", &format!("{}", sample.sample_count));
                let secs = if sample.sample_rate > 0 {
                    sample.sample_count as f64 / sample.sample_rate as f64
                } else {
                    0.0
                };
                kv(ui, "duration", &format!("{secs:.2} s"));
                kv(
                    ui,
                    "loop",
                    &if sample.has_loop {
                        format!("{} .. {}", sample.loop_start, sample.loop_end)
                    } else {
                        "none".to_string()
                    },
                );
                kv(ui, "packets", &format!("{}", sample.audio_packets.len()));
            });

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("▶  Play").clicked() {
                    play_sample(cache, &sample, player, player_error);
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

fn play_sample(
    cache: &mut Cache,
    sample: &JagVorbis,
    player: Option<&PcmPlayer>,
    player_error: &mut Option<String>,
) {
    let Some(p) = player else {
        if player_error.is_none() {
            *player_error = Some("audio device unavailable (failed at init)".into());
        }
        return;
    };
    let Some(headers) = shared_headers(cache) else {
        *player_error = Some("shared vorbis header (group 0) failed to load".into());
        return;
    };
    let wave = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        sample.to_wave(&headers)
    })) {
        Ok(w) => Arc::new(w),
        Err(_) => {
            *player_error = Some("packet decode panicked".into());
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
