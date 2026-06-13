// @ObfuscatedName("ad")
// jag::oldscape::midi2::MidiManager — orchestration.
//
// Loads a song, materialises its patches via the patches archive, resolves
// wave references via WaveCache, drives playback through MidiPlayer. The
// PcmPlayer's cpal callback locks the SharedManager and calls render.

#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;

use crate::midi2::midi_file::MidiFile;
use crate::midi2::midi_player::MidiPlayer;
use crate::midi2::patch::Patch;
use crate::sound::js5_cache::Js5Cache;
use crate::sound::wave::Wave;
use crate::sound::wave_cache::WaveCache;

// @ObfuscatedName("ad.PATCHES_ARCHIVE")
const PATCHES_ARCHIVE: u8 = 15;

pub struct MidiManager {
    // @ObfuscatedName("ad.r")
    pub player: MidiPlayer,
    // @ObfuscatedName("ad.d")
    pub wave_cache: WaveCache,
    pub wave_table: HashMap<(i32, usize), Arc<Wave>>,
    pub patch_table: HashMap<i32, Arc<Patch>>,
    // Pending song mirroring Java's MidiManager state machine. swap_songs
    // sets this; mainloop calls try_advance_loading each tick until all
    // patches + waves have arrived from JS5.
    pub pending: Option<PendingSong>,
    // @ObfuscatedName("ad.l") — manager state. Java uses three values:
    //   0 = idle / no song
    //   1 = fading out current song (volume drops by `fade_out_rate`
    //       per tick until 0, then swap_songs is allowed)
    //   2 = playing
    pub state: i32,
    // @ObfuscatedName("ad.m") — volume drop per tick during state 1.
    pub fade_out_rate: i32,
    // @ObfuscatedName("ad.c") — saved pending fade-target so a song
    // queued while still fading lands at the right volume.
    pub queued_volume: i32,
    // @ObfuscatedName("ad.n") — the pending song waiting for fade-out
    // to complete before swap_songs flips it active.
    pub queued_pending: Option<PendingSong>,
    // custom — one-shot wave voices riding the same output as the song
    // (jaged's vorbis audition today; the game's play_synth / SOUND_AREA
    // drain can feed this later). Finished voices drop after each render.
    pub sfx: Vec<crate::sound::wave_stream::WaveStream>,
    // Output sample rate — needed to pitch a wave for 1:1 playback.
    pub frequency: i32,
}

pub struct PendingSong {
    pub midi: Vec<u8>,
    pub loop_song: bool,
    pub file: MidiFile,
}

impl MidiManager {
    pub fn new(frequency: i32, stereo: bool) -> Self {
        Self {
            player: MidiPlayer::new(frequency, stereo),
            wave_cache: WaveCache::new(),
            wave_table: HashMap::new(),
            patch_table: HashMap::new(),
            pending: None,
            state: 0,
            fade_out_rate: 0,
            queued_volume: 256,
            queued_pending: None,
            sfx: Vec::new(),
            frequency,
        }
    }

    // custom — start a one-shot wave voice at its native rate.
    // `volume` is Java's 0..=255 scale (WaveStream shifts it to the
    // 16384-based fine scale internally); returns false when the wave
    // is empty.
    pub fn play_wave(&mut self, wave: std::sync::Arc<crate::sound::wave::Wave>, volume: i32) -> bool {
        use crate::sound::wave_stream::WaveStream;
        match WaveStream::new_rate_percent_full(wave, 100, volume, self.frequency) {
            Some(ws) => {
                self.sfx.push(ws);
                true
            }
            None => false,
        }
    }

    // custom — drop every active one-shot voice immediately.
    pub fn stop_waves(&mut self) {
        self.sfx.clear();
    }

