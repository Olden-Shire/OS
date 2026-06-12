// @ObfuscatedName("ay")
// jag::oldscape::midi2::MidiFile
//
// A loaded song decoded to standard MIDI bytes plus the patch set it
// references. `discover_patches` walks each track building the
// (patch_id → 128-key bitmap) map that MidiManager uses to pre-fetch
// instruments before playback starts.

#![allow(dead_code)]

use std::collections::BTreeMap;

use crate::midi2::midi_parser::MidiParser;

#[derive(Debug, Clone)]
pub struct MidiFile {
    // @ObfuscatedName("ay.r") — standard MIDI (MThd + MTrk) buffer.
    pub midi: Vec<u8>,
    // @ObfuscatedName("ay.d") — (patch_id → 128-key bitmap). None until discover_patches runs.
    pub patches: Option<BTreeMap<i32, [u8; 128]>>,
}

impl MidiFile {
    // Jagex's songs archive stores a custom column-oriented format. Decoding
    // happens elsewhere; the constructor here takes already-standard MIDI.
    pub fn from_standard(midi: Vec<u8>) -> Self {
        Self { midi, patches: None }
    }

    pub fn drop_patches(&mut self) {
        self.patches = None;
    }

    // @ObfuscatedName("ay.method1773") — MidiFile.discoverPatches
    pub fn discover_patches(&mut self) {
        if self.patches.is_some() {
            return;
        }
        let mut patches: BTreeMap<i32, [u8; 128]> = BTreeMap::new();
        let mut channel_bank = [0i32; 16];
        let mut channel_patch = [0i32; 16];
        channel_bank[9] = 128;
        channel_patch[9] = 128;

        let mut parser = MidiParser::with_midi(self.midi.clone());
        let track_count = parser.track_count();
        for t in 0..track_count {
            parser.set_track(t);
            parser.process_delta_time(t);
            parser.unset_track(t);
        }

        'outer: loop {
            let t = match parser.next_track_to_play() {
                Some(t) => t,
                None => break 'outer,
            };
            let tick = parser.track_current_tick[t];
            while parser.track_current_tick[t] == tick {
                parser.set_track(t);
                let ev = parser.get_event(t);
                if ev == 1 {
                    parser.finish_track();
                    parser.unset_track(t);
                    if parser.all_tracks_finished() {
                        break 'outer;
                    }
                    continue 'outer;
                }
                let kind = ev & 0xF0;
                if kind == 0xB0 {
                    let ch = (ev & 0xF) as usize;
                    let cc = (ev >> 8) & 0x7F;
                    let val = (ev >> 16) & 0x7F;
                    if cc == 0 {
                        channel_bank[ch] = (val << 14) + (channel_bank[ch] & 0xFFE0_3FFFu32 as i32);
                    }
                    if cc == 32 {
                        channel_bank[ch] = (val << 7) + (channel_bank[ch] & 0xFFFF_C07Fu32 as i32);
                    }
                } else if kind == 0xC0 {
                    let ch = (ev & 0xF) as usize;
                    let prog = (ev >> 8) & 0x7F;
                    channel_patch[ch] = channel_bank[ch] + prog;
                } else if kind == 0x90 {
                    let ch = (ev & 0xF) as usize;
                    let key = ((ev >> 8) & 0x7F) as usize;
                    let vel = (ev >> 16) & 0x7F;
                    if vel > 0 {
                        let patch_id = channel_patch[ch];
                        let entry = patches.entry(patch_id).or_insert([0u8; 128]);
                        entry[key] = 1;
                    }
                }
                parser.process_delta_time(t);
                parser.unset_track(t);
            }
        }
        self.patches = Some(patches);
    }
}
