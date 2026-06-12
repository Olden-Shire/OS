//! `jagex3.midi2.MidiPlayer` — the synth engine. Port of `MidiPlayer.java`.
//!
//! Manages 16 MIDI channels of state (program/pan/volume/pitchbend/effects), walks the
//! parsed standard MIDI track-by-track at the appropriate sample tempo, and emits stereo
//! audio frames into a caller-provided buffer.
//!
//! Major simplifications from the Java source kept for sanity:
//! - We don't implement the retrigger effect (channel CCs 16/17/48/49/81). It needs the
//!   field1766 timekeeper and a second stream-replacement path; can land later.
//! - WaveStream's 16-variant inner mixer is reduced to a single linear-interpolation
//!   stereo mixer with ramping (see [`crate::wave_stream`]).
//! - Note unlinking uses index-based removal from `notes: Vec<MidiNote>` rather than
//!   Java's `Linkable` doubly-linked list. Same effect, simpler ownership.

use std::collections::HashMap;
use std::sync::Arc;

use crate::midi_note::MidiNote;
use crate::parser::MidiParser;
use crate::patch::Patch;
use crate::wave::Wave;
use crate::wave_stream::WaveStream;

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
        // GM convention: channel 10 (index 9) is drums, bank 128.
        // MidiFile::discover_patches relies on this so the pre-resolved wave table is
        // keyed by drum patch IDs (128 + program). Without this match, drum note-ons at
        // playback look up patch 0+prog instead of 128+prog and never find their waves.
        channel_default_patch[9] = 128;
        channel_bank[9] = 128;
        let mut me = Self {
            frequency,
            stereo,
            patches: HashMap::new(),
            global_volume: 256,
            tempo_us: 1_000_000,
            channel_expression: [0; 16],
            channel_pan: [0; 16],
            channel_volume: [0; 16],
            channel_default_patch,
            channel_patch: [0; 16],
            channel_bank,
            channel_pitch_bend: [0; 16],
            channel_modulation: [0; 16],
            channel_portamento_time: [0; 16],
            channel_effects: [0; 16],
            channel_parameter_number: [0; 16],
            channel_pitch_bend_range: [0; 16],
            channel_notes: (0..16).map(|_| [None; 128]).collect(),
            channel_secondary_notes: (0..16).map(|_| [None; 128]).collect(),
            notes: Vec::new(),
            parser: MidiParser::new(),
            loop_song: false,
            track: 0,
            track_current_tick: 0,
            track_previous_time: 0,
            track_current_time: 0,
        };
        me.reset();
        me
    }

    pub fn set_global_volume(&mut self, v: i32) {
        self.global_volume = v;
    }

    pub fn global_volume(&self) -> i32 {
        self.global_volume
    }

    pub fn install_patch(&mut self, id: i32, patch: Arc<Patch>) {
        self.patches.insert(id, patch);
    }

    pub fn loaded(&self) -> bool {
        self.parser.got_midi()
    }

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
        self.notes.retain(|n| chan.is_some() && Some(n.channel) != chan);
        for ch in 0..16 {
            if chan.is_none() || chan == Some(ch) {
                self.channel_notes[ch] = [None; 128];
            }
        }
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
            for c in 0..16 {
                self.all_controllers_off(Some(c));
            }
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
        if self.channel_patch[ch] == prog {
            return;
        }
        self.channel_patch[ch] = prog;
        for k in 0..128 {
            self.channel_secondary_notes[ch][k] = None;
        }
    }

    fn play_note(&mut self, ch: usize, key: usize, vel: i32) {
        self.stop_note(ch, key, 64);
        let Some(patch) = self.patches.get(&self.channel_patch[ch]).cloned() else {
            return;
        };
        let Some(wave_idx) = (key < 128).then_some(key) else { return };
        // Patch.note_sound[] is populated lazily during loadWaves; we hold Arc<Patch> here
        // so we can't mutate. The Wave Arc lives in the MidiManager's WaveCache and gets
        // attached to MidiNote when play_note runs (after load_and_queue_patches).
        let _ = wave_idx; // placeholder — wave attachment is in play_note_with_wave below.
        let _ = patch;
        let _ = vel;
    }

    /// Play a note that already has a Wave resolved by the manager (after Patch.load_waves
    /// would have run). Splits from `play_note` so the manager owns the patch→wave map.
    pub fn play_note_with_wave(
        &mut self,
        ch: usize,
        key: usize,
        vel: i32,
        patch: Arc<Patch>,
        wave: Arc<Wave>,
    ) {
        self.stop_note(ch, key, 64);
        let envelope = patch.envelopes.get(patch.note_envelope[key]).cloned();
        let envelope = envelope.map(Arc::new);
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
            if let Some(s) = note.stream.as_mut() {
                s.set_loop_count(-1);
            }
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
        if let Some(sk) = secondary_key {
            self.channel_secondary_notes[ch][sk] = Some(idx);
        }
    }

    fn stop_note(&mut self, ch: usize, key: usize, _vel: i32) {
        let Some(idx) = self.channel_notes[ch][key].take() else { return };
        if let Some(n) = self.notes.get_mut(idx) {
            n.release_progress = 0;
        }
    }

    fn pitch_wheel(&mut self, ch: usize, value: i32) {
        self.channel_pitch_bend[ch] = value;
    }

    fn get_rate_raw(&self, note: &MidiNote, patch: &Patch) -> i32 {
        let mut p = (note.portamento_amount * note.portamento_delta >> 12) + note.pitch;
        p += (self.channel_pitch_bend[note.channel] - 8192) * self.channel_pitch_bend_range[note.channel] >> 12;
        let env = patch.envelopes.get(patch.note_envelope[note.note_key]);
        if let Some(env) = env {
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
            / self.frequency as f64
            + 0.5) as i32;
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
        if v < 8192 {
            (note.pan * v + 32) >> 6
        } else {
            16384 - (((128 - note.pan) * (16384 - v) + 32) >> 6)
        }
    }

    fn process_midi(&mut self, packed: i32) {
        let kind = packed & 0xF0;
        let ch = (packed & 0xF) as usize;
        let d1 = (packed >> 8) & 0x7F;
        let d2 = (packed >> 16) & 0x7F;
        match kind {
            0x80 => self.stop_note(ch, d1 as usize, d2),
            0x90 => {
                if d2 > 0 { /* play_note happens via manager - see public play_note_with_wave */
                } else {
                    self.stop_note(ch, d1 as usize, 64);
                }
            }
            0xA0 => { /* poly aftertouch — ignored */ }
            0xB0 => self.process_cc(ch, d1, d2),
            0xC0 => self.set_inst(ch, self.channel_bank[ch] + d1),
            0xD0 => { /* channel pressure — ignored */ }
            0xE0 => self.pitch_wheel(ch, d1 + (d2 << 7)),
            _ => if (packed & 0xFF) == 0xFF { self.reset(); },
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
            7 => self.channel_expression[ch] = (val << 7) + (self.channel_expression[ch] & 0xFFFF_C07Fu32 as i32),
            39 => self.channel_expression[ch] = (self.channel_expression[ch] & 0xFFFF_FF80u32 as i32) + val,
            10 => self.channel_pan[ch] = (val << 7) + (self.channel_pan[ch] & 0xFFFF_C07Fu32 as i32),
            42 => self.channel_pan[ch] = (self.channel_pan[ch] & 0xFFFF_FF80u32 as i32) + val,
            11 => self.channel_volume[ch] = (val << 7) + (self.channel_volume[ch] & 0xFFFF_C07Fu32 as i32),
            43 => self.channel_volume[ch] = (self.channel_volume[ch] & 0xFFFF_FF80u32 as i32) + val,
            64 => {
                if val >= 64 { self.channel_effects[ch] |= 0x1; } else { self.channel_effects[ch] &= !0x1; }
            }
            65 => {
                if val >= 64 { self.channel_effects[ch] |= 0x2; } else { self.channel_effects[ch] &= !0x2; }
            }
            99 => self.channel_parameter_number[ch] = (val << 7) + (self.channel_parameter_number[ch] & 0x7F),
            98 => self.channel_parameter_number[ch] = (self.channel_parameter_number[ch] & 0x3F80) + val,
            101 => self.channel_parameter_number[ch] = (val << 7) + (self.channel_parameter_number[ch] & 0x7F) + 16384,
            100 => self.channel_parameter_number[ch] = (self.channel_parameter_number[ch] & 0x3F80) + 16384 + val,
            120 => self.all_sound_off(Some(ch)),
            121 => self.all_controllers_off(Some(ch)),
            123 => self.all_notes_off(Some(ch)),
            6 => if self.channel_parameter_number[ch] == 16384 {
                self.channel_pitch_bend_range[ch] = (val << 7) + (self.channel_pitch_bend_range[ch] & 0xFFFF_C07Fu32 as i32);
            },
            38 => if self.channel_parameter_number[ch] == 16384 {
                self.channel_pitch_bend_range[ch] = (self.channel_pitch_bend_range[ch] & 0xFFFF_FF80u32 as i32) + val;
            },
            _ => {}
        }
    }

    /// Pull the next packed event(s) from the parser at the current track. Returns the
    /// list of `0x9X` note-on events that need to be played (their wave isn't resolved
    /// inside MidiPlayer because the patch→wave mapping lives in the manager). All other
    /// events are processed inline.
    #[must_use]
    pub fn update_midi(&mut self) -> Vec<(usize, usize, i32, i32)> {
        let mut note_ons: Vec<(usize, usize, i32, i32)> = Vec::new();
        let mut track = self.track;
        let mut tick = self.track_current_tick;
        let mut time = self.track_current_time;
        while self.track_current_tick == tick {
            while self.parser.track_current_tick[track] == tick {
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

    /// Per-tick note state update + render `n` stereo frames. `out` is interleaved L,R.
    /// The manager calls this after applying any pending note-ons.
    pub fn render(&mut self, out: &mut [i32], n: usize) {
        // Per-100-Hz volume ramp (Java's volumeChangeDuration = frequency / 100).
        let chunk = self.frequency / 100;
        let mut produced = 0usize;
        while produced < n {
            let this_chunk = (n - produced).min(chunk as usize);
            for note_idx in 0..self.notes.len() {
                let (rate, vol, pan, finished_note) = {
                    let tmp_note = std::mem::replace(&mut self.notes[note_idx], MidiNote::new());
                    let patch = tmp_note.patch.as_ref().cloned();
                    let mut finished = false;
                    if patch.is_none() || tmp_note.stream.is_none() {
                        finished = true;
                    }
                    let (rate, vol, pan) = if let Some(patch) = patch.as_ref() {
                        (
                            self.get_rate_raw(&tmp_note, patch),
                            self.get_volume_for(&tmp_note, patch),
                            self.get_pan_for(&tmp_note),
                        )
                    } else {
                        (0, 0, 0)
                    };
                    self.notes[note_idx] = tmp_note;
                    (rate, vol, pan, finished)
                };
                // Skip notes already marked finished in an earlier chunk of this render
                // call — their ramp_out is in flight and we don't want subsequent chunks
                // to call ramp_vol_pan_fine on top of it.
                if finished_note || self.notes[note_idx].finished {
                    continue;
                }
                let env_clone = self.notes[note_idx].envelope.as_ref().map(|e| (**e).clone());
                let channel_effects = self.channel_effects[self.notes[note_idx].channel];
                let secondary = self.notes[note_idx].secondary_note;
                let secondary_replaced = if secondary >= 0 {
                    self.channel_secondary_notes[self.notes[note_idx].channel][secondary as usize]
                        != Some(note_idx)
                } else {
                    true
                };

                let note = &mut self.notes[note_idx];
                if let Some(s) = note.stream.as_mut() {
                    s.set_rate_raw(rate);
                    s.ramp_vol_pan_fine(this_chunk as i32, vol, pan);
                    s.do_mix(out, produced, this_chunk);
                }

                // Envelope progression. Returns true if the note's envelope completed and
                // the stream should be ramped out (Java's `var8`).
                let mut envelope_done = false;
                if let Some(env) = env_clone.as_ref() {
                    note.vibrato_ramp_progress += 1;
                    note.vibrato_progress += env.vibrato_frequency;
                    // Key-factor for envelope-speed exponentiation. Java parses
                    // `(noteKey - 60 << 8) + (portamento >> 12)` — explicit parens around
                    // the shift so + is outside the shift. Replicate here.
                    let key_factor = ((((note.note_key as i32 - 60) << 8)
                        + ((note.portamento_amount * note.portamento_delta) >> 12)) as f64)
                        * 5.086_263_020_833_333e-6;

                    if env.decay_volume > 0 {
                        if env.decay_speed > 0 {
                            note.decay_progress +=
                                (2.0_f64.powf(env.decay_speed as f64 * key_factor) * 128.0 + 0.5) as i32;
                        } else {
                            note.decay_progress += 128;
                        }
                    }
                    if let Some(av) = env.attack_volume.as_ref() {
                        if env.attack_speed > 0 {
                            note.attack_progress +=
                                (2.0_f64.powf(env.attack_speed as f64 * key_factor) * 128.0 + 0.5) as i32;
                        } else {
                            note.attack_progress += 128;
                        }
                        while note.attack_envelope_progress < av.len() - 2
                            && note.attack_progress
                                > ((av[note.attack_envelope_progress + 2] as i32 & 0xFF) << 8)
                        {
                            note.attack_envelope_progress += 2;
                        }
                        // Java: if attack reached end at level 0 → envelope is done.
                        if note.attack_envelope_progress == av.len() - 2
                            && av[note.attack_envelope_progress + 1] == 0
                        {
                            envelope_done = true;
                        }
                    }
                    // Release envelope progresses only when the note has been note-off'd
                    // AND sustain pedal (CC64, effect bit 0x1) is OFF, AND this note hasn't
                    // been retriggered as someone else's secondary note.
                    if note.release_progress >= 0
                        && env.release_volume.is_some()
                        && (channel_effects & 0x1) == 0
                        && secondary_replaced
                    {
                        let rv = env.release_volume.as_ref().unwrap();
                        if env.release_speed > 0 {
                            note.release_progress +=
                                (2.0_f64.powf(env.release_speed as f64 * key_factor) * 128.0 + 0.5) as i32;
                        } else {
                            note.release_progress += 128;
                        }
                        while note.release_envelope_progress < rv.len() - 2
                            && note.release_progress
                                > ((rv[note.release_envelope_progress + 2] as i32 & 0xFF) << 8)
                        {
                            note.release_envelope_progress += 2;
                        }
                        if note.release_envelope_progress == rv.len() - 2 {
                            envelope_done = true;
                        }
                    }
                }
                if envelope_done {
                    // Ramp the stream to zero over this chunk, then mark the note finished
                    // so the cleanup pass removes it.
                    if let Some(s) = self.notes[note_idx].stream.as_mut() {
                        s.ramp_out(this_chunk as i32);
                    }
                    self.notes[note_idx].finished = true;
                }
            }
            produced += this_chunk;
        }
        // Drop finished notes (and their channel_notes/channel_secondary_notes pointers).
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
            // Adjust remaining indices in channel_notes/secondary tables.
            for slot in self.channel_notes.iter_mut() {
                for entry in slot.iter_mut() {
                    if let Some(idx) = entry {
                        if *idx > i {
                            *idx -= 1;
                        } else if *idx == i {
                            *entry = None;
                        }
                    }
                }
            }
            for slot in self.channel_secondary_notes.iter_mut() {
                for entry in slot.iter_mut() {
                    if let Some(idx) = entry {
                        if *idx > i {
                            *idx -= 1;
                        } else if *idx == i {
                            *entry = None;
                        }
                    }
                }
            }
        }
    }

    /// How many frames advance per tick — derived from tempo and sample rate.
    pub fn samples_per_tempo_unit(&self) -> i32 {
        self.tempo_us * self.parser.division / self.frequency
    }
}
