//! `jagex3.sound.JagFX` — additive-synthesis sound effect (up to 10 tones).
//!
//! Each cache JagFX record decodes into up to 10 [`Tone`]s; calling [`JagFX::to_wave`]
//! renders them into an 8-bit PCM buffer at 22050 Hz suitable for [`crate::wave::Wave`].

pub mod envelope;
pub mod filter;
pub mod tone;

use io::packet::Packet;

use crate::wave::Wave;
use tone::Tone;

pub struct JagFX {
    pub tones: [Option<Tone>; 10],
    pub loop_begin: i32,
    pub loop_end: i32,
}

impl JagFX {
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

    /// Render to an 8-bit PCM Wave at 22050 Hz.
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
