// @ObfuscatedName("ai")
// jag::oldscape::midi2::MidiNote
//
// One playing note. Holds the patch + wave + envelope reference plus
// the WaveStream voice rendering it. Updated each mixer tick by
// MidiPlayer::render.

#![allow(dead_code)]

use std::sync::Arc;

use crate::midi2::envelope_set::EnvelopeSet;
use crate::midi2::patch::Patch;
use crate::sound::wave::Wave;
use crate::sound::wave_stream::WaveStream;

pub struct MidiNote {
    // @ObfuscatedName("ai.r")
    pub channel: usize,
    // @ObfuscatedName("ai.d")
    pub patch: Option<Arc<Patch>>,
    // @ObfuscatedName("ai.l")
    pub sound: Option<Arc<Wave>>,
    // @ObfuscatedName("ai.m")
    pub envelope: Option<Arc<EnvelopeSet>>,
    // @ObfuscatedName("ai.c")
    pub secondary_note: i32,
    // @ObfuscatedName("ai.n")
    pub note_key: usize,
    // @ObfuscatedName("ai.j")
    pub volume: i32,
    // @ObfuscatedName("ai.z")
    pub pan: i32,
    // @ObfuscatedName("ai.g")
    pub pitch: i32,
    // @ObfuscatedName("ai.q")
    pub portamento_delta: i32,
    // @ObfuscatedName("ai.i")
    pub portamento_amount: i32,
    // @ObfuscatedName("ai.s")
    pub decay_progress: i32,
    // @ObfuscatedName("ai.u")
    pub attack_progress: i32,
    // @ObfuscatedName("ai.v")
    pub attack_envelope_progress: usize,
    // @ObfuscatedName("ai.w")
    pub release_progress: i32,
    // @ObfuscatedName("ai.e")
    pub release_envelope_progress: usize,
    // @ObfuscatedName("ai.b")
    pub vibrato_ramp_progress: i32,
    // @ObfuscatedName("ai.y")
    pub vibrato_progress: i32,
    // @ObfuscatedName("ai.t")
    pub stream: Option<WaveStream>,
    // @ObfuscatedName("ai.f")
    pub volume_change_duration: i32,
    // @ObfuscatedName("ai.k") — field1766 in deob source
    pub field1766: i32,
    // @ObfuscatedName("ai.o")
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

    // @ObfuscatedName("ej.c(B)V") — MidiNote.dropData. Verbatim port
    // of MidiNote.java:76-81. Called during note release; clears the
    // four hot Arc references so the patch/wave/envelope/stream can
    // be reclaimed by the cache LRU on the next eviction tick.
    pub fn drop_data(&mut self) {
        self.patch = None;
        self.sound = None;
        self.envelope = None;
        self.stream = None;
    }
}
