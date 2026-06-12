// @ObfuscatedName("aj")
// jag::oldscape::midi2::MidiPlayer — the synth engine.
//
// Manages 16 MIDI channels of state (program/pan/volume/pitchbend/effects),
// walks the parsed standard MIDI track-by-track at the appropriate sample
// tempo, and emits stereo audio frames into a caller-provided buffer.

#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::Arc;

use crate::midi2::midi_note::MidiNote;
use crate::midi2::midi_parser::MidiParser;
use crate::midi2::patch::Patch;
use crate::sound::wave::Wave;
use crate::sound::wave_stream::WaveStream;

pub struct MidiPlayer {
    pub frequency: i32,
    pub stereo: bool,
    pub patches: HashMap<i32, Arc<Patch>>,
    pub global_volume: i32,
    pub tempo_us: i32,
    pub channel_expression: [i32; 16],
    pub channel_pan: [i32; 16],
    pub channel_volume: [i32; 16],
    pub channel_default_patch: [i32; 16],
    pub channel_patch: [i32; 16],
    pub channel_bank: [i32; 16],
    pub channel_pitch_bend: [i32; 16],
    pub channel_modulation: [i32; 16],
    pub channel_portamento_time: [i32; 16],
    pub channel_effects: [i32; 16],
    pub channel_parameter_number: [i32; 16],
    pub channel_pitch_bend_range: [i32; 16],
    // CC 6/38 Data Entry — value written to the currently-selected
    // RPN or NRPN parameter.
    pub channel_data_entry: [i32; 16],
    // CC 16/48 Custom1 — filter cutoff / RetrigEffect anchor; widely
    // re-purposed by the OSRS synth.
    pub channel_custom1: [i32; 16],
    // CC 17/49 retrigger rate (10ms periods); paired with CC 81 ON
    // bit in channel_effects.
    pub channel_retrig_rate: [i32; 16],
    // Companion lookup written by set_retrig_rate from channel_retrig_rate.
    // @ObfuscatedName("ed.channelCustom2/3") — Java's MidiPlayer.java:577
    // stores the retrig rate raw + a precomputed exp curve so the inner
    // mix loop doesn't recompute the pow each sample.
    pub channel_custom2: [i32; 16],
    pub channel_custom3: [i32; 16],
    // CC 99/98 NRPN parameter select (coarse / fine). Most commonly
    // OSRS uses NRPN 1:21 for the OSRS-only "custom synth" knob.
    pub channel_nrpn: [i32; 16],
    // CC 101/100 RPN parameter select (coarse / fine). RPN 0/0 is
    // pitch-bend range; RPN 0/1 is fine tuning; CC 6/38 writes here.
    pub channel_rpn: [i32; 16],
    pub channel_notes: Vec<[Option<usize>; 128]>,
    pub channel_secondary_notes: Vec<[Option<usize>; 128]>,
    pub notes: Vec<MidiNote>,
    pub parser: MidiParser,
    pub loop_song: bool,
    pub track: usize,
    pub track_current_tick: i32,
    pub track_previous_time: i64,
    pub track_current_time: i64,
}

impl MidiPlayer {
    pub fn new(frequency: i32, stereo: bool) -> Self {
        let mut channel_default_patch = [0; 16];
        let mut channel_bank = [0; 16];
        channel_default_patch[9] = 128;
        channel_bank[9] = 128;
        let mut me = Self {
            frequency, stereo,
            patches: HashMap::new(),
            global_volume: 256,
            tempo_us: 1_000_000,
            channel_expression: [0; 16], channel_pan: [0; 16], channel_volume: [0; 16],
            channel_default_patch, channel_patch: [0; 16], channel_bank,
            channel_pitch_bend: [0; 16], channel_modulation: [0; 16],
            channel_portamento_time: [0; 16], channel_effects: [0; 16],
            channel_parameter_number: [0; 16], channel_pitch_bend_range: [0; 16],
            channel_data_entry: [0; 16], channel_custom1: [0; 16],
            channel_retrig_rate: [0; 16],
            channel_custom2: [0; 16], channel_custom3: [0; 16],
            channel_nrpn: [0; 16], channel_rpn: [0; 16],
            channel_notes: (0..16).map(|_| [None; 128]).collect(),
            channel_secondary_notes: (0..16).map(|_| [None; 128]).collect(),
            notes: Vec::new(),
            parser: MidiParser::new(),
            loop_song: false,
            track: 0, track_current_tick: 0,
            track_previous_time: 0, track_current_time: 0,
        };
        me.reset();
        me
    }

