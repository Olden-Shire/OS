//! `jagex3.sound.Wave` — an 8-bit signed PCM sample buffer plus loop metadata.
//!
//! Two ways into a Wave: synthesized from a [`crate::jagfx::JagFX`] record, or decoded
//! from a Vorbis blob via [`crate::wave_cache`]. The MIDI player references waves through
//! a `Patch.note_sound[]` table — many notes share waves so they're stored once in
//! [`WaveCache`] keyed by `(group, file)`.

#[derive(Debug, Clone)]
pub struct Wave {
    pub sampling_frequency: i32,
    /// 8-bit signed PCM, mono. WaveStream interpolates between adjacent bytes.
    pub samples: Vec<i8>,
    pub loop_start_position: i32,
    pub loop_end_position: i32,
    pub loop_reversed: bool,
}

impl Wave {
    pub fn empty() -> Self {
        Self {
            sampling_frequency: 22050,
            samples: Vec::new(),
            loop_start_position: 0,
            loop_end_position: 0,
            loop_reversed: false,
        }
    }
}
