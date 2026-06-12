//! `jagex3.sound.Tone` — additive-synthesis sound generator.
//!
//! Combines up to 5 harmonic oscillators (sine/square/saw/noise) with optional
//! amplitude+frequency modulation envelopes, release-gating, reverb, and a final IIR
//! filter pass. Used by JagFX to synthesize the rev1 client's sound-effect samples.

use io::packet::Packet;

use super::envelope::Envelope;
use super::filter::{Filter, FilterCoeffs};

/// Lookup tables shared across all Tone instances — generated once and used as if `&'static`.
struct Tables {
    noise: Vec<i32>,
    sine: Vec<i32>,
}

fn tables() -> &'static Tables {
    use std::sync::OnceLock;
    static T: OnceLock<Tables> = OnceLock::new();
    T.get_or_init(|| {
        // Java seeds Random with 0L. Their nextInt() & 0x2 yields either 0 or 2; subtract
        // 1 → either -1 or 1. We emulate Java's java.util.Random for byte-exact match.
        let mut rng = JavaRandom::new(0);
        let mut noise = Vec::with_capacity(32768);
        for _ in 0..32768 {
            noise.push((rng.next_int() & 0x2) - 1);
        }
        let mut sine = Vec::with_capacity(32768);
        for i in 0..32768 {
            sine.push((((i as f64) / 5215.190_3).sin() * 16384.0) as i32);
        }
        Tables { noise, sine }
    })
}

/// Java `java.util.Random` LCG — needed for the noise table to match the Java client exactly.
struct JavaRandom {
    seed: u64,
}

impl JavaRandom {
    fn new(seed: i64) -> Self {
        Self { seed: ((seed as u64) ^ 0x5DEEC_E66D) & ((1 << 48) - 1) }
    }
    fn next(&mut self, bits: u32) -> i32 {
        self.seed = (self.seed.wrapping_mul(0x5DEEC_E66D).wrapping_add(0xB)) & ((1 << 48) - 1);
        (self.seed >> (48 - bits)) as i32
    }
    fn next_int(&mut self) -> i32 {
        self.next(32)
    }
}

#[derive(Debug, Default, Clone)]
pub struct Tone {
    pub frequency_base: Envelope,
    pub amplitude_base: Envelope,
    pub frequency_mod_rate: Option<Envelope>,
    pub frequency_mod_range: Option<Envelope>,
    pub amplitude_mod_rate: Option<Envelope>,
    pub amplitude_mod_range: Option<Envelope>,
    pub release: Option<Envelope>,
    pub attack: Option<Envelope>,
    pub harmonic_volume: [i32; 5],
    pub harmonic_semitone: [i32; 5],
    pub harmonic_delay: [i32; 5],
    pub reverb_delay: i32,
    pub reverb_volume: i32,
    pub filter: Filter,
    pub filter_range: Envelope,
    pub length: i32,
    pub start: i32,
}