    pub fn set_global_volume(&mut self, v: i32) { self.global_volume = v; }
    // @ObfuscatedName(— MidiPlayer.getGlobalVolume). Verbatim accessor.
    pub fn get_global_volume(&self) -> i32 { self.global_volume }
    pub fn install_patch(&mut self, id: i32, patch: Arc<Patch>) { self.patches.insert(id, patch); }
    pub fn loaded(&self) -> bool { self.parser.got_midi() }

    pub fn start(&mut self, midi: Vec<u8>, loop_song: bool) {
        self.stop();
        self.parser.set_midi(midi);
        self.loop_song = loop_song;
        self.track_previous_time = 0;
        let n = self.parser.track_count();
        for t in 0..n {
            self.parser.set_track(t);
            self.parser.process_delta_time(t);
            self.parser.unset_track(t);
        }
        if let Some(t) = self.parser.next_track_to_play() {
            self.track = t;
            self.track_current_tick = self.parser.track_current_tick[t];
            self.track_current_time = self.parser.time_from_tick(self.track_current_tick);
        }
    }

    pub fn stop(&mut self) {
        self.parser.drop_midi();
        self.reset();
    }

    pub fn reset(&mut self) {
        self.all_sound_off(None);
        for ch in 0..16 {
            self.all_controllers_off(Some(ch));
        }
        for ch in 0..16 {
            self.channel_patch[ch] = self.channel_default_patch[ch];
            self.channel_bank[ch] = self.channel_default_patch[ch] & 0xFFFF_FF80u32 as i32;
        }
    }

    fn all_sound_off(&mut self, chan: Option<usize>) {
        // Java's all_sound_off ramps the WaveStream voices out over
        // ~10ms (`stream.rampOut(frequency / 100)`) before dropping
        // them, avoiding the audible click that an instant cut
        // produces. Since the Rust voice model still mixes notes
        // directly through MidiPlayer.notes (no per-voice WaveStream
        // yet) we approximate by zeroing the per-note release
        // progress so the synth fades on the very next sample.
        for n in &mut self.notes {
            if chan.is_none() || chan == Some(n.channel) {
                if n.release_progress < 0 {
                    n.release_progress = 0;
                }
            }
        }
        self.notes.retain(|n| chan.is_some() && Some(n.channel) != chan);
        for ch in 0..16 {
            if chan.is_none() || chan == Some(ch) {
                self.channel_notes[ch] = [None; 128];
            }
        }
    }

    // @ObfuscatedName("ed.au(IB)V") — MidiPlayer.cleanPorta. Verbatim
    // port of MidiPlayer.java:398-408. When CC65 (channel_effects bit
    // 0x2 = porta-glide hold) turns off, any held notes on the channel
    // that no longer have a live note slot get their release_progress
    // bumped to 0 so they start fading out next mix tick.
    pub fn clean_porta(&mut self, ch: usize) {
        if (self.channel_effects[ch] & 0x2) == 0 { return; }
        for n in &mut self.notes {
            if n.channel == ch
                && self.channel_notes[ch][n.note_key].is_none()
                && n.release_progress < 0
            {
                n.release_progress = 0;
            }
        }
    }

