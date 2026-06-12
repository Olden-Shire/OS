// @ObfuscatedName("o")
// jag::oldscape::sound::JagFX
//
// Additive-synthesis sound effect (up to 10 tones). Each JagFX cache
// record decodes into up to 10 Tones; `to_wave` renders them into an
// 8-bit PCM buffer at 22050 Hz suitable for sound::wave::Wave.

#![allow(dead_code)]

pub mod envelope;
pub mod filter;
pub mod tone;

use crate::io::packet::Packet;
use crate::sound::wave::Wave;

use tone::Tone;

pub struct JagFX {
    // @ObfuscatedName("o.d")
    pub tones: [Option<Tone>; 10],
    // @ObfuscatedName("o.l")
    pub loop_begin: i32,
    // @ObfuscatedName("o.m")
    pub loop_end: i32,
}

impl JagFX {
    // @ObfuscatedName("o.r(Lch;II)Lo;") — Java: JagFX.load via Packet ctor
    pub fn decode(src: &[u8]) -> Self {
        let mut p = Packet::from_vec(src.to_vec());
        let mut tones: [Option<Tone>; 10] = Default::default();
        for slot in &mut tones {
            let probe = p.g1();
            if probe != 0 {
                p.pos -= 1;
                let mut t = Tone::default();
                t.load(&mut p);
                *slot = Some(t);
            }
        }
        let loop_begin = p.g2();
        let loop_end = p.g2();
        Self { tones, loop_begin, loop_end }
    }

    // @ObfuscatedName("o.r()I") — JagFX.optimiseStart.
    //
    // Verbatim port of JagFX.java:50-73. Finds the earliest tone start
    // (or loop-begin) in 20-sample units and shifts every tone +
    // loop boundary backwards by that amount, so the rendered Wave
    // doesn't carry a leading silent prefix. Returns the trim offset.
    pub fn optimise_start(&mut self) -> i32 {
        let mut earliest = 9_999_999i32;
        for t in self.tones.iter().flatten() {
            let s = t.start / 20;
            if s < earliest { earliest = s; }
        }
        if self.loop_begin < self.loop_end && self.loop_begin / 20 < earliest {
            earliest = self.loop_begin / 20;
        }
        if earliest == 9_999_999 || earliest == 0 { return 0; }
        for t in self.tones.iter_mut().flatten() {
            t.start -= earliest * 20;
        }
        if self.loop_begin < self.loop_end {
            self.loop_begin -= earliest * 20;
            self.loop_end -= earliest * 20;
        }
        earliest
    }

    // @ObfuscatedName("o.d()Leq;") — JagFX.toWave: render to 22050 Hz Wave
    pub fn to_wave(&mut self) -> Wave {
        let samples = self.make_sound();
        Wave {
            sampling_frequency: 22050,
            samples,
            loop_start_position: self.loop_begin * 22050 / 1000,
            loop_end_position: self.loop_end * 22050 / 1000,
            loop_reversed: false,
        }
    }

    fn make_sound(&mut self) -> Vec<i8> {
        let mut duration = 0i32;
        for t in self.tones.iter().flatten() {
            if t.start + t.length > duration {
                duration = t.start + t.length;
            }
        }
        if duration == 0 {
            return Vec::new();
        }
        let sample_count = (duration * 22050 / 1000) as usize;
        let mut buf = vec![0i8; sample_count];
        let mut scratch = vec![0i32; 22050 * 10];
        for slot in &mut self.tones {
            if let Some(t) = slot.as_mut() {
                let tone_samples = (t.length * 22050 / 1000) as usize;
                let start = (t.start * 22050 / 1000) as usize;
                t.generate(&mut scratch, tone_samples, t.length);
                for s in 0..tone_samples {
                    if start + s >= sample_count {
                        break;
                    }
                    let mut v = (scratch[s] >> 8) + buf[start + s] as i32;
                    if ((v + 128) & 0xFFFF_FF00u32 as i32) != 0 {
                        v = (v >> 31) ^ 0x7F;
                    }
                    buf[start + s] = v as i8;
                }
            }
        }
        buf
    }
}
