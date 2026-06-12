//! `jagex3.midi2.MidiFile` — a loaded song (decoded to standard MIDI) plus the patch
//! set it references.
//!
//! The cache stores songs in a column-oriented format (see [`io::midi`]); decoding to
//! standard MIDI happens once at load time. `discover_patches` walks the events to build
//! the patch → note-set map that `MidiPlayer.load_and_queue_patches` consumes.

use std::collections::BTreeMap;

use crate::parser::MidiParser;

#[derive(Debug, Clone)]
pub struct MidiFile {
    /// Decoded standard MIDI (MThd + MTrk). This is what the player walks during playback.
    pub midi: Vec<u8>,
    /// `patch_id → [128]` bitmap of which note keys the song hits on that patch. `None`
    /// until [`Self::discover_patches`] runs.
    pub patches: Option<BTreeMap<i32, [u8; 128]>>,
}

impl MidiFile {
    /// Construct from a Jagex-format cache buffer (NOT standard MIDI). Decodes to
    /// standard MIDI internally so subsequent playback / discovery hits the standard
    /// representation.
    pub fn from_jagex(jagex_bytes: &[u8]) -> Self {
        Self { midi: io::midi::decode(jagex_bytes), patches: None }
    }

    /// Construct from already-decoded standard MIDI bytes (e.g. when the editor decoded
    /// once for inspection and wants to hand the buffer to the player without re-decoding).
    pub fn from_standard(midi: Vec<u8>) -> Self {
        Self { midi, patches: None }
    }

    pub fn drop_patches(&mut self) {
        self.patches = None;
    }

    /// Walk every track collecting (bank+program → note bitmap). Patches are identified
    /// by `(bank << 0) + program`, where bank is the combined BankMSB(CC0)<<14 + BankLSB(CC32)<<7
    /// running value per channel. Channel 9 is forced to bank `128` (GM percussion).
    ///
    /// Mirrors `MidiFile.method1773` in the Java source.
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
                        channel_bank[ch] =
                            (val << 14) + (channel_bank[ch] & 0xFFE0_3FFFu32 as i32);
                    }
                    if cc == 32 {
                        channel_bank[ch] =
                            (val << 7) + (channel_bank[ch] & 0xFFFF_C07Fu32 as i32);
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