    // @ObfuscatedName("ed.ax(IB)V") — MidiPlayer.cleanRetrig. Verbatim
    // port of MidiPlayer.java:412-421. When CC81 (channel_effects bit
    // 0x4 = retrigger effect) turns off, zero every voice's retrig
    // phase counter (field1766) on the channel so the next CC81-on
    // restart starts from phase zero.
    pub fn clean_retrig(&mut self, ch: usize) {
        if (self.channel_effects[ch] & 0x4) == 0 { return; }
        for n in &mut self.notes {
            if n.channel == ch {
                n.field1766 = 0;
            }
        }
    }

    // @ObfuscatedName("ed.ay(Lej;ZI)V") — MidiPlayer.setSampleOffset.
    // Verbatim port of MidiPlayer.java:272-287. Resolves the CC16/48
    // custom1 value into a sample position inside the WaveStream's
    // backing samples. The `reverse_wrap` arg (Java's `arg1`) lets a
    // reversed-loop sample wrap past the end and flip the stream's
    // playback direction.
    pub fn set_sample_offset(&mut self, note_idx: usize, reverse_wrap: bool) {
        let Some(note) = self.notes.get_mut(note_idx) else { return; };
        let Some(sound) = note.sound.clone() else { return; };
        let len = sound.samples.len() as i32;
        let ch = note.channel;
        let pos: i32;
        if reverse_wrap && sound.loop_reversed {
            let span = len + len - sound.loop_start_position;
            let mut p = ((self.channel_custom1[ch] as i64 * span as i64) >> 6) as i32;
            let end = len << 8;
            if p >= end {
                p = end + end - 1 - p;
                if let Some(s) = note.stream.as_mut() { s.set_reverse(true); }
            }
            pos = p;
        } else {
            pos = ((self.channel_custom1[ch] as i64 * len as i64) >> 6) as i32;
        }
        if let Some(s) = note.stream.as_mut() { s.set_position(pos); }
    }

    fn all_notes_off(&mut self, chan: Option<usize>) {
        for n in &mut self.notes {
            if (chan.is_none() || chan == Some(n.channel)) && n.release_progress < 0 {
                self.channel_notes[n.channel][n.note_key] = None;
                n.release_progress = 0;
            }
        }
    }

    fn all_controllers_off(&mut self, chan: Option<usize>) {
        let Some(ch) = chan else {
            for c in 0..16 { self.all_controllers_off(Some(c)); }
            return;
        };
        self.channel_expression[ch] = 12800;
        self.channel_pan[ch] = 8192;
        self.channel_volume[ch] = 16383;
        self.channel_pitch_bend[ch] = 8192;
        self.channel_modulation[ch] = 0;
        self.channel_portamento_time[ch] = 8192;
        self.channel_effects[ch] = 0;
        self.channel_parameter_number[ch] = 32767;
        self.channel_pitch_bend_range[ch] = 256;
    }

    fn set_inst(&mut self, ch: usize, prog: i32) {
        if self.channel_patch[ch] == prog { return; }
        self.channel_patch[ch] = prog;
        for k in 0..128 { self.channel_secondary_notes[ch][k] = None; }
    }

    // @ObfuscatedName("ed.as(I)V") — MidiPlayer.clearPatches.
    // Verbatim port of MidiPlayer.java:147-151. Sweeps the per-player
    // patch cache. Java iterates a HashTable and calls patch.unlink();
    // we just clear the HashMap since Arc<Patch> handles cleanup.
    pub fn clear_patches(&mut self) {
        self.patches.clear();
    }

    // @ObfuscatedName("ed.aa(B)V") — MidiPlayer.freeWaveIds. Verbatim
    // port of MidiPlayer.java:140-144. Iterates each cached Patch and
    // calls free_wave_ids on it (releases the per-note wave-id LUT
    // once waves have been resolved through the cache).
    pub fn free_wave_ids(&mut self) {
        for patch in self.patches.values_mut() {
            if let Some(p) = std::sync::Arc::get_mut(patch) {
                p.free_wave_ids();
            }
        }
    }

