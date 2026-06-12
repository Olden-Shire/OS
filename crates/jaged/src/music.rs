//! Music player widget for song / jingle inspector views.
//!
//! Shows: name (from a sibling `.pack` if present), track count, division (PPQN),
//! event count, estimated duration. Play / Stop buttons exist but are disabled until the
//! Jagex synth port lands (see queued task #39).

use std::collections::BTreeMap;
use std::path::Path;

use cache::Cache;
use cache::content::pack_file;
use eframe::egui;
use synth::{MidiFile, Patch};

const PATCHES_ARCHIVE: u8 = 15;

/// Compact stats decoded from a standard-MIDI byte buffer.
pub struct MidiStats {
    pub format: u16,
    pub tracks: u16,
    pub division: u16,
    pub total_events: u32,
    pub max_ticks: u32,
    pub tempo_us_per_qn: u32, // last tempo seen; default 500_000 (120 bpm)
}

impl MidiStats {
    /// Approximate duration assuming the last-seen tempo (ignores tempo changes —
    /// good enough for the inspector readout).
    pub fn approx_seconds(&self) -> f64 {
        if self.division == 0 { return 0.0; }
        let ticks_per_qn = self.division as f64;
        let qn = self.max_ticks as f64 / ticks_per_qn;
        qn * (self.tempo_us_per_qn as f64 / 1_000_000.0)
    }
}

/// Parse a standard-MIDI byte buffer for inspector stats. Returns None on malformed
/// input. Walks events to find max tick across tracks and the last tempo meta.
pub fn parse_stats(src: &[u8]) -> Option<MidiStats> {
    if src.len() < 14 || &src[..4] != b"MThd" { return None; }
    let format = u16::from_be_bytes(src[8..10].try_into().ok()?);
    let tracks = u16::from_be_bytes(src[10..12].try_into().ok()?);
    let division = u16::from_be_bytes(src[12..14].try_into().ok()?);
    let mut pos = 14usize;
    let mut total_events = 0u32;
    let mut max_ticks = 0u32;
    let mut tempo_us = 500_000u32;
    for _ in 0..tracks {
        if pos + 8 > src.len() || &src[pos..pos + 4] != b"MTrk" { return None; }
        let trk_len = u32::from_be_bytes(src[pos + 4..pos + 8].try_into().ok()?) as usize;
        pos += 8;
        let end = pos.min(src.len()).saturating_add(trk_len).min(src.len());
        let mut tick = 0u32;
        let mut status = 0u8;
        while pos < end {
            let (delta, n) = read_vlq(&src[pos..end])?;
            pos += n;
            tick = tick.wrapping_add(delta);
            if pos >= end { break; }
            let first = src[pos];
            if first & 0x80 != 0 {
                status = first;
                pos += 1;
                if pos >= end { break; }
            }
            total_events += 1;
            match status & 0xF0 {
                0x80 | 0x90 | 0xA0 | 0xB0 | 0xE0 => { pos += 2; }
                0xC0 | 0xD0 => { pos += 1; }
                _ => match status {
                    0xFF => {
                        let meta_type = src[pos]; pos += 1;
                        let (mlen, mn) = read_vlq(&src[pos..end])?;
                        pos += mn;
                        if meta_type == 0x51 && mlen == 3 && pos + 3 <= end {
                            tempo_us = ((src[pos] as u32) << 16)
                                | ((src[pos + 1] as u32) << 8)
                                | (src[pos + 2] as u32);
                        }
                        pos += mlen as usize;
                    }
                    _ => return None,
                },
            }
        }
        if tick > max_ticks { max_ticks = tick; }
        pos = end;
    }
    Some(MidiStats { format, tracks, division, total_events, max_ticks, tempo_us_per_qn: tempo_us })
}

fn read_vlq(s: &[u8]) -> Option<(u32, usize)> {
    let mut v = 0u32;
    for (i, &b) in s.iter().enumerate() {
        v = (v << 7) | u32::from(b & 0x7F);
        if b & 0x80 == 0 { return Some((v, i + 1)); }
        if i >= 4 { return None; }
    }
    None
}

/// Look up a name in `Content/pack/{scope}.pack` if present. Returns `default_name` when
/// the pack file (or the entry) is missing.
pub fn pack_name(scope: &str, id: u32, default_name: &str) -> String {
    let pack_path = Path::new("Content/pack").join(format!("{scope}.pack"));
    let map: BTreeMap<u32, String> = match pack_file::read(&pack_path) {
        Ok(m) => m,
        Err(_) => return default_name.to_string(),
    };
    map.get(&id).cloned().unwrap_or_else(|| default_name.to_string())
}

