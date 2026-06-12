// @ObfuscatedName("at")
// jag::oldscape::sound::Wave
//
// 8-bit signed PCM + loop metadata. Two backings: synthesized via
// JagFX, or decoded from the JagVorbis archive. MidiNote streams play
// these via WaveStream.

#![allow(dead_code)]

#[derive(Debug, Clone)]
pub struct Wave {
    // @ObfuscatedName("at.r")
    pub sampling_frequency: i32,
    // @ObfuscatedName("at.d") — i8 mono PCM samples
    pub samples: Vec<i8>,
    // @ObfuscatedName("at.l")
    pub loop_start_position: i32,
    // @ObfuscatedName("at.m")
    pub loop_end_position: i32,
    // @ObfuscatedName("at.c")
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

    // Verbatim port of Java Wave 4-arg ctor (Wave.java:24-29). Used by
    // JagFX + JagVorbis to wrap an already-decoded sample buffer with
    // its frequency + loop bounds. `loop_reversed` defaults to false.
    pub fn new(sampling_frequency: i32, samples: Vec<i8>,
               loop_start_position: i32, loop_end_position: i32) -> Self {
        Self {
            sampling_frequency,
            samples,
            loop_start_position,
            loop_end_position,
            loop_reversed: false,
        }
    }

    // Verbatim port of Java Wave 5-arg ctor (Wave.java:31-37). Same as
    // `new` but with explicit `loop_reversed`; used by samples that
    // play their loop tail backwards before forward.
    pub fn new_with_reverse(sampling_frequency: i32, samples: Vec<i8>,
                            loop_start_position: i32, loop_end_position: i32,
                            loop_reversed: bool) -> Self {
        Self {
            sampling_frequency,
            samples,
            loop_start_position,
            loop_end_position,
            loop_reversed,
        }
    }

    // @ObfuscatedName("at.k(Lp;I)V") — Wave.decimate.
    //
    // Resamples this wave through a Decimator, updating sampling
    // frequency + loop positions. Java's impl rebuilds `samples` via
    // Decimator.decimate then scales the loop bounds by
    // `transmitPos`, and updates sampling_frequency via
    // `transmitFreq`.
    pub fn decimate(&mut self, dec: &crate::sound::decimator::Decimator) {
        if dec.resample_table.is_none() { return; }
        let new_samples = dec.decimate(&self.samples);
        self.samples = new_samples;
        self.loop_start_position = dec.transmit_pos(self.loop_start_position);
        self.loop_end_position = dec.transmit_pos(self.loop_end_position);
        self.sampling_frequency = dec.transmit_freq(self.sampling_frequency);
    }
}