    // @ObfuscatedName("ed.az(III)V") — MidiPlayer.setPatchAndBank.
    // Verbatim port of MidiPlayer.java:189-194. Combined helper that
    // writes the default patch + bank-aligned high bits then forwards
    // to set_inst. The bank is the upper 25 bits (program & 0xFFFFFF80).
    pub fn set_patch_and_bank(&mut self, ch: usize, prog: i32) {
        self.channel_default_patch[ch] = prog;
        self.channel_bank[ch] = prog & (0xFFFFFF80u32 as i32);
        self.set_inst(ch, prog);
    }

    // @ObfuscatedName("ed.aj(III)V") — MidiPlayer.setRetrigRate.
    // Verbatim port of MidiPlayer.java:575-579. Writes both channel_
    // custom2 (raw rate) and channel_custom3 (precomputed exp curve).
    // The exp formula: pow(2, val * 5.4931640625e-4) * 2097152 + 0.5.
    pub fn set_retrig_rate(&mut self, ch: usize, val: i32) {
        self.channel_custom2[ch] = val;
        let curve = ((val as f64) * 5.4931640625e-4f64).exp2() * 2097152.0 + 0.5;
        self.channel_custom3[ch] = curve as i32;
    }

    pub fn play_note_with_wave(&mut self, ch: usize, key: usize, vel: i32, patch: Arc<Patch>, wave: Arc<Wave>) {
        self.stop_note(ch, key, 64);
        let envelope = patch.envelopes.get(patch.note_envelope[key]).cloned().map(Arc::new);
        let mut note = MidiNote::new();
        note.channel = ch;
        note.note_key = key;
        note.secondary_note = patch.note_secondary[key] as i32;
        note.volume = (patch.volume * vel * vel * patch.note_volume[key] as i32 + 1024) >> 11;
        note.pan = patch.note_pan[key] as i32 & 0xFF;
        note.pitch = ((key as i32) << 8) - (patch.note_pitch[key] as i32 & 0x7FFF);
        note.release_progress = -1;
        let rate = self.get_rate_raw(&note, &patch);
        let vol = self.get_volume_for(&note, &patch);
        let pan = self.get_pan_for(&note);
        note.stream = Some(WaveStream::new_rate_fine_vol_pan(Arc::clone(&wave), rate, vol, pan));
        if patch.note_pitch[key] < 0 {
            if let Some(s) = note.stream.as_mut() { s.set_loop_count(-1); }
        }
        let secondary_key = if note.secondary_note >= 0 { Some(note.secondary_note as usize) } else { None };
        note.patch = Some(Arc::clone(&patch));
        note.sound = Some(wave);
        note.envelope = envelope;
        if let Some(sk) = secondary_key {
            if let Some(prev_idx) = self.channel_secondary_notes[ch][sk] {
                if let Some(prev) = self.notes.get_mut(prev_idx) {
                    if prev.release_progress < 0 {
                        self.channel_notes[ch][prev.note_key] = None;
                        prev.release_progress = 0;
                    }
                }
            }
        }
        let idx = self.notes.len();
        self.notes.push(note);
        self.channel_notes[ch][key] = Some(idx);
        if let Some(sk) = secondary_key { self.channel_secondary_notes[ch][sk] = Some(idx); }
    }

    fn stop_note(&mut self, ch: usize, key: usize, _vel: i32) {
        let Some(idx) = self.channel_notes[ch][key].take() else { return };
        if let Some(n) = self.notes.get_mut(idx) {
            n.release_progress = 0;
        }
    }

    fn pitch_wheel(&mut self, ch: usize, value: i32) { self.channel_pitch_bend[ch] = value; }

