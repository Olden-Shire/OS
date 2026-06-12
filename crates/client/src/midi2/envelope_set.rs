// @ObfuscatedName("ax")
// jag::oldscape::midi2::EnvelopeSet
//
// Per-note ADSR envelope tables (attack-volume, release-volume, decay,
// vibrato). Populated by Patch::decode while parsing the cache patch
// record. Used by MidiPlayer to apply envelope progression to each
// MidiNote's WaveStream.

#![allow(dead_code)]

#[derive(Debug, Default, Clone)]
pub struct EnvelopeSet {
    // @ObfuscatedName("ax.r") — `[time_lo, level, ...]` pairs.
    pub attack_volume: Option<Vec<i8>>,
    // @ObfuscatedName("ax.d") — `[time, level, ...]` pairs, prefixed (0, 64).
    pub release_volume: Option<Vec<i8>>,
    // @ObfuscatedName("ax.l")
    pub decay_volume: i32,
    // @ObfuscatedName("ax.m")
    pub attack_speed: i32,
    // @ObfuscatedName("ax.c")
    pub release_speed: i32,
    // @ObfuscatedName("ax.n")
    pub decay_speed: i32,
    // @ObfuscatedName("ax.j")
    pub vibrato_amplitude: i32,
    // @ObfuscatedName("ax.z")
    pub vibrato_frequency: i32,
    // @ObfuscatedName("ax.g")
    pub vibrato_ramp_time: i32,
}
