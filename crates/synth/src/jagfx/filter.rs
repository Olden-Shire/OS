//! `jagex3.sound.Filter` — IIR biquad cascade used by Tone synthesis.
//!
//! Up to two pairs per direction (forward / backward); coefficients are interpolated over
//! the lifetime of a tone via the filter's range envelope.

use io::packet::Packet;

use super::envelope::Envelope;

#[derive(Debug, Default, Clone)]
pub struct Filter {
    pub pairs: [i32; 2],
    pub frequencies: [[[i32; 4]; 2]; 2],
    pub ranges: [[[i32; 4]; 2]; 2],
    pub unities: [i32; 2],
}

/// Coefficient scratch — Java keeps them on static fields shared across all filters; we
/// pass these along the call chain so the filter is reentrant.
#[derive(Debug, Default, Clone)]
pub struct FilterCoeffs {
    pub coeff: [[f32; 8]; 2],
    pub coeff_int: [[i32; 8]; 2],
    pub reduce_coeff: f32,
    pub reduce_coeff_int: i32,
}

impl Filter {
    fn radius(&self, dir: usize, pair: usize, delta: f32) -> f32 {
        let g = (self.ranges[dir][1][pair] - self.ranges[dir][0][pair]) as f32 * delta
            + self.ranges[dir][0][pair] as f32;
        let g2 = g * 0.001_525_878_9;
        1.0 - 10.0_f32.powf(-g2 / 20.0)
    }

    fn frequency_for(&self, dir: usize, pair: usize, delta: f32) -> f32 {
        let f1 = (self.frequencies[dir][1][pair] - self.frequencies[dir][0][pair]) as f32 * delta
            + self.frequencies[dir][0][pair] as f32;
        let f2 = f1 * 1.220_703_1e-4;
        let f3 = 2.0_f32.powf(f2) * 32.703_197;
        f3 * std::f32::consts::PI / 11025.0
    }

    pub fn calculate_coeffs(&self, dir: usize, delta: f32, c: &mut FilterCoeffs) -> i32 {
        if dir == 0 {
            let u = (self.unities[1] - self.unities[0]) as f32 * delta + self.unities[0] as f32;
            let u2 = u * 0.003_051_758;
            c.reduce_coeff = 10.0_f32.powf(-u2 / 20.0 / 1.0); // == pow(0.1, u2/20)
            c.reduce_coeff_int = (c.reduce_coeff * 65536.0) as i32;
        }
        if self.pairs[dir] == 0 {
            return 0;
        }
        let u = self.radius(dir, 0, delta);
        c.coeff[dir][0] = u * -2.0 * self.frequency_for(dir, 0, delta).cos();
        c.coeff[dir][1] = u * u;
        for pair in 1..self.pairs[dir] as usize {
            let g = self.radius(dir, pair, delta);
            let a = g * -2.0 * self.frequency_for(dir, pair, delta).cos();
            let b = g * g;
            c.coeff[dir][pair * 2 + 1] = c.coeff[dir][pair * 2 - 1] * b;
            c.coeff[dir][pair * 2] = c.coeff[dir][pair * 2 - 1] * a + c.coeff[dir][pair * 2 - 2] * b;
            for i in (2..=pair * 2 - 1).rev() {
                c.coeff[dir][i] += c.coeff[dir][i - 1] * a + c.coeff[dir][i - 2] * b;
            }
            c.coeff[dir][1] += c.coeff[dir][0] * a + b;
            c.coeff[dir][0] += a;
        }
        if dir == 0 {
            for i in 0..(self.pairs[0] * 2) as usize {
                c.coeff[0][i] *= c.reduce_coeff;
            }
        }
        for i in 0..(self.pairs[dir] * 2) as usize {
            c.coeff_int[dir][i] = (c.coeff[dir][i] * 65536.0) as i32;
        }
        self.pairs[dir] * 2
    }

    pub fn load(&mut self, p: &mut Packet, range: &mut Envelope) {
        let header = p.g1();
        self.pairs[0] = header >> 4;
        self.pairs[1] = header & 0xF;
        if header == 0 {
            self.unities = [0, 0];
            return;
        }
        self.unities[0] = p.g2();
        self.unities[1] = p.g2();
        let flags = p.g1();
        for d in 0..2 {
            for k in 0..self.pairs[d] as usize {
                self.frequencies[d][0][k] = p.g2();
                self.ranges[d][0][k] = p.g2();
            }
        }
        for d in 0..2 {
            for k in 0..self.pairs[d] as usize {
                if (flags & (1 << (d * 4 + k))) == 0 {
                    self.frequencies[d][1][k] = self.frequencies[d][0][k];
                    self.ranges[d][1][k] = self.ranges[d][0][k];
                } else {
                    self.frequencies[d][1][k] = p.g2();
                    self.ranges[d][1][k] = p.g2();
                }
            }
        }
        if flags != 0 || self.unities[1] != self.unities[0] {
            range.load_points(p);
        }
    }
}