    fn get_rate_raw(&self, note: &MidiNote, patch: &Patch) -> i32 {
        let mut p = (note.portamento_amount * note.portamento_delta >> 12) + note.pitch;
        p += (self.channel_pitch_bend[note.channel] - 8192) * self.channel_pitch_bend_range[note.channel] >> 12;
        if let Some(env) = patch.envelopes.get(patch.note_envelope[note.note_key]) {
            if env.vibrato_frequency > 0 && (env.vibrato_amplitude > 0 || self.channel_modulation[note.channel] > 0) {
                let mut amp = env.vibrato_amplitude << 2;
                let ramp = env.vibrato_ramp_time << 1;
                if note.vibrato_ramp_progress < ramp && ramp > 0 {
                    amp = note.vibrato_ramp_progress * amp / ramp;
                }
                let combined = (self.channel_modulation[note.channel] >> 7) + amp;
                let phase = (note.vibrato_progress & 0x1FF) as f64 * 0.012_271_846_303_085_13;
                p += (combined as f64 * phase.sin()) as i32;
            }
        }
        let wave = note.sound.as_ref().map_or(22050, |w| w.sampling_frequency);
        let rate = (wave as f64 * 256.0 * 2.0_f64.powf(p as f64 * 3.255_208_333_333_333e-4)
            / self.frequency as f64 + 0.5) as i32;
        rate.max(1)
    }

    fn get_volume_for(&self, note: &MidiNote, patch: &Patch) -> i32 {
        let v3 = (self.channel_volume[note.channel] * self.channel_expression[note.channel] + 4096) >> 13;
        let v4 = (v3 * v3 + 16384) >> 15;
        let v5 = (note.volume * v4 + 16384) >> 15;
        let mut v6 = (self.global_volume * v5 + 128) >> 8;
        if let Some(env) = patch.envelopes.get(patch.note_envelope[note.note_key]) {
            if env.decay_volume > 0 {
                v6 = (v6 as f64
                    * 0.5_f64.powf(note.decay_progress as f64 * 1.953_125e-5 * env.decay_volume as f64)
                    + 0.5) as i32;
            }
            if let Some(av) = &env.attack_volume {
                let p = note.attack_progress;
                let mut v8 = av[note.attack_envelope_progress + 1] as i32;
                if note.attack_envelope_progress < av.len() - 2 {
                    let lo = (av[note.attack_envelope_progress] as i32 & 0xFF) << 8;
                    let hi = (av[note.attack_envelope_progress + 2] as i32 & 0xFF) << 8;
                    if hi != lo {
                        v8 += (av[note.attack_envelope_progress + 3] as i32 - v8) * (p - lo) / (hi - lo);
                    }
                }
                v6 = (v6 * v8 + 32) >> 6;
            }
            if note.release_progress > 0 {
                if let Some(rv) = &env.release_volume {
                    let p = note.release_progress;
                    let mut v12 = rv[note.release_envelope_progress + 1] as i32;
                    if note.release_envelope_progress < rv.len() - 2 {
                        let lo = (rv[note.release_envelope_progress] as i32 & 0xFF) << 8;
                        let hi = (rv[note.release_envelope_progress + 2] as i32 & 0xFF) << 8;
                        if hi != lo {
                            v12 += (rv[note.release_envelope_progress + 3] as i32 - v12) * (p - lo) / (hi - lo);
                        }
                    }
                    v6 = (v6 * v12 + 32) >> 6;
                }
            }
        }
        v6
    }

    fn get_pan_for(&self, note: &MidiNote) -> i32 {
        let v = self.channel_pan[note.channel];
        if v < 8192 { (note.pan * v + 32) >> 6 } else { 16384 - (((128 - note.pan) * (16384 - v) + 32) >> 6) }
    }

    fn process_midi(&mut self, packed: i32) {
        let kind = packed & 0xF0;
        let ch = (packed & 0xF) as usize;
        let d1 = (packed >> 8) & 0x7F;
        let d2 = (packed >> 16) & 0x7F;
        match kind {
            0x80 => self.stop_note(ch, d1 as usize, d2),
            0x90 => if d2 == 0 { self.stop_note(ch, d1 as usize, 64); },
            0xB0 => self.process_cc(ch, d1, d2),
            0xC0 => { let bank = self.channel_bank[ch]; self.set_inst(ch, bank + d1); }
            0xE0 => self.pitch_wheel(ch, d1 + (d2 << 7)),
            _ => if (packed & 0xFF) == 0xFF { self.reset(); }
        }
    }