    // @ObfuscatedName("by.j(I)V") — MidiManager.updateFadeOut, Java-
    // verbatim (MidiManager.java:117-149). Called every mainloop tick.
    // state 1: while the player is loaded and volume > 0, drop volume
    // by fade_out_rate; once silent (or nothing playing), stop + clear
    // patches and move to state 2 (loading) if a song is queued, else
    // state 0 (idle). Volume is restored to queued_volume when the
    // load completes (Java updateLoading:169).
    pub fn update_fade_out(&mut self) {
        if self.state != 1 { return; }
        let cur = self.player.global_volume;
        if cur > 0 && self.player.loaded() {
            self.player.set_global_volume((cur - self.fade_out_rate).max(0));
            return;
        }
        self.player.stop();
        self.wave_table.clear();
        self.patch_table.clear();
        if let Some(pending) = self.queued_pending.take() {
            self.pending = Some(pending);
            self.state = 2;
        } else {
            self.state = 0;
        }
    }

    // @ObfuscatedName("ad.d(IB)V") — MidiManager.clearPatches.
    // Drops every wave/patch reference; the next play call has to
    // re-download from the patches archive.
    pub fn clear_patches(&mut self) {
        self.wave_table.clear();
        self.patch_table.clear();
    }

    // @ObfuscatedName("ad.swapSongs") — MidiManager.swapSongs(priority, songs, group, file, vol, loop)
    //
    // Loads a song: decodes the MIDI bytes, discovers required patches,
    // fetches each patch from the patches archive, resolves its waves
    // through WaveCache, and queues them on the player.
    // Java swapSongs(fadeRate, songs, group, file, vol, loop) only
    // records the pending song and enters state 1 — the OLD song keeps
    // playing and fades by fade_rate per tick (updateFadeOut); the new
    // one loads after the fade completes. (Java defers the MIDI decode
    // to updateLoading; we decode up front — same bytes either way.)
    pub fn swap_songs(&mut self, fade_rate: i32, raw_midi: Vec<u8>, loop_song: bool) {
        eprintln!("[audio] swap_songs: raw={} bytes, fade_rate={fade_rate}", raw_midi.len());
        let midi = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::midi2::jagex_codec::decode(&raw_midi)
        })) {
            Ok(m) => m,
            Err(_) => {
                eprintln!("[audio] swap_songs: Jagex MIDI decode panicked");
                return;
            }
        };
        let mut file = MidiFile::from_standard(midi.clone());
        file.discover_patches();
        // pendingVolume: Java callers pass the midiVolume option; our
        // callers don't carry it, so capture the pre-fade volume once
        // (a re-queue mid-fade keeps the original target).
        if self.state != 1 {
            self.queued_volume = self.player.global_volume;
        }
        self.state = 1;
        self.fade_out_rate = fade_rate;
        self.queued_pending = Some(PendingSong { midi, loop_song, file });
    }

    // Called from mainloop each tick. Polls the cache for all patches and
    // waves the pending song needs and starts the player once ready.
    pub fn try_advance_loading(&mut self) {
        let Some(pending) = self.pending.as_ref() else { return };
        let patches = match pending.file.patches.as_ref() {
            Some(p) => p.clone(),
            None => return,
        };
        let mut cache = Js5Cache::new();
        let mut all_ready = true;
        let mut waves_needed = 0;
        let mut waves_present = 0;
        for (&pid, hits) in &patches {
            if let Some(patch) = self.patch_table.get(&pid) {
                for key in 0..128 {
                    if hits[key] != 0 && patch.note_wave_id[key] != 0 {
                        waves_needed += 1;
                        if self.wave_table.contains_key(&(pid, key)) {
                            waves_present += 1;
                        }
                    }
                }
            }
        }
        for (&pid, hits) in &patches {
            if self.patch_table.contains_key(&pid) {
                // Patch already decoded — verify all its waves are resolved.
                let patch = self.patch_table.get(&pid).cloned().unwrap();
                let mut waves_ready = true;
                for key in 0..128 {
                    if hits[key] != 0 && patch.note_wave_id[key] != 0 {
                        if !self.wave_table.contains_key(&(pid, key)) {
                            if let Some(wave) = self.wave_cache.get_by_wave_id(&mut cache, patch.note_wave_id[key]) {
                                self.wave_table.insert((pid, key), wave);
                            } else {
                                waves_ready = false;
                            }
                        }
                    }
                }
                if !waves_ready { all_ready = false; }
                continue;
            }
            // Patch not yet decoded — try to fetch + decode.
            let bytes = match cache.read_group(PATCHES_ARCHIVE, pid as u32) {
                Ok(Some(b)) if !b.is_empty() => b,
                _ => { all_ready = false; continue; }
            };
            let patch = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| Patch::decode(&bytes))) {
                Ok(p) => Arc::new(p),
                Err(_) => continue,
            };
            for key in 0..128 {
                if hits[key] != 0 && patch.note_wave_id[key] != 0 {
                    if let Some(wave) = self.wave_cache.get_by_wave_id(&mut cache, patch.note_wave_id[key]) {
                        self.wave_table.insert((pid, key), wave);
                    } else {
                        all_ready = false;
                    }
                }
            }
            self.patch_table.insert(pid, Arc::clone(&patch));
            self.player.install_patch(pid, patch);
        }
        if all_ready {
            let pending = self.pending.take().unwrap();
            eprintln!("[audio] try_advance: ALL READY — starting song ({} patches, {} waves)",
                self.patch_table.len(), self.wave_table.len());
            // Java updateLoading:168-171 — restore the pre-fade volume
            // and return to idle once the song actually starts.
            self.player.set_global_volume(self.queued_volume);
            self.player.start(pending.midi, pending.loop_song);
            self.state = 0;
        } else {
            use std::sync::atomic::{AtomicUsize, Ordering};
            static LAST_PRINT: AtomicUsize = AtomicUsize::new(0);
            let counter = LAST_PRINT.fetch_add(1, Ordering::Relaxed);
            if counter % 50 == 0 {
                eprintln!("[audio] try_advance: patches {}/{}, waves {}/{}",
                    self.patch_table.len(), patches.len(), waves_present, waves_needed);
            }
        }
    }

    // @ObfuscatedName("bc.m(B)V") — MidiManager.stop: halt playback
    // now, cancel any queued/in-flight song, and let state 1 fall
    // through to idle on the next updateFadeOut tick (player is no
    // longer loaded, so the fade is skipped). Patches clear there.
    pub fn stop(&mut self) {
        self.player.stop();
        self.state = 1;
        self.queued_pending = None;
        self.pending = None;
    }

    // @ObfuscatedName("i.l(II)V") — MidiManager.setVolume: applies now
    // when idle; mid-swap it retargets the post-fade restore volume.
    pub fn set_volume(&mut self, v: i32) {
        if self.state == 0 {
            self.player.set_global_volume(v);
        } else {
            self.queued_volume = v;
        }
    }

    pub fn render(&mut self, out: &mut [i32], n: usize) {
        for v in out.iter_mut().take(n * 2) { *v = 0; }
        if self.player.loaded() {
            let samples_per_unit = self.player.samples_per_tempo_unit().max(1);
            let mut remaining = n;
            let mut offset = 0usize;
            while remaining > 0 {
                let proposed_time = remaining as i64 * samples_per_unit as i64 + self.player.track_previous_time;
                if self.player.track_current_time - proposed_time >= 0 {
                    self.player.track_previous_time = proposed_time;
                    self.player.render(&mut out[offset * 2..], remaining);
                    break;
                }
                let chunk = ((self.player.track_current_time - self.player.track_previous_time
                    + samples_per_unit as i64 - 1) / samples_per_unit as i64) as i32;
                self.player.track_previous_time += samples_per_unit as i64 * chunk as i64;
                self.player.render(&mut out[offset * 2..], chunk as usize);
                offset += chunk as usize;
                remaining = remaining.saturating_sub(chunk as usize);
                let note_ons = self.player.update_midi();
                for (ch, key, vel, patch_id) in note_ons {
                    let patch = self.patch_table.get(&patch_id).cloned();
                    let wave = self.wave_table.get(&(patch_id, key)).cloned();
                    if let (Some(p), Some(w)) = (patch, wave) {
                        self.player.play_note_with_wave(ch, key, vel, p, w);
                    }
                }
                if !self.player.loaded() { break; }
            }
        }
        // One-shot wave voices mix on top of (or without) the song;
        // finished voices drop here.
        if !self.sfx.is_empty() {
            for ws in &mut self.sfx {
                ws.do_mix(out, 0, n);
            }
            self.sfx.retain(|ws| !ws.is_finished());
        }
    }
}

pub type SharedManager = Arc<Mutex<MidiManager>>;
