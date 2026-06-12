//! `jagex3.midi2.EnvelopeSet` — per-note volume / vibrato envelope tables.
//!
//! Used by [`crate::patch::Patch`] and (later) `MidiNote` playback. Members are populated
//! by `Patch::decode` while parsing the cache patch record.

#[derive(Debug, Default, Clone)]
pub struct EnvelopeSet {
    /// Attack envelope as `[time_lo, level, ...]` pairs (`time` is stored as a byte that
    /// expands to a 16-bit time via `<< 8`). Length is `2 * point_count`.
    pub attack_volume: Option<Vec<i8>>,
    /// Release envelope, same shape; length is `2 * point_count + 2` and starts with an
    /// implicit `(0, 64)` point so the first stored point is `[1]`.
    pub release_volume: Option<Vec<i8>>,
    pub decay_volume: i32,
    pub attack_speed: i32,
    pub release_speed: i32,
    pub decay_speed: i32,
    pub vibrato_amplitude: i32,
    pub vibrato_frequency: i32,
    pub vibrato_ramp_time: i32,
}