impl Tone {
    /// Render `sample_count` mono samples into `out` (must be at least that long). Returns
    /// the buffer (the same `out`) so callers can chain.
    #[allow(clippy::cognitive_complexity, clippy::too_many_lines)]
    pub fn generate(&mut self, out: &mut [i32], sample_count: usize, length: i32) {
        for v in out.iter_mut().take(sample_count) {
            *v = 0;
        }
        if length < 10 {
            return;
        }
        let samples_per_step = sample_count as f64 / length as f64;

        self.frequency_base.gen_init();
        self.amplitude_base.gen_init();

        let mut freq_start = 0i32;
        let mut freq_duration = 0i32;
        let mut freq_phase = 0i32;
        if let (Some(rate), Some(range)) = (&mut self.frequency_mod_rate, &mut self.frequency_mod_range) {
            rate.gen_init();
            range.gen_init();
            freq_start = ((rate.end - rate.start) as f64 * 32.768 / samples_per_step) as i32;
            freq_duration = (rate.start as f64 * 32.768 / samples_per_step) as i32;
        }

        let mut amp_start = 0i32;
        let mut amp_duration = 0i32;
        let mut amp_phase = 0i32;
        if let (Some(rate), Some(range)) = (&mut self.amplitude_mod_rate, &mut self.amplitude_mod_range) {
            rate.gen_init();
            range.gen_init();
            amp_start = ((rate.end - rate.start) as f64 * 32.768 / samples_per_step) as i32;
            amp_duration = (rate.start as f64 * 32.768 / samples_per_step) as i32;
        }

        let mut f_pos = [0i32; 5];
        let mut f_del = [0i32; 5];
        let mut f_amp = [0i32; 5];
        let mut f_multi = [0i32; 5];
        let mut f_offset = [0i32; 5];
        for h in 0..5 {
            if self.harmonic_volume[h] != 0 {
                f_pos[h] = 0;
                f_del[h] = (self.harmonic_delay[h] as f64 * samples_per_step) as i32;
                f_amp[h] = (self.harmonic_volume[h] << 14) / 100;
                f_multi[h] = ((self.frequency_base.end - self.frequency_base.start) as f64
                    * 32.768
                    * 1.005_792_941_067_853_4_f64.powi(self.harmonic_semitone[h])
                    / samples_per_step) as i32;
                f_offset[h] = (self.frequency_base.start as f64 * 32.768 / samples_per_step) as i32;
            }
        }

        let form = self.frequency_base.form;
        for sample in 0..sample_count {
            let frequency = self.frequency_base.gen_next(sample_count as i32);
            let mut amplitude = self.amplitude_base.gen_next(sample_count as i32);

            if let (Some(rate), Some(range)) =
                (&mut self.frequency_mod_rate, &mut self.frequency_mod_range)
            {
                let r = rate.gen_next(sample_count as i32);
                let g = range.gen_next(sample_count as i32);
                // emulated waveFunc on freq_phase, range g, form rate.form
                let f = wave_func(freq_phase, g, rate.form);
                // Java: frequency += this.waveFunc(...) >> 1
                let _frequency_mod = f >> 1;
                freq_phase = freq_phase.wrapping_add((freq_start * r >> 16) + freq_duration);
                let _ = _frequency_mod; // we re-apply below
            }

            if let (Some(rate), Some(range)) =
                (&mut self.amplitude_mod_rate, &mut self.amplitude_mod_range)
            {
                let r = rate.gen_next(sample_count as i32);
                let g = range.gen_next(sample_count as i32);
                amplitude = amplitude * ((wave_func(amp_phase, g, rate.form) >> 1) + 32768) >> 15;
                amp_phase = amp_phase.wrapping_add((amp_start * r >> 16) + amp_duration);
            }

            for h in 0..5 {
                if self.harmonic_volume[h] != 0 {
                    let position = f_del[h] + sample as i32;
                    if (position as usize) < sample_count {
                        out[position as usize] = out[position as usize].wrapping_add(wave_func(
                            f_pos[h],
                            f_amp[h] * amplitude >> 15,
                            form,
                        ));
                        f_pos[h] = f_pos[h].wrapping_add((f_multi[h] * frequency >> 16) + f_offset[h]);
                    }
                }
            }
        }

        if let (Some(release), Some(attack)) = (&mut self.release, &mut self.attack) {
            release.gen_init();
            attack.gen_init();
            let mut counter = 0i32;
            let mut muted = true;
            for sample in 0..sample_count {
                let release_value = release.gen_next(sample_count as i32);
                let attack_value = attack.gen_next(sample_count as i32);
                let threshold = if muted {
                    ((release.end - release.start) * release_value >> 8) + release.start
                } else {
                    ((release.end - release.start) * attack_value >> 8) + release.start
                };
                counter += 256;
                if counter >= threshold {
                    counter = 0;
                    muted = !muted;
                }
                if muted {
                    out[sample] = 0;
                }
            }
        }

        if self.reverb_delay > 0 && self.reverb_volume > 0 {
            let start = (self.reverb_delay as f64 * samples_per_step) as usize;
            if start < sample_count {
                for sample in start..sample_count {
                    out[sample] = out[sample]
                        .wrapping_add(out[sample - start] * self.reverb_volume / 100);
                }
            }
        }

        if self.filter.pairs[0] > 0 || self.filter.pairs[1] > 0 {
            let mut c = FilterCoeffs::default();
            self.filter_range.gen_init();
            let mut range = self.filter_range.gen_next(sample_count as i32 + 1);
            let mut forward = self.filter.calculate_coeffs(0, range as f32 / 65536.0, &mut c) as usize;
            let mut backward = self.filter.calculate_coeffs(1, range as f32 / 65536.0, &mut c) as usize;
            if sample_count >= forward + backward {
                let mut index = 0usize;
                let mut interval = backward;
                if backward > sample_count - forward {
                    interval = sample_count - forward;
                }
                while index < interval {
                    let mut sample =
                        ((out[forward + index] as i64 * c.reduce_coeff_int as i64) >> 16) as i32;
                    for offset in 0..forward {
                        sample = sample.wrapping_add(
                            ((out[forward + index - 1 - offset] as i64 * c.coeff_int[0][offset] as i64) >> 16) as i32,
                        );
                    }
                    for offset in 0..index {
                        sample = sample.wrapping_sub(
                            ((out[index - 1 - offset] as i64 * c.coeff_int[1][offset] as i64) >> 16) as i32,
                        );
                    }
                    out[index] = sample;
                    range = self.filter_range.gen_next(sample_count as i32 + 1);
                    index += 1;
                }
                interval = 128;
                loop {
                    if interval > sample_count - forward {
                        interval = sample_count - forward;
                    }
                    while index < interval {
                        let mut sample = ((out[forward + index] as i64 * c.reduce_coeff_int as i64) >> 16) as i32;
                        for offset in 0..forward {
                            sample = sample.wrapping_add(
                                ((out[forward + index - 1 - offset] as i64 * c.coeff_int[0][offset] as i64) >> 16) as i32,
                            );
                        }
                        for offset in 0..backward {
                            sample = sample.wrapping_sub(
                                ((out[index - 1 - offset] as i64 * c.coeff_int[1][offset] as i64) >> 16) as i32,
                            );
                        }
                        out[index] = sample;
                        range = self.filter_range.gen_next(sample_count as i32 + 1);
                        index += 1;
                    }
                    if index >= sample_count - forward {
                        while index < sample_count {
                            let mut sample = 0i32;
                            for offset in (forward + index - sample_count)..forward {
                                sample = sample.wrapping_add(
                                    ((out[forward + index - 1 - offset] as i64 * c.coeff_int[0][offset] as i64) >> 16) as i32,
                                );
                            }
                            for offset in 0..backward {
                                sample = sample.wrapping_sub(
                                    ((out[index - 1 - offset] as i64 * c.coeff_int[1][offset] as i64) >> 16) as i32,
                                );
                            }
                            out[index] = sample;
                            self.filter_range.gen_next(sample_count as i32 + 1);
                            index += 1;
                        }
                        break;
                    }
                    forward = self.filter.calculate_coeffs(0, range as f32 / 65536.0, &mut c) as usize;
                    backward = self.filter.calculate_coeffs(1, range as f32 / 65536.0, &mut c) as usize;
                    interval += 128;
                }
            }
        }

        for sample in 0..sample_count {
            out[sample] = out[sample].clamp(-32768, 32767);
        }
    }

