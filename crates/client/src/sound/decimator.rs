// @ObfuscatedName("p") — jag::oldscape::sound::Decimator.
//
// Sinc resampler. Java's Wave.adjustForRate uses this to convert
// arbitrary-rate PCM into the mixer's native sample rate. The kernel
// is a 14-tap windowed sinc with a 0.46 + 0.54·cos Hamming envelope.

#![allow(dead_code)]

pub struct Decimator {
    // @ObfuscatedName("p.m")
    pub input_rate: i32,
    // @ObfuscatedName("p.c")
    pub output_rate: i32,
    // @ObfuscatedName("p.n") — `[input_rate][14]` kernel table.
    pub resample_table: Option<Vec<[i32; 14]>>,
}

fn hcf(mut a: i32, mut b: i32) -> i32 {
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a
}

impl Decimator {
    pub fn new(arg0: i32, arg1: i32) -> Self {
        if arg0 == arg1 {
            return Self { input_rate: arg0, output_rate: arg1, resample_table: None };
        }
        let g = hcf(arg0, arg1);
        let input_rate = arg0 / g;
        let output_rate = arg1 / g;
        let mut table: Vec<[i32; 14]> = vec![[0i32; 14]; input_rate as usize];
        for var6 in 0..input_rate as usize {
            let row = &mut table[var6];
            let var8 = var6 as f64 / input_rate as f64 + 6.0;
            let mut var10 = (var8 - 7.0 + 1.0).floor() as i32;
            if var10 < 0 { var10 = 0; }
            let mut var11 = (var8 + 7.0).ceil() as i32;
            if var11 > 14 { var11 = 14; }
            let var12 = output_rate as f64 / input_rate as f64;
            while var10 < var11 {
                let var14 = (var10 as f64 - var8) * std::f64::consts::PI;
                let var16 = if !(-1.0e-4..=1.0e-4).contains(&var14) {
                    var12 * (var14.sin() / var14)
                } else {
                    var12
                };
                let var18 = var16
                    * (((var10 as f64 - var8) * 0.2243994752564138).cos() * 0.46 + 0.54);
                row[var10 as usize] = (var18 * 65536.0 + 0.5).floor() as i32;
                var10 += 1;
            }
        }
        Self { input_rate, output_rate, resample_table: Some(table) }
    }

    // @ObfuscatedName("p.r([BI)[B") — Decimator.decimate. Resamples
    // `arg0` to the output rate; returns the resampled buffer (or
    // the input unchanged when input_rate == output_rate).
    pub fn decimate(&self, arg0: &[i8]) -> Vec<i8> {
        let Some(table) = self.resample_table.as_ref() else {
            return arg0.to_vec();
        };
        let var2 = ((self.output_rate as i64 * arg0.len() as i64) / self.input_rate as i64
            + 14) as usize;
        let mut var3 = vec![0i32; var2];
        let mut var4 = 0usize;
        let mut var5 = 0i32;
        for var6 in 0..arg0.len() {
            let var7 = arg0[var6] as i32;
            let var8 = &table[var5 as usize];
            for var9 in 0..14usize {
                if var4 + var9 < var2 {
                    var3[var4 + var9] += var8[var9] * var7;
                }
            }
            let var10 = self.output_rate + var5;
            let var11 = var10 / self.input_rate;
            var4 += var11 as usize;
            var5 = var10 - self.input_rate * var11;
        }
        let mut out = vec![0i8; var2];
        for var12 in 0..var2 {
            let var13 = (var3[var12] + 32768) >> 16;
            out[var12] = if var13 < -128 { -128 } else if var13 > 127 { 127 } else { var13 as i8 };
        }
        out
    }

    pub fn transmit_freq(&self, arg0: i32) -> i32 {
        if self.resample_table.is_some() {
            ((self.output_rate as i64 * arg0 as i64) / self.input_rate as i64) as i32
        } else { arg0 }
    }

    pub fn transmit_pos(&self, arg0: i32) -> i32 {
        if self.resample_table.is_some() {
            ((self.output_rate as i64 * arg0 as i64) / self.input_rate as i64) as i32 + 6
        } else { arg0 }
    }
}