    fn process_cc(&mut self, ch: usize, cc: i32, val: i32) {
        match cc {
            0 => self.channel_bank[ch] = (val << 14) + (self.channel_bank[ch] & 0xFFE0_3FFFu32 as i32),
            32 => self.channel_bank[ch] = (val << 7) + (self.channel_bank[ch] & 0xFFFF_C07Fu32 as i32),
            1 => self.channel_modulation[ch] = (val << 7) + (self.channel_modulation[ch] & 0xFFFF_C07Fu32 as i32),
            33 => self.channel_modulation[ch] = (self.channel_modulation[ch] & 0xFFFF_FF80u32 as i32) + val,
            5 => self.channel_portamento_time[ch] = (val << 7) + (self.channel_portamento_time[ch] & 0xFFFF_C07Fu32 as i32),
            37 => self.channel_portamento_time[ch] = (self.channel_portamento_time[ch] & 0xFFFF_FF80u32 as i32) + val,
            // CC 6 / 38 — Data Entry coarse / fine. Java updates the
            // currently-selected RPN (pitch-bend range / fine-tune).
            6 => self.channel_data_entry[ch] = (val << 7) + (self.channel_data_entry[ch] & 0xFFFF_C07Fu32 as i32),
            38 => self.channel_data_entry[ch] = (self.channel_data_entry[ch] & 0xFFFF_FF80u32 as i32) + val,
            7 => self.channel_expression[ch] = (val << 7) + (self.channel_expression[ch] & 0xFFFF_C07Fu32 as i32),
            39 => self.channel_expression[ch] = (self.channel_expression[ch] & 0xFFFF_C07Fu32 as i32) + val,
            10 => self.channel_pan[ch] = (val << 7) + (self.channel_pan[ch] & 0xFFFF_C07Fu32 as i32),
            42 => self.channel_pan[ch] = (self.channel_pan[ch] & 0xFFFF_FF80u32 as i32) + val,
            11 => self.channel_volume[ch] = (val << 7) + (self.channel_volume[ch] & 0xFFFF_C07Fu32 as i32),
            43 => self.channel_volume[ch] = (self.channel_volume[ch] & 0xFFFF_FF80u32 as i32) + val,
            // CC 16/48 — Custom1 (per-channel arbitrary parameter used
            // by the OSRS synth for filter cutoff / RetrigEffect rate).
            16 => self.channel_custom1[ch] = (val << 7) + (self.channel_custom1[ch] & 0xFFFF_C07Fu32 as i32),
            48 => self.channel_custom1[ch] = (self.channel_custom1[ch] & 0xFFFF_FF80u32 as i32) + val,
            // CC 17/49 — Retrigger rate (Java's CC 17 controls the
            // RetrigEffect speed introduced by CC 81).
            17 => self.channel_retrig_rate[ch] = (val << 7) + (self.channel_retrig_rate[ch] & 0xFFFF_C07Fu32 as i32),
            49 => self.channel_retrig_rate[ch] = (self.channel_retrig_rate[ch] & 0xFFFF_FF80u32 as i32) + val,
            64 => if val >= 64 { self.channel_effects[ch] |= 0x1; } else { self.channel_effects[ch] &= !0x1; },
            65 => if val >= 64 { self.channel_effects[ch] |= 0x2; } else { self.channel_effects[ch] &= !0x2; },
            // CC 81 — RetrigEffect on/off bit; CC 17/49 sets its rate.
            81 => if val >= 64 { self.channel_effects[ch] |= 0x4; } else { self.channel_effects[ch] &= !0x4; },
            // CC 98/99/100/101 — NRPN/RPN parameter selection. We
            // capture them into the per-channel state Java uses; the
            // CC 6/38 data-entry pair above writes to the selected
            // parameter.
            98 => self.channel_nrpn[ch] = (self.channel_nrpn[ch] & 0xFFFF_FF80u32 as i32) | val,
            99 => self.channel_nrpn[ch] = (val << 7) | (self.channel_nrpn[ch] & 0x7F),
            100 => self.channel_rpn[ch] = (self.channel_rpn[ch] & 0xFFFF_FF80u32 as i32) | val,
            101 => self.channel_rpn[ch] = (val << 7) | (self.channel_rpn[ch] & 0x7F),
            120 => self.all_sound_off(Some(ch)),
            121 => self.all_controllers_off(Some(ch)),
            // CC 123 — All Notes Off (alias path; same target as
            // 120/121 but the conventional GM channel-wide stop).
            122 | 123 => self.all_notes_off(Some(ch)),
            _ => {}
        }
    }

