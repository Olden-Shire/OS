//! `jagex3.midi2.MidiManager` — orchestration. Loads a song, materialises its patches,
//! resolves wave references for each note-on, drives the audio output via cpal.
//!
//! The cpal output thread holds a reference to a [`MidiManager`] under a mutex and pulls
//! audio in chunks. UI calls `play`, `stop`, `set_volume` from the editor's main thread.

use std::sync::Arc;

use cache::Cache;
use parking_lot::Mutex;

use crate::file::MidiFile;
use crate::midi_player::MidiPlayer;
use crate::patch::Patch;
use crate::wave::Wave;
use crate::wave_cache::WaveCache;

const PATCHES_ARCHIVE: u8 = 15;

pub struct MidiManager {
    pub player: MidiPlayer,
    pub wave_cache: WaveCache,
    /// (patch_id, key) → Arc<Wave>. Pre-resolved when loading a song so the cpal callback
    /// never touches the cache (it's owned by the editor thread).
    pub wave_table: std::collections::HashMap<(i32, usize), Arc<Wave>>,
    pub patch_table: std::collections::HashMap<i32, Arc<Patch>>,
    pub debug_note_ons_seen: usize,
    pub debug_note_ons_played: usize,
    pub debug_note_ons_missing_patch: usize,
    pub debug_note_ons_missing_wave: usize,
}

impl MidiManager {
    pub fn new(frequency: i32, stereo: bool) -> Self {
        Self {
            player: MidiPlayer::new(frequency, stereo),
            wave_cache: WaveCache::new(),
            wave_table: std::collections::HashMap::new(),
            patch_table: std::collections::HashMap::new(),
            debug_note_ons_seen: 0,
            debug_note_ons_played: 0,
            debug_note_ons_missing_patch: 0,
            debug_note_ons_missing_wave: 0,
        }
    }

    /// Load a song from standard-MIDI bytes. Decodes the patches it requires from the
    /// `cache`, decodes each patch's referenced waves, and queues them on the player.
    /// Returns the number of patches+waves it could not load (so the caller can warn).
    pub fn load_song(&mut self, cache: &mut Cache, midi: Vec<u8>, loop_song: bool) -> (usize, usize) {
        let mut file = MidiFile::from_standard(midi.clone());
        file.discover_patches();
        self.wave_table.clear();
        self.patch_table.clear();
        let mut missing_patches = 0usize;
        let mut missing_waves = 0usize;
        if let Some(patches) = file.patches.as_ref() {
            for (&pid, hits) in patches {
                let bytes = match cache.read_group(PATCHES_ARCHIVE, pid as u32).ok().flatten() {
                    Some(b) if !b.is_empty() => b,
                    _ => { missing_patches += 1; continue; }
                };
                let patch = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| Patch::decode(&bytes))) {
                    Ok(p) => Arc::new(p),
                    Err(_) => { missing_patches += 1; continue; }
                };
                for key in 0..128 {
                    if hits[key] != 0 && patch.note_wave_id[key] != 0 {
                        if let Some(wave) = self.wave_cache.get_by_wave_id(cache, patch.note_wave_id[key]) {
                            self.wave_table.insert((pid, key), wave);
                        } else {
                            missing_waves += 1;
                        }
                    }
                }
                self.patch_table.insert(pid, Arc::clone(&patch));
                self.player.install_patch(pid, patch);
            }
        }
        self.player.start(midi, loop_song);
        (missing_patches, missing_waves)
    }

    pub fn stop(&mut self) {
        self.player.stop();
        self.wave_table.clear();
        self.patch_table.clear();
    }

    pub fn set_volume(&mut self, v: i32) {
        self.player.set_global_volume(v);
    }

    /// Render `n` stereo frames into `out` (interleaved L,R as i32 — caller clips).
    /// Pulls events from the parser, materializes any note-ons against the pre-loaded
    /// wave table, then advances the player.
    pub fn render(&mut self, out: &mut [i32], n: usize) {
        // Zero the buffer — the mixer accumulates into it.
        for v in out.iter_mut().take(n * 2) {
            *v = 0;
        }
        if !self.player.loaded() {
            return;
        }
        let samples_per_unit = self.player.samples_per_tempo_unit();
        let mut remaining = n;
        let mut offset = 0usize;
        while remaining > 0 {
            let proposed_time = remaining as i64 * samples_per_unit as i64 + self.player.track_previous_time;
            if self.player.track_current_time - proposed_time >= 0 {
                self.player.track_previous_time = proposed_time;
                self.player.render(&mut out[offset * 2..], remaining);
                return;
            }
            let chunk = ((self.player.track_current_time - self.player.track_previous_time
                + samples_per_unit as i64 - 1) / samples_per_unit as i64) as i32;
            self.player.track_previous_time += samples_per_unit as i64 * chunk as i64;
            self.player.render(&mut out[offset * 2..], chunk as usize);
            offset += chunk as usize;
            remaining = remaining.saturating_sub(chunk as usize);
            // Pull pending events; apply note-ons here with wave lookup.
            let note_ons = self.player.update_midi();
            for (ch, key, vel, patch_id) in note_ons {
                self.debug_note_ons_seen += 1;
                let patch = self.patch_table.get(&patch_id).cloned();
                let wave = self.wave_table.get(&(patch_id, key)).cloned();
                match (patch, wave) {
                    (Some(p), Some(w)) => {
                        self.player.play_note_with_wave(ch, key, vel, p, w);
                        self.debug_note_ons_played += 1;
                    }
                    (None, _) => self.debug_note_ons_missing_patch += 1,
                    (Some(_), None) => self.debug_note_ons_missing_wave += 1,
                }
            }
            if !self.player.loaded() {
                break;
            }
        }
    }
}

pub type SharedManager = Arc<Mutex<MidiManager>>;