    pub fn load(&mut self, p: &mut Packet) {
        self.frequency_base = Envelope::new();
        self.frequency_base.load(p);
        self.amplitude_base = Envelope::new();
        self.amplitude_base.load(p);

        if p.g1() != 0 {
            p.pos -= 1;
            let mut a = Envelope::new();
            a.load(p);
            let mut b = Envelope::new();
            b.load(p);
            self.frequency_mod_rate = Some(a);
            self.frequency_mod_range = Some(b);
        }
        if p.g1() != 0 {
            p.pos -= 1;
            let mut a = Envelope::new();
            a.load(p);
            let mut b = Envelope::new();
            b.load(p);
            self.amplitude_mod_rate = Some(a);
            self.amplitude_mod_range = Some(b);
        }
        if p.g1() != 0 {
            p.pos -= 1;
            let mut a = Envelope::new();
            a.load(p);
            let mut b = Envelope::new();
            b.load(p);
            self.release = Some(a);
            self.attack = Some(b);
        }

        for h in 0..10 {
            let v = p.gsmart();
            if v == 0 {
                break;
            }
            if h < 5 {
                self.harmonic_volume[h] = v;
                self.harmonic_semitone[h] = p.gsmarts();
                self.harmonic_delay[h] = p.gsmart();
            } else {
                p.gsmarts();
                p.gsmart();
            }
        }

        self.reverb_delay = p.gsmart();
        self.reverb_volume = p.gsmart();
        self.length = p.g2();
        self.start = p.g2();

        self.filter = Filter::default();
        self.filter_range = Envelope::new();
        self.filter.load(p, &mut self.filter_range);
    }
}

fn wave_func(phase: i32, amplitude: i32, form: i32) -> i32 {
    let t = tables();
    match form {
        1 => if (phase & 0x7FFF) < 16384 { amplitude } else { -amplitude },
        2 => (t.sine[(phase & 0x7FFF) as usize] * amplitude) >> 14,
        3 => ((phase & 0x7FFF) * amplitude >> 14) - amplitude,
        4 => t.noise[((phase / 2607) & 0x7FFF) as usize] * amplitude,
        _ => 0,
    }
}