    #[must_use]
    pub fn update_midi(&mut self) -> Vec<(usize, usize, i32, i32)> {
        let mut note_ons: Vec<(usize, usize, i32, i32)> = Vec::new();
        let mut track = self.track;
        let mut tick = self.track_current_tick;
        let mut time = self.track_current_time;
        while self.track_current_tick == tick {
            while track < self.parser.track_current_tick.len() && self.parser.track_current_tick[track] == tick {
                self.parser.set_track(track);
                let ev = self.parser.get_event(track);
                if ev == 1 {
                    self.parser.finish_track();
                    self.parser.unset_track(track);
                    if self.parser.all_tracks_finished() {
                        if !self.loop_song || tick == 0 {
                            self.reset();
                            self.parser.drop_midi();
                            return note_ons;
                        }
                        self.parser.restart(time);
                    }
                    break;
                }
                if ev & 0x80 != 0 {
                    let kind = ev & 0xF0;
                    if kind == 0x90 {
                        let ch = (ev & 0xF) as usize;
                        let key = ((ev >> 8) & 0x7F) as usize;
                        let vel = (ev >> 16) & 0x7F;
                        if vel > 0 {
                            note_ons.push((ch, key, vel, self.channel_patch[ch]));
                        } else {
                            self.stop_note(ch, key, 64);
                        }
                    } else {
                        self.process_midi(ev);
                    }
                }
                self.parser.process_delta_time(track);
                self.parser.unset_track(track);
            }
            if let Some(t) = self.parser.next_track_to_play() {
                track = t;
                tick = self.parser.track_current_tick[t];
                time = self.parser.time_from_tick(tick);
            } else {
                break;
            }
        }
        self.track = track;
        self.track_current_tick = tick;
        self.track_current_time = time;
        note_ons
    }

