// @ObfuscatedName("ft")
// jag::oldscape::graphics::Pix8
//
// Java extends Pix2D — a color-mapped 8-bit sprite plus a 24-bit palette.
// Used for logos, runes, title boxes, fonts. Each pixel is 1 byte into
// the `bpal` table (0 = transparent).

#![allow(dead_code)]

use super::pix2d;

#[derive(Clone)]
pub struct Pix8 {
    // @ObfuscatedName("ft.u")
    pub data: Vec<u8>,

    // @ObfuscatedName("ft.v")
    pub bpal: Vec<i32>,

    // @ObfuscatedName("ft.w")
    pub wi: i32,

    // @ObfuscatedName("ft.e")
    pub hi: i32,

    // @ObfuscatedName("ft.b")
    pub xof: i32,

    // @ObfuscatedName("ft.y")
    pub yof: i32,

    // @ObfuscatedName("ft.t")
    pub owi: i32,

    // @ObfuscatedName("ft.f")
    pub ohi: i32,
}

impl Pix8 {
    pub fn new() -> Self {
        Self {
            data: Vec::new(),
            bpal: Vec::new(),
            wi: 0,
            hi: 0,
            xof: 0,
            yof: 0,
            owi: 0,
            ohi: 0,
        }
    }

    // @ObfuscatedName("ft.bm()V") — Pix8.trim
    pub fn trim(&mut self) {
        if self.owi == self.wi && self.ohi == self.hi {
            return;
        }
        let mut var1 = vec![0u8; (self.ohi * self.owi) as usize];
        let mut var2 = 0usize;
        for var3 in 0..self.hi {
            for var4 in 0..self.wi {
                let dst_idx = ((self.yof + var3) * self.owi + self.xof + var4) as usize;
                var1[dst_idx] = self.data[var2];
                var2 += 1;
            }
        }
        self.data = var1;
        self.wi = self.owi;
        self.hi = self.ohi;
        self.xof = 0;
        self.yof = 0;
    }

    // @ObfuscatedName("ft.bn(III)V") — Pix8.rgbAdjust
    pub fn rgb_adjust(&mut self, arg0: i32, arg1: i32, arg2: i32) {
        for var4 in 0..self.bpal.len() {
            let var5 = self.bpal[var4] >> 16 & 0xFF;
            let mut var6 = arg0 + var5;
            if var6 < 0 { var6 = 0; } else if var6 > 255 { var6 = 255; }
            let var7 = self.bpal[var4] >> 8 & 0xFF;
            let mut var8 = arg1 + var7;
            if var8 < 0 { var8 = 0; } else if var8 > 255 { var8 = 255; }
            let var9 = self.bpal[var4] & 0xFF;
            let mut var10 = arg2 + var9;
            if var10 < 0 { var10 = 0; } else if var10 > 255 { var10 = 255; }
            self.bpal[var4] = (var6 << 16) + (var8 << 8) + var10;
        }
    }

    // @ObfuscatedName("ft.be(II)V") — Pix8.plotSprite (instance form)
    pub fn plot_sprite(&self, arg0: i32, arg1: i32) {
        let mut s = pix2d::STATE.lock().unwrap();
        let mut var3 = self.xof + arg0;
        let mut var4 = self.yof + arg1;
        let mut var5 = s.width * var4 + var3;
        let mut var6: i32 = 0;
        let mut var7 = self.hi;
        let mut var8 = self.wi;
        let mut var9 = s.width - var8;
        let mut var10: i32 = 0;
        if var4 < s.clip_min_y {
            let var11 = s.clip_min_y - var4;
            var7 -= var11;
            var4 = s.clip_min_y;
            var6 += var8 * var11;
            var5 += s.width * var11;
        }
        if var4 + var7 > s.clip_max_y {
            var7 -= var4 + var7 - s.clip_max_y;
        }
        if var3 < s.clip_min_x {
            let var12 = s.clip_min_x - var3;
            var8 -= var12;
            var3 = s.clip_min_x;
            var6 += var12;
            var5 += var12;
            var10 += var12;
            var9 += var12;
        }
        if var3 + var8 > s.clip_max_x {
            let var13 = var3 + var8 - s.clip_max_x;
            var8 -= var13;
            var10 += var13;
            var9 += var13;
        }
        if var8 > 0 && var7 > 0 {
            Self::plot_sprite_inner(&mut s.pixels, &self.data, &self.bpal, var6, var5, var8, var7, var9, var10);
        }
    }

    // @ObfuscatedName("ft.bp([I[B[IIIIIII)V") — Pix8.plotSprite (static form)
    pub fn plot_sprite_inner(
        arg0: &mut [i32],
        arg1: &[u8],
        arg2: &[i32],
        mut arg3: i32,
        mut arg4: i32,
        arg5: i32,
        arg6: i32,
        arg7: i32,
        arg8: i32,
    ) {
        let var9 = -(arg5 >> 2);
        let var10 = -(arg5 & 0x3);
        for _ in (-arg6)..0 {
            for _ in var9..0 {
                for _ in 0..4 {
                    let var13 = arg1[arg3 as usize];
                    arg3 += 1;
                    if var13 == 0 {
                        arg4 += 1;
                    } else {
                        arg0[arg4 as usize] = arg2[(var13 as i32 & 0xFF) as usize];
                        arg4 += 1;
                    }
                }
            }
            for _ in var10..0 {
                let var18 = arg1[arg3 as usize];
                arg3 += 1;
                if var18 == 0 {
                    arg4 += 1;
                } else {
                    arg0[arg4 as usize] = arg2[(var18 as i32 & 0xFF) as usize];
                    arg4 += 1;
                }
            }
            arg4 += arg7;
            arg3 += arg8;
        }
    }
}

impl Default for Pix8 {
    fn default() -> Self {
        Self::new()
    }
}