/// Draw the music player card for a song or jingle. `midi_bytes` is the decoded standard
/// MIDI (output of `io::midi::decode`); `name` is what to display under the icon.
/// `cache` is used to decode required patches inline so the user sees which instruments
/// the song needs and whether each one parses. `player` is lazy-initialised on first
/// Play click — opening the cpal stream is what causes the audio device to claim itself.
pub fn draw(
    ui: &mut egui::Ui,
    cache: &mut Cache,
    name: &str,
    midi_bytes: &[u8],
    player: &mut Option<synth::Player>,
    player_error: &mut Option<String>,
) {
    let stats = parse_stats(midi_bytes);

    egui::Frame::group(ui.style())
        .corner_radius(6.0)
        .inner_margin(egui::Margin::same(14))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("♪")
                        .size(46.0)
                        .color(egui::Color32::from_rgb(220, 160, 220)),
                );
                ui.add_space(8.0);
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new(name).heading());
                    ui.label(
                        egui::RichText::new(format!("{} bytes (standard MIDI)", midi_bytes.len()))
                            .weak()
                            .small(),
                    );
                });
            });

            ui.add_space(6.0);
            if let Some(s) = &stats {
                egui::Grid::new("midi_stats").num_columns(2).striped(true).show(ui, |ui| {
                    kv(ui, "format", &format!("{}", s.format));
                    kv(ui, "tracks", &format!("{}", s.tracks));
                    kv(ui, "division", &format!("{} ticks / quarter", s.division));
                    kv(ui, "events", &format!("{}", s.total_events));
                    kv(ui, "ticks", &format!("{}", s.max_ticks));
                    let bpm = if s.tempo_us_per_qn > 0 {
                        60_000_000.0 / s.tempo_us_per_qn as f64
                    } else { 0.0 };
                    kv(ui, "tempo", &format!("{bpm:.1} bpm"));
                    let secs = s.approx_seconds();
                    let mins = (secs / 60.0) as u32;
                    let s = secs % 60.0;
                    kv(ui, "duration", &format!("~{mins}:{s:05.2}"));
                });
            } else {
                ui.colored_label(egui::Color32::LIGHT_RED, "could not parse MIDI header");
            }

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("▶  Play").clicked() {
                    play_song(cache, midi_bytes, player, player_error);
                }
                if ui.button("⏹  Stop").clicked() {
                    if let Some(p) = player.as_ref() {
                        p.manager().lock().stop();
                    }
                }
                if let Some(err) = player_error.as_ref() {
                    ui.colored_label(egui::Color32::LIGHT_RED, err);
                }
            });

            ui.add_space(8.0);
            draw_patch_list(ui, cache, midi_bytes);
        });
}

/// Lazy-init the cpal player on first click, then load + play the song.
fn play_song(
    cache: &mut Cache,
    midi_bytes: &[u8],
    player: &mut Option<synth::Player>,
    player_error: &mut Option<String>,
) {
    if player.is_none() {
        match synth::Player::open() {
            Ok(p) => {
                *player = Some(p);
                *player_error = None;
            }
            Err(e) => {
                *player_error = Some(format!("audio init failed: {e}"));
                return;
            }
        }
    }
    let p = player.as_ref().unwrap();
    let manager = p.manager();
    let mut mgr = manager.lock();
    let (mp, mw) = mgr.load_song(cache, midi_bytes.to_vec(), false);
    if mp + mw > 0 {
        *player_error = Some(format!("loaded with {mp} missing patches, {mw} missing waves"));
    } else {
        *player_error = None;
    }
}

/// Discover and render the patch list this song requires. Compact one-line-per-patch
/// list inside a bounded scroll area so it can't blow the inspector width out.
fn draw_patch_list(ui: &mut egui::Ui, cache: &mut Cache, midi_bytes: &[u8]) {
    let mut f = MidiFile::from_standard(midi_bytes.to_vec());
    f.discover_patches();
    let Some(patches) = f.patches.as_ref() else {
        return;
    };
    if patches.is_empty() {
        return;
    }
    ui.label(egui::RichText::new(format!("PATCHES ({})", patches.len())).small().weak());
    egui::ScrollArea::vertical()
        .id_salt("song_patches")
        .max_height(180.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            for (&pid, hits) in patches {
                let note_count = hits.iter().filter(|&&b| b != 0).count();
                let (status_text, status_color) =
                    match cache.read_group(PATCHES_ARCHIVE, pid as u32) {
                        Ok(Some(bytes)) if !bytes.is_empty() => {
                            match std::panic::catch_unwind(|| Patch::decode(&bytes)) {
                                Ok(p) => (
                                    format!("ok ({} env)", p.envelopes.len()),
                                    egui::Color32::LIGHT_GREEN,
                                ),
                                Err(_) => ("decode panic".into(), egui::Color32::LIGHT_RED),
                            }
                        }
                        _ => ("missing".into(), egui::Color32::LIGHT_RED),
                    };
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!("{pid:>4}   · {note_count:>3} notes"))
                            .monospace(),
                    );
                    ui.label(egui::RichText::new(status_text).color(status_color).small());
                });
            }
        });
}

fn kv(ui: &mut egui::Ui, k: &str, v: &str) {
    ui.label(egui::RichText::new(k).weak());
    ui.label(egui::RichText::new(v).monospace());
    ui.end_row();
}