    pub fn render(&mut self, out: &mut [i32], n: usize) {
        let chunk = (self.frequency / 100).max(1);
        let mut produced = 0usize;
        while produced < n {
            let this_chunk = (n - produced).min(chunk as usize);
            for note_idx in 0..self.notes.len() {
                let (rate, vol, pan, finished_note) = {
                    let tmp_note = std::mem::replace(&mut self.notes[note_idx], MidiNote::new());
                    let patch = tmp_note.patch.as_ref().cloned();
                    let mut finished = false;
                    if patch.is_none() || tmp_note.stream.is_none() { finished = true; }
                    let (rate, vol, pan) = if let Some(patch) = patch.as_ref() {
                        (self.get_rate_raw(&tmp_note, patch), self.get_volume_for(&tmp_note, patch), self.get_pan_for(&tmp_note))
                    } else { (0, 0, 0) };
                    self.notes[note_idx] = tmp_note;
                    (rate, vol, pan, finished)
                };
                if finished_note || self.notes[note_idx].finished { continue; }
                let env_clone = self.notes[note_idx].envelope.as_ref().map(|e| (**e).clone());
                let channel_effects = self.channel_effects[self.notes[note_idx].channel];
                let secondary = self.notes[note_idx].secondary_note;
                let secondary_replaced = if secondary >= 0 {
                    self.channel_secondary_notes[self.notes[note_idx].channel][secondary as usize] != Some(note_idx)
                } else { true };

                let note = &mut self.notes[note_idx];
                if let Some(s) = note.stream.as_mut() {
                    s.set_rate_raw(rate);
                    s.ramp_vol_pan_fine(this_chunk as i32, vol, pan);
                    s.do_mix(out, produced, this_chunk);
                }

                let mut envelope_done = false;
                if let Some(env) = env_clone.as_ref() {
                    note.vibrato_ramp_progress += 1;
                    note.vibrato_progress += env.vibrato_frequency;
                    let key_factor = ((((note.note_key as i32 - 60) << 8)
                        + ((note.portamento_amount * note.portamento_delta) >> 12)) as f64)
                        * 5.086_263_020_833_333e-6;
                    if env.decay_volume > 0 {
                        if env.decay_speed > 0 {
                            note.decay_progress += (2.0_f64.powf(env.decay_speed as f64 * key_factor) * 128.0 + 0.5) as i32;
                        } else {
                            note.decay_progress += 128;
                        }
                    }
                    if let Some(av) = env.attack_volume.as_ref() {
                        if env.attack_speed > 0 {
                            note.attack_progress += (2.0_f64.powf(env.attack_speed as f64 * key_factor) * 128.0 + 0.5) as i32;
                        } else {
                            note.attack_progress += 128;
                        }
                        while note.attack_envelope_progress < av.len() - 2
                            && note.attack_progress > ((av[note.attack_envelope_progress + 2] as i32 & 0xFF) << 8)
                        {
                            note.attack_envelope_progress += 2;
                        }
                        if note.attack_envelope_progress == av.len() - 2
                            && av[note.attack_envelope_progress + 1] == 0
                        {
                            envelope_done = true;
                        }
                    }
                    if note.release_progress >= 0
                        && env.release_volume.is_some()
                        && (channel_effects & 0x1) == 0
                        && secondary_replaced
                    {
                        let rv = env.release_volume.as_ref().unwrap();
                        if env.release_speed > 0 {
                            note.release_progress += (2.0_f64.powf(env.release_speed as f64 * key_factor) * 128.0 + 0.5) as i32;
                        } else {
                            note.release_progress += 128;
                        }
                        while note.release_envelope_progress < rv.len() - 2
                            && note.release_progress > ((rv[note.release_envelope_progress + 2] as i32 & 0xFF) << 8)
                        {
                            note.release_envelope_progress += 2;
                        }
                        if note.release_envelope_progress == rv.len() - 2 {
                            envelope_done = true;
                        }
                    }
                }
                if envelope_done {
                    if let Some(s) = self.notes[note_idx].stream.as_mut() { s.ramp_out(this_chunk as i32); }
                    self.notes[note_idx].finished = true;
                }
            }
            produced += this_chunk;
        }
        // Drop finished notes
        let mut remove: Vec<usize> = Vec::new();
        for (i, n) in self.notes.iter().enumerate() {
            let stream_done = n.stream.as_ref().map_or(true, |s| s.is_finished());
            if (n.release_progress >= 0 && stream_done) || n.finished {
                remove.push(i);
            }
        }
        for &i in remove.iter().rev() {
            let n = self.notes.remove(i);
            if let Some(slot) = self.channel_notes.get_mut(n.channel) {
                if slot[n.note_key] == Some(i) {
                    slot[n.note_key] = None;
                }
            }
            for slot in self.channel_notes.iter_mut() {
                for entry in slot.iter_mut() {
                    if let Some(idx) = entry {
                        if *idx > i { *idx -= 1; } else if *idx == i { *entry = None; }
                    }
                }
            }
            for slot in self.channel_secondary_notes.iter_mut() {
                for entry in slot.iter_mut() {
                    if let Some(idx) = entry {
                        if *idx > i { *idx -= 1; } else if *idx == i { *entry = None; }
                    }
                }
            }
        }
    }

    pub fn samples_per_tempo_unit(&self) -> i32 {
        self.tempo_us * self.parser.division / self.frequency.max(1)
    }
}
