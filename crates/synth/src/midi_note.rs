//! `jagex3.midi2.MidiNote` — one playing note. Holds the patch + wave + envelope being
//! played plus the WaveStream voice. Updated each mixer tick by `MidiPlayer.update_note`.

use std::sync::Arc;

use crate::envelope::EnvelopeSet;
use crate::patch::Patch;
use crate::wave::Wave;
use crate::wave_stream::WaveStream;

pub struct MidiNote {
    pub channel: usize,
    pub patch: Option<Arc<Patch>>,
    pub sound: Option<Arc<Wave>>,
    pub envelope: Option<Arc<EnvelopeSet>>,
    pub secondary_note: i32,
    pub note_key: usize,
    pub volume: i32,
    pub pan: i32,
    pub pitch: i32,
    pub portamento_delta: i32,
    pub portamento_amount: i32,
    pub decay_progress: i32,
    pub attack_progress: i32,
    pub attack_envelope_progress: usize,
    pub release_progress: i32,
    pub release_envelope_progress: usize,
    pub vibrato_ramp_progress: i32,
    pub vibrato_progress: i32,
    pub stream: Option<WaveStream>,
    pub volume_change_duration: i32,
    pub field1766: i32,
    pub finished: bool,
}

impl MidiNote {
    pub fn new() -> Self {
        Self {
            channel: 0,
            patch: None,
            sound: None,
            envelope: None,
            secondary_note: 0,
            note_key: 0,
            volume: 0,
            pan: 0,
            pitch: 0,
            portamento_delta: 0,
            portamento_amount: 0,
            decay_progress: 0,
            attack_progress: 0,
            attack_envelope_progress: 0,
            release_progress: -1,
            release_envelope_progress: 0,
            vibrato_ramp_progress: 0,
            vibrato_progress: 0,
            stream: None,
            volume_change_duration: 0,
            field1766: 0,
            finished: false,
        }
    }

    pub fn drop_data(&mut self) {
        self.patch = None;
        self.sound = None;
        self.envelope = None;
        self.stream = None;
    }
}
