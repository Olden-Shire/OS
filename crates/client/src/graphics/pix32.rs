// @ObfuscatedName("fq")
// jag::oldscape::graphics::Pix32
//
// Java extends Pix2D — a 32-bit ARGB sprite. Backs the JPEG title image,
// the logo, and is the per-line buffer used by the flame animation. The
// rotation routines are needed by TitleScreen for the spinning runes and
// flame swirls.

#![allow(dead_code, non_snake_case)]

use super::pix2d;
use super::pix8::Pix8;

pub struct Pix32 {
    // @ObfuscatedName("fq.u")
    pub data: Vec<i32>,

    // @ObfuscatedName("fq.v")
    pub wi: i32,

    // @ObfuscatedName("fq.w")
    pub hi: i32,

    // @ObfuscatedName("fq.e")
    pub xof: i32,

    // @ObfuscatedName("fq.b")
    pub yof: i32,

    // @ObfuscatedName("fq.y")
    pub owi: i32,

    // @ObfuscatedName("fq.t")
    pub ohi: i32,
}

impl Pix32 {
    pub fn new_empty() -> Self {
        Self { data: Vec::new(), wi: 0, hi: 0, xof: 0, yof: 0, owi: 0, ohi: 0 }
    }

    // Pure accessors for the original (outer) width/height — these stay
    // constant across trim/untrim so callers anchoring tag layout use
    // them instead of the live wi/hi.
    pub fn outer_w(&self) -> i32 { self.owi }
    pub fn outer_h(&self) -> i32 { self.ohi }

    // Anchor a sprite plot at (x, y) accounting for the sprite's own
    // trim offset — Java's plotSprite paths add `xof`/`yof` inline.
    pub fn anchor_x(&self, x: i32) -> i32 { self.xof + x }
    pub fn anchor_y(&self, y: i32) -> i32 { self.yof + y }

    // Compute centre-anchored x/y given the desired centre point —
    // shifts by half the outer width / height.
    pub fn centre_x(&self, x: i32) -> i32 { x - self.owi / 2 }
    pub fn centre_y(&self, y: i32) -> i32 { y - self.ohi / 2 }

    // Java ctor `Pix32(int wi, int hi)`
    pub fn new(arg0: i32, arg1: i32) -> Self {
        let size = (arg0 * arg1) as usize;
        Self {
            data: vec![0; size],
            wi: arg0,
            owi: arg0,
            hi: arg1,
            ohi: arg1,
            xof: 0,
            yof: 0,
        }
    }

    // Java ctor `Pix32(byte[] jpeg, Component)` decodes a JPEG via AWT.
    // Caller provides the decoded pixel buffer.
    pub fn from_pixels(data: Vec<i32>, width: i32, height: i32) -> Self {
        Self {
            data,
            wi: width,
            owi: width,
            hi: height,
            ohi: height,
            xof: 0,
            yof: 0,
        }
    }

    // @ObfuscatedName("fq.bm()Lfq;") — Pix32.copyHFlip
    pub fn copy_h_flip(&self) -> Pix32 {
        let mut var1 = Pix32::new(self.wi, self.hi);
        var1.owi = self.owi;
        var1.ohi = self.ohi;
        var1.xof = self.owi - self.wi - self.xof;
        var1.yof = self.yof;
        for var2 in 0..self.hi {
            for var3 in 0..self.wi {
                var1.data[(self.wi * var2 + var3) as usize] =
                    self.data[(self.wi * var2 + self.wi - 1 - var3) as usize];
            }
        }
        var1
    }

    // @ObfuscatedName("fq.bn()V") — Pix32.setPixels (installs as Pix2D buffer)
    pub fn set_pixels(&mut self) {
        pix2d::set_pixels(self.data.clone(), self.wi, self.hi);
    }

    // @ObfuscatedName("fq.be(III)V") — Pix32.rgbAdjust
    pub fn rgb_adjust(&mut self, arg0: i32, arg1: i32, arg2: i32) {
        for var4 in 0..self.data.len() {
            let var5 = self.data[var4];
            if var5 != 0 {
                let var6 = var5 >> 16 & 0xFF;
                let mut var7 = arg0 + var6;
                if var7 < 1 { var7 = 1; } else if var7 > 255 { var7 = 255; }
                let var8 = var5 >> 8 & 0xFF;
                let mut var9 = arg1 + var8;
                if var9 < 1 { var9 = 1; } else if var9 > 255 { var9 = 255; }
                let var10 = var5 & 0xFF;
                let mut var11 = arg2 + var10;
                if var11 < 1 { var11 = 1; } else if var11 > 255 { var11 = 255; }
                self.data[var4] = (var7 << 16) + (var9 << 8) + var11;
            }
        }
    }

    // @ObfuscatedName("fq.bp()V") — Pix32.trim
    pub fn trim(&mut self) {
        if self.wi == self.owi && self.ohi == self.hi {
            return;
        }
        let mut var1 = vec![0i32; (self.ohi * self.owi) as usize];
        for var2 in 0..self.hi {
            for var3 in 0..self.wi {
                let dst = ((self.yof + var2) * self.owi + self.xof + var3) as usize;
                var1[dst] = self.data[(self.wi * var2 + var3) as usize];
            }
        }
        self.data = var1;
        self.wi = self.owi;
        self.hi = self.ohi;
        self.xof = 0;
        self.yof = 0;
    }

    // @ObfuscatedName("fq.ba(I)V") — Pix32.untrim
    pub fn untrim(&mut self, arg0: i32) {
        if self.wi == self.owi && self.ohi == self.hi {
            return;
        }
        let mut var2 = arg0;
        if arg0 > self.xof { var2 = self.xof; }
        let mut var3 = arg0;
        if self.xof + arg0 + self.wi > self.owi { var3 = self.owi - self.xof - self.wi; }
        let mut var4 = arg0;
        if arg0 > self.yof { var4 = self.yof; }
        let mut var5 = arg0;
        if self.yof + arg0 + self.hi > self.ohi { var5 = self.ohi - self.yof - self.hi; }
        let var6 = self.wi + var2 + var3;
        let var7 = self.hi + var4 + var5;
        let mut var8 = vec![0i32; (var6 * var7) as usize];
        for var9 in 0..self.hi {
            for var10 in 0..self.wi {
                let dst = ((var4 + var9) * var6 + var2 + var10) as usize;
                var8[dst] = self.data[(self.wi * var9 + var10) as usize];
            }
        }
        self.data = var8;
        self.wi = var6;
        self.hi = var7;
        self.xof -= var2;
        self.yof -= var4;
    }

    // @ObfuscatedName("fq.bc()V") — Pix32.hflip
    pub fn hflip(&mut self) {
        let mut var1 = vec![0i32; (self.wi * self.hi) as usize];
        let mut var2 = 0;
        for var3 in 0..self.hi {
            for var4 in (0..self.wi).rev() {
                var1[var2] = self.data[(self.wi * var3 + var4) as usize];
                var2 += 1;
            }
        }
        self.data = var1;
        self.xof = self.owi - self.wi - self.xof;
    }

    // @ObfuscatedName("fq.br()V") — Pix32.vflip
    pub fn vflip(&mut self) {
        let mut var1 = vec![0i32; (self.wi * self.hi) as usize];
        let mut var2 = 0;
        for var3 in (0..self.hi).rev() {
            for var4 in 0..self.wi {
                var1[var2] = self.data[(self.wi * var3 + var4) as usize];
                var2 += 1;
            }
        }
        self.data = var1;
        self.yof = self.ohi - self.hi - self.yof;
    }

    // @ObfuscatedName("fq.bb(I)V") — Pix32.addOutline
    pub fn add_outline(&mut self, arg0: i32) {
        let mut var2 = vec![0i32; (self.wi * self.hi) as usize];
        let mut var3 = 0i32;
        for var4 in 0..self.hi {
            for var5 in 0..self.wi {
                let mut var6 = self.data[var3 as usize];
                if var6 == 0 {
                    if var5 > 0 && self.data[(var3 - 1) as usize] != 0 { var6 = arg0; }
                    else if var4 > 0 && self.data[(var3 - self.wi) as usize] != 0 { var6 = arg0; }
                    else if var5 < self.wi - 1 && self.data[(var3 + 1) as usize] != 0 { var6 = arg0; }
                    else if var4 < self.hi - 1 && self.data[(self.wi + var3) as usize] != 0 { var6 = arg0; }
                }
                var2[var3 as usize] = var6;
                var3 += 1;
            }
        }
        self.data = var2;
    }

    // @ObfuscatedName("fq.bd(I)V") — Pix32.addShadow
    pub fn add_shadow(&mut self, arg0: i32) {
        for var2 in (1..self.hi).rev() {
            let var3 = self.wi * var2;
            for var4 in (1..self.wi).rev() {
                if self.data[(var3 + var4) as usize] == 0
                    && self.data[(var3 + var4 - 1 - self.wi) as usize] != 0
                {
                    self.data[(var3 + var4) as usize] = arg0;
                }
            }
        }
    }

    // @ObfuscatedName("fq.cr(II)V") — Pix32.quickPlotSprite
    pub fn quick_plot_sprite(&self, x: i32, y: i32) {
        let mut s = pix2d::STATE.lock().unwrap();
        let mut var3 = self.xof + x;
        let mut var4 = self.yof + y;
        let mut var5 = s.width * var4 + var3;
        let mut var6 = 0i32;
        let mut var7 = self.hi;
        let mut var8 = self.wi;
        let mut var9 = s.width - var8;
        let mut var10 = 0i32;
        if var4 < s.clip_min_y {
            let var11 = s.clip_min_y - var4;
            var7 -= var11;
            var4 = s.clip_min_y;
            var6 += var8 * var11;
            var5 += s.width * var11;
        }
        if var4 + var7 > s.clip_max_y { var7 -= var4 + var7 - s.clip_max_y; }
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
            Self::plot_quick(&mut s.pixels, &self.data, var6, var5, var8, var7, var9, var10);
        }
    }

    // @ObfuscatedName("fq.cs([I[IIIIIII)V") — Pix32.plotQuick (static)
    pub fn plot_quick(
        arg0: &mut [i32],
        arg1: &[i32],
        mut arg2: i32,
        mut arg3: i32,
        arg4: i32,
        arg5: i32,
        arg6: i32,
        arg7: i32,
    ) {
        for _ in (-arg5)..0 {
            let var9 = arg3 + arg4 - 3;
            while arg3 < var9 {
                for _ in 0..4 {
                    arg0[arg3 as usize] = arg1[arg2 as usize];
                    arg3 += 1;
                    arg2 += 1;
                }
            }
            let var9 = var9 + 3;
            while arg3 < var9 {
                arg0[arg3 as usize] = arg1[arg2 as usize];
                arg3 += 1;
                arg2 += 1;
            }
            arg3 += arg6;
            arg2 += arg7;
        }
    }

    // @ObfuscatedName("fq.cj(II)V") — Pix32.plotSprite (instance)
    pub fn plot_sprite(&self, arg0: i32, arg1: i32) {
        let mut s = pix2d::STATE.lock().unwrap();
        let mut var3 = self.xof + arg0;
        let mut var4 = self.yof + arg1;
        let mut var5 = s.width * var4 + var3;
        let mut var6 = 0i32;
        let mut var7 = self.hi;
        let mut var8 = self.wi;
        let mut var9 = s.width - var8;
        let mut var10 = 0i32;
        if var4 < s.clip_min_y {
            let var11 = s.clip_min_y - var4;
            var7 -= var11;
            var4 = s.clip_min_y;
            var6 += var8 * var11;
            var5 += s.width * var11;
        }
        if var4 + var7 > s.clip_max_y { var7 -= var4 + var7 - s.clip_max_y; }
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
            Self::plot_sprite_inner(&mut s.pixels, &self.data, 0, var6, var5, var8, var7, var9, var10);
        }
    }

    // @ObfuscatedName("fq.cl([I[IIIIIIII)V") — Pix32.plotSprite (static)
    pub fn plot_sprite_inner(
        arg0: &mut [i32],
        arg1: &[i32],
        _arg2: i32,
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
                        arg0[arg4 as usize] = var13;
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
                    arg0[arg4 as usize] = var18;
                    arg4 += 1;
                }
            }
            arg4 += arg7;
            arg3 += arg8;
        }
    }

    // @ObfuscatedName("fq.cp(IIII)V") — Pix32.scalePlotSprite
    pub fn scale_plot_sprite(&self, mut arg0: i32, mut arg1: i32, mut arg2: i32, mut arg3: i32) {
        if arg2 <= 0 || arg3 <= 0 {
            return;
        }
        let var5 = self.wi;
        let var6 = self.hi;
        let mut var7 = 0i32;
        let mut var8 = 0i32;
        let var9 = self.owi;
        let var10 = self.ohi;
        let var11 = (var9 << 16) / arg2;
        let var12 = (var10 << 16) / arg3;
        if self.xof > 0 {
            let var13 = ((self.xof << 16) + var11 - 1) / var11;
            arg0 += var13;
            var7 += var11 * var13 - (self.xof << 16);
        }
        if self.yof > 0 {
            let var14 = ((self.yof << 16) + var12 - 1) / var12;
            arg1 += var14;
            var8 += var12 * var14 - (self.yof << 16);
        }
        if var5 < var9 { arg2 = ((var5 << 16) - var7 + var11 - 1) / var11; }
        if var6 < var10 { arg3 = ((var6 << 16) - var8 + var12 - 1) / var12; }
        let mut s = pix2d::STATE.lock().unwrap();
        let mut var15 = s.width * arg1 + arg0;
        let mut var16 = s.width - arg2;
        if arg1 + arg3 > s.clip_max_y { arg3 -= arg1 + arg3 - s.clip_max_y; }
        if arg1 < s.clip_min_y {
            let var17 = s.clip_min_y - arg1;
            arg3 -= var17;
            var15 += s.width * var17;
            var8 += var12 * var17;
        }
        if arg0 + arg2 > s.clip_max_x {
            let var18 = arg0 + arg2 - s.clip_max_x;
            arg2 -= var18;
            var16 += var18;
        }
        if arg0 < s.clip_min_x {
            let var19 = s.clip_min_x - arg0;
            arg2 -= var19;
            var15 += var19;
            var7 += var11 * var19;
            var16 += var19;
        }
        Self::plot_scale(&mut s.pixels, &self.data, 0, var7, var8, var15, var16, arg2, arg3, var11, var12, var5);
    }

    // @ObfuscatedName("fq.ca([I[IIIIIIIIIII)V") — Pix32.plotScale (static)
    pub fn plot_scale(
        arg0: &mut [i32],
        arg1: &[i32],
        _arg2: i32,
        mut arg3: i32,
        mut arg4: i32,
        mut arg5: i32,
        arg6: i32,
        arg7: i32,
        arg8: i32,
        arg9: i32,
        arg10: i32,
        arg11: i32,
    ) {
        let var12 = arg3;
        for _ in (-arg8)..0 {
            let var14 = (arg4 >> 16) * arg11;
            for _ in (-arg7)..0 {
                let var16 = arg1[((arg3 >> 16) + var14) as usize];
                if var16 == 0 { arg5 += 1; } else { arg0[arg5 as usize] = var16; arg5 += 1; }
                arg3 += arg9;
            }
            arg4 += arg10;
            arg3 = var12;
            arg5 += arg6;
        }
    }

    // @ObfuscatedName("fq.cm(IIIII)V") — Pix32.transScalePlotSprite.
    //
    // Java's translucent + scaled blit. Combines scale_plot_sprite's
    // step logic with a per-pixel alpha-blend; `trans` is the SOURCE
    // weight (256 = opaque, 0 = invisible) — callers pass
    // 256 - com.trans, same as tran_sprite's alpha. Used by
    // interface_render type-5 (graphic) when both com.trans != 0 and
    // com.width != owi/com.height != ohi.
    pub fn trans_scale_plot_sprite(&self, mut arg0: i32, mut arg1: i32,
                                   mut arg2: i32, mut arg3: i32, trans: i32) {
        if arg2 <= 0 || arg3 <= 0 { return; }
        let var5 = self.wi;
        let var6 = self.hi;
        let mut var7 = 0i32;
        let mut var8 = 0i32;
        let var9 = self.owi;
        let var10 = self.ohi;
        let var11 = (var9 << 16) / arg2;
        let var12 = (var10 << 16) / arg3;
        if self.xof > 0 {
            let v = ((self.xof << 16) + var11 - 1) / var11;
            arg0 += v;
            var7 += var11 * v - (self.xof << 16);
        }
        if self.yof > 0 {
            let v = ((self.yof << 16) + var12 - 1) / var12;
            arg1 += v;
            var8 += var12 * v - (self.yof << 16);
        }
        if var5 < var9 { arg2 = ((var5 << 16) - var7 + var11 - 1) / var11; }
        if var6 < var10 { arg3 = ((var6 << 16) - var8 + var12 - 1) / var12; }
        let mut s = pix2d::STATE.lock().unwrap();
        let mut var15 = s.width * arg1 + arg0;
        let mut var16 = s.width - arg2;
        if arg1 + arg3 > s.clip_max_y { arg3 -= arg1 + arg3 - s.clip_max_y; }
        if arg1 < s.clip_min_y {
            let v = s.clip_min_y - arg1;
            arg3 -= v;
            var15 += s.width * v;
            var8 += var12 * v;
        }
        if arg0 + arg2 > s.clip_max_x {
            let v = arg0 + arg2 - s.clip_max_x;
            arg2 -= v;
            var16 += v;
        }
        if arg0 < s.clip_min_x {
            let v = s.clip_min_x - arg0;
            arg2 -= v;
            var15 += v;
            var7 += var11 * v;
            var16 += v;
        }
        Self::plot_trans_scale(&mut s.pixels, &self.data, var7, var8, var15,
                               var16, arg2, arg3, var11, var12, var5, trans);
    }

    // @ObfuscatedName("fq.cw([I[IIIIIIIIIIII)V") — Pix32.tranScale (static)
    fn plot_trans_scale(
        dst: &mut [i32], src: &[i32],
        mut sx: i32, mut sy: i32, mut dpos: i32,
        skip: i32, w: i32, h: i32,
        step_x: i32, step_y: i32, src_w: i32,
        trans: i32,
    ) {
        let var13 = (256 - trans) as u32;
        let sx_start = sx;
        for _ in (-h)..0 {
            let row = (sy >> 16) * src_w;
            for _ in (-w)..0 {
                let var18 = src[((sx >> 16) + row) as usize] as u32;
                if var18 == 0 {
                    dpos += 1;
                } else {
                    let var19 = dst[dpos as usize] as u32;
                    let blended = (((var18 & 0xFF00FF) * trans as u32 + (var19 & 0xFF00FF) * var13) & 0xFF00FF00)
                        + (((var18 & 0xFF00) * trans as u32 + (var19 & 0xFF00) * var13) & 0xFF0000);
                    dst[dpos as usize] = (blended >> 8) as i32;
                    dpos += 1;
                }
                sx += step_x;
            }
            sy += step_y;
            sx = sx_start;
            dpos += skip;
        }
    }

    // @ObfuscatedName("fq.co(IIII)V") — Pix32.litPlotSprite
    pub fn lit_plot_sprite(&self, arg0: i32, arg1: i32, arg2: i32, arg3: i32) {
        if arg2 == 256 {
            self.plot_sprite(arg0, arg1);
            return;
        }
        let mut s = pix2d::STATE.lock().unwrap();
        let mut var5 = self.xof + arg0;
        let mut var6 = self.yof + arg1;
        let mut var7 = s.width * var6 + var5;
        let mut var8 = 0i32;
        let mut var9 = self.hi;
        let mut var10 = self.wi;
        let mut var11 = s.width - var10;
        let mut var12 = 0i32;
        if var6 < s.clip_min_y {
            let var13 = s.clip_min_y - var6;
            var9 -= var13;
            var6 = s.clip_min_y;
            var8 += var10 * var13;
            var7 += s.width * var13;
        }
        if var6 + var9 > s.clip_max_y { var9 -= var6 + var9 - s.clip_max_y; }
        if var5 < s.clip_min_x {
            let var14 = s.clip_min_x - var5;
            var10 -= var14;
            var5 = s.clip_min_x;
            var8 += var14;
            var7 += var14;
            var12 += var14;
            var11 += var14;
        }
        if var5 + var10 > s.clip_max_x {
            let var15 = var5 + var10 - s.clip_max_x;
            var10 -= var15;
            var12 += var15;
            var11 += var15;
        }
        if var10 > 0 && var9 > 0 {
            Self::lit_sprite(&mut s.pixels, &self.data, 0, var8, var7, var10, var9, var11, var12, arg2, arg3);
        }
    }

    // @ObfuscatedName("fq.ch([I[IIIIIIIIII)V") — Pix32.litSprite (static)
    pub fn lit_sprite(
        arg0: &mut [i32], arg1: &[i32], _arg2: i32,
        mut arg3: i32, mut arg4: i32, arg5: i32, arg6: i32, arg7: i32, arg8: i32, arg9: i32, arg10: i32,
    ) {
        let var11 = 256 - arg9;
        let var12 = (((arg10 & 0xFF00FF) as u32) * var11 as u32) & 0xFF00FF00;
        let var13 = (((arg10 & 0xFF00) as u32) * var11 as u32) & 0xFF0000;
        let var14 = (var12 | var13) >> 8;
        for _ in (-arg6)..0 {
            for _ in (-arg5)..0 {
                let var17 = arg1[arg3 as usize] as u32;
                arg3 += 1;
                if var17 == 0 {
                    arg4 += 1;
                } else {
                    let var18 = ((var17 & 0xFF00FF) * arg9 as u32) & 0xFF00FF00;
                    let var19 = ((var17 & 0xFF00) * arg9 as u32) & 0xFF0000;
                    arg0[arg4 as usize] = (((var18 | var19) >> 8) + var14) as i32;
                    arg4 += 1;
                }
            }
            arg4 += arg7;
            arg3 += arg8;
        }
    }

    // @ObfuscatedName("fq.cu(III)V") — Pix32.transPlotSprite
    pub fn trans_plot_sprite(&self, x: i32, y: i32, alpha: i32) {
        let mut s = pix2d::STATE.lock().unwrap();
        let mut var4 = self.xof + x;
        let mut var5 = self.yof + y;
        let mut var6 = s.width * var5 + var4;
        let mut var7 = 0i32;
        let mut h = self.hi;
        let mut w = self.wi;
        let mut dst_off = s.width - w;
        let mut src_off = 0i32;
        if var5 < s.clip_min_y {
            let var12 = s.clip_min_y - var5;
            h -= var12;
            var5 = s.clip_min_y;
            var7 += w * var12;
            var6 += s.width * var12;
        }
        if var5 + h > s.clip_max_y { h -= var5 + h - s.clip_max_y; }
        if var4 < s.clip_min_x {
            let var13 = s.clip_min_x - var4;
            w -= var13;
            var4 = s.clip_min_x;
            var7 += var13;
            var6 += var13;
            src_off += var13;
            dst_off += var13;
        }
        if var4 + w > s.clip_max_x {
            let cutoff = var4 + w - s.clip_max_x;
            w -= cutoff;
            src_off += cutoff;
            dst_off += cutoff;
        }
        if w > 0 && h > 0 {
            Self::tran_sprite(&mut s.pixels, &self.data, 0, var7, var6, w, h, dst_off, src_off, alpha);
        }
    }

    // @ObfuscatedName("fq.cc([I[IIIIIIIII)V") — Pix32.tranSprite (static)
    pub fn tran_sprite(
        dst: &mut [i32], src: &[i32], _arg2: i32,
        mut src_off: i32, mut dst_off: i32, w: i32, h: i32, dst_step: i32, src_step: i32, alpha: i32,
    ) {
        let var10 = 256 - alpha;
        for _ in (-h)..0 {
            for _ in (-w)..0 {
                let var13 = src[src_off as usize] as u32;
                src_off += 1;
                if var13 == 0 {
                    dst_off += 1;
                } else {
                    let var14 = dst[dst_off as usize] as u32;
                    let blended = (((var13 & 0xFF00FF) * alpha as u32 + (var14 & 0xFF00FF) * var10 as u32) & 0xFF00FF00)
                        + (((var13 & 0xFF00) * alpha as u32 + (var14 & 0xFF00) * var10 as u32) & 0xFF0000);
                    dst[dst_off as usize] = (blended >> 8) as i32;
                    dst_off += 1;
                }
            }
            dst_off += dst_step;
            src_off += src_step;
        }
    }

    // @ObfuscatedName("fq.cv(IIIIIIDI)V") — Pix32.rotateTransPlotSprite.
    //
    // Rotate-and-plot with transparency-keyed alpha (pixels with raw
    // value 0 pass through). `theta` is in radians, matching Java's
    // signature; for the 2048-step yaw convention, the caller should
    // pre-convert via `theta * 2.0 * PI / 2048.0`.
    //
    // Verbatim port of Pix32.java:702-733 with the Java try/catch
    // swallow replaced by explicit bounds checks.
    pub fn rotate_trans_plot_sprite(
        &self,
        x: i32, y: i32, w: i32, h: i32,
        anchor_x: i32, anchor_y: i32,
        theta: f64, zoom: i32,
    ) {
        let mut s = pix2d::STATE.lock().unwrap();
        let neg_w_2 = -w / 2;
        let neg_h_2 = -h / 2;
        let sin_t = (theta.sin() * 65536.0) as i32;
        let cos_t = (theta.cos() * 65536.0) as i32;
        let step_sx = (zoom * sin_t) >> 8;
        let step_cx = (zoom * cos_t) >> 8;
        let mut row_sx = (anchor_x << 16) + neg_w_2 * step_cx + neg_h_2 * step_sx;
        let mut row_sy = (anchor_y << 16) + (neg_h_2 * step_cx - neg_w_2 * step_sx);
        let mut dst_idx = s.width * y + x;
        for _row in 0..h {
            let mut col_dst = dst_idx;
            let mut col_sx = row_sx;
            let mut col_sy = row_sy;
            for _col in (-w)..0 {
                let sx_int = col_sx >> 16;
                let sy_int = col_sy >> 16;
                if sx_int >= 0 && sx_int < self.wi && sy_int >= 0 && sy_int < self.hi {
                    let src_idx = sx_int + sy_int * self.wi;
                    let pixel = self.data[src_idx as usize];
                    if pixel != 0 {
                        if (col_dst as usize) < s.pixels.len() {
                            s.pixels[col_dst as usize] = pixel;
                        }
                    }
                }
                col_dst += 1;
                col_sx += step_cx;
                col_sy -= step_sx;
            }
            row_sx += step_sx;
            row_sy += step_cx;
            dst_idx += s.width;
        }
    }

    // @ObfuscatedName("fq.ct(IIII)V") — Pix32.pixelPerfectRotateScalePlotSprite
    // (4-arg overload, Pix32.java:737-739). The sprite pivots around its
    // centre (owi/ohi halves in <<4 fixed point) at the given destination
    // centre; `angle` is in 1/65536ths of a turn, `zoom` 4096 = 1:1.
    pub fn pixel_perfect_rotate_scale_plot_sprite_4(
        &self, centre_x: i32, centre_y: i32, angle: i32, zoom: i32,
    ) {
        self.pixel_perfect_rotate_scale_plot_sprite_6(
            self.owi << 3, self.ohi << 3, centre_x << 4, centre_y << 4, angle, zoom,
        );
    }

    // @ObfuscatedName("fq.ck(IIIIII)V") — Pix32.pixelPerfectRotateScalePlotSprite
    // (6-arg, Pix32.java:743-1283). All the fixed-point setup — rotated
    // corner bbox, clip clamp, 24.8 source steps, row starts — is verbatim.
    // Java then specialises the inner walk into 9 sign-cases (cos/sin each
    // 0/neg/pos) whose prologs compute the in-bounds pixel span
    // analytically; the sampling itself is identical in every case, so the
    // port collapses them into one per-pixel-bounds-checked walk that
    // touches exactly the same texels and writes exactly the same pixels.
    //
    // (arg0, arg1) = pivot in sprite space <<4, (arg2, arg3) = destination
    // pivot <<4, arg4 = angle in 1/65536ths of a turn, arg5 = zoom
    // (4096 = 1:1). Pixels with raw value 0 are transparent.
    pub fn pixel_perfect_rotate_scale_plot_sprite_6(
        &self, arg0: i32, arg1: i32, arg2: i32, arg3: i32, arg4: i32, arg5: i32,
    ) {
        if arg5 == 0 {
            return;
        }
        let mut s = pix2d::STATE.lock().unwrap();

        let var7 = arg0 - (self.xof << 4);
        let var8 = arg1 - (self.yof << 4);
        // 9.587379924285257e-5 == 2π / 65536.
        let var9 = (arg4 & 0xFFFF) as f64 * 9.587379924285257e-5;
        let var11 = (var9.sin() * arg5 as f64 + 0.5).floor() as i32;
        let var12 = (var9.cos() * arg5 as f64 + 0.5).floor() as i32;

        // Rotated corner coordinates (sprite bbox in <<12 dest space).
        let var13 = -var7 * var12 + -var8 * var11;
        let var14 = var7 * var11 + -var8 * var12;
        let var15 = ((self.wi << 4) - var7) * var12 + -var8 * var11;
        let var16 = -((self.wi << 4) - var7) * var11 + -var8 * var12;
        let var17 = ((self.hi << 4) - var8) * var11 + -var7 * var12;
        let var18 = ((self.hi << 4) - var8) * var12 + var7 * var11;
        let var19 = ((self.wi << 4) - var7) * var12 + ((self.hi << 4) - var8) * var11;
        let var20 = ((self.hi << 4) - var8) * var12 + -((self.wi << 4) - var7) * var11;

        let (mut var21, mut var22) = if var13 < var15 { (var13, var15) } else { (var15, var13) };
        if var17 < var21 { var21 = var17; }
        if var19 < var21 { var21 = var19; }
        if var17 > var22 { var22 = var17; }
        if var19 > var22 { var22 = var19; }
        let (mut var23, mut var24) = if var14 < var16 { (var14, var16) } else { (var16, var14) };
        if var18 < var23 { var23 = var18; }
        if var20 < var23 { var23 = var20; }
        if var18 > var24 { var24 = var18; }
        if var20 > var24 { var24 = var20; }

        let var25 = var21 >> 12;
        let var26 = (var22 + 4095) >> 12;
        let var27 = var23 >> 12;
        let var28 = (var24 + 4095) >> 12;
        let var29 = arg2 + var25;
        let var30 = arg2 + var26;
        let var31 = arg3 + var27;
        let var32 = arg3 + var28;
        let mut var33 = var29 >> 4;
        let mut var34 = (var30 + 15) >> 4;
        let mut var35 = var31 >> 4;
        let mut var36 = (var32 + 15) >> 4;
        if var33 < s.clip_min_x { var33 = s.clip_min_x; }
        if var34 > s.clip_max_x { var34 = s.clip_max_x; }
        if var35 < s.clip_min_y { var35 = s.clip_min_y; }
        if var36 > s.clip_max_y { var36 = s.clip_max_y; }
        let var37 = var33 - var34;
        if var37 >= 0 {
            return;
        }
        let var38 = var35 - var36;
        if var38 >= 0 {
            return;
        }

        let mut var39 = s.width * var35 + var33;
        // 16777216.0 / zoom — inverse scale in 12.12.
        let var40 = 16777216.0 / arg5 as f64;
        let var42 = (var9.sin() * var40 + 0.5).floor() as i32;
        let var43 = (var9.cos() * var40 + 0.5).floor() as i32;
        let var44 = (var33 << 4) + 8 - arg2;
        let var45 = (var35 << 4) + 8 - arg3;
        let mut var46 = (var7 << 8) - ((var42 * var45) >> 4);
        let mut var47 = (var8 << 8) + ((var43 * var45) >> 4);

        let w_fp = self.wi << 12;
        let h_fp = self.hi << 12;
        for _row in var38..0 {
            let mut dst = var39;
            let mut u = ((var43 * var44) >> 4) + var46;
            let mut v = ((var42 * var44) >> 4) + var47;
            for _px in var37..0 {
                if u >= 0 && v >= 0 && u < w_fp && v < h_fp {
                    let pixel = self.data[((u >> 12) + (v >> 12) * self.wi) as usize];
                    if pixel != 0 {
                        if (dst as usize) < s.pixels.len() {
                            s.pixels[dst as usize] = pixel;
                        }
                    }
                }
                dst += 1;
                u += var43;
                v += var42;
            }
            var46 -= var42;
            var47 += var43;
            var39 += s.width;
        }
    }

    // @ObfuscatedName("fq.cz(IIIIIIII[I[I)V") — Pix32.scanlineRotatePlotSprite
    //
    // Used by TitleScreen to draw the rune ring around the logo. Catches
    // any out-of-bounds access (Java has the same try/catch swallow).
    pub fn scanline_rotate_plot_sprite(
        &self,
        x: i32, y: i32, w: i32, h: i32,
        anchor_x: i32, anchor_y: i32,
        theta: i32, zoom: i32,
        line_start: &[i32], line_width: &[i32],
    ) {
        let mut s = pix2d::STATE.lock().unwrap();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let var11 = -w / 2;
            let var12 = -h / 2;
            let var13 = (f64::sin(theta as f64 / 326.11) * 65536.0) as i32;
            let var14 = (f64::cos(theta as f64 / 326.11) * 65536.0) as i32;
            let var15 = zoom * var13 >> 8;
            let var16 = zoom * var14 >> 8;
            let mut var17 = (anchor_x << 16) + var11 * var16 + var12 * var15;
            let mut var18 = (anchor_y << 16) + (var12 * var16 - var11 * var15);
            let mut var19 = s.width * y + x;
            for var20 in 0..h {
                let var21 = line_start[var20 as usize];
                let mut var22 = var19 + var21;
                let mut var23 = var16 * var21 + var17;
                let mut var24 = var18 - var15 * var21;
                for _ in (-line_width[var20 as usize])..0 {
                    let src_idx = ((var23 >> 16) + (var24 >> 16) * self.wi) as usize;
                    if src_idx < self.data.len() && (var22 as usize) < s.pixels.len() {
                        s.pixels[var22 as usize] = self.data[src_idx];
                    }
                    var22 += 1;
                    var23 += var16;
                    var24 -= var15;
                }
                var17 += var15;
                var18 += var16;
                var19 += s.width;
            }
        }));
        let _ = result;
    }

    // @ObfuscatedName("fq.cy(Lft;II)V") — Pix32.scanlinePlotSprite (uses Pix8 as a mask)
    pub fn scanline_plot_sprite(&self, mask: &Pix8, x: i32, y: i32) {
        let mut s = pix2d::STATE.lock().unwrap();
        if s.clip_max_x - s.clip_min_x != mask.wi || s.clip_max_y - s.clip_min_y != mask.hi {
            panic!("");
        }
        let mut var4 = self.xof + x;
        let mut var5 = self.yof + y;
        let mut var6 = s.width * var5 + var4;
        let mut var7 = 0i32;
        let mut var8 = self.hi;
        let mut var9 = self.wi;
        let mut var10 = s.width - var9;
        let mut var11 = 0i32;
        if var5 < s.clip_min_y {
            let var12 = s.clip_min_y - var5;
            var8 -= var12;
            var5 = s.clip_min_y;
            var7 += var9 * var12;
            var6 += s.width * var12;
        }
        if var5 + var8 > s.clip_max_y { var8 -= var5 + var8 - s.clip_max_y; }
        if var4 < s.clip_min_x {
            let var13 = s.clip_min_x - var4;
            var9 -= var13;
            var4 = s.clip_min_x;
            var7 += var13;
            var6 += var13;
            var11 += var13;
            var10 += var13;
        }
        if var4 + var9 > s.clip_max_x {
            let var14 = var4 + var9 - s.clip_max_x;
            var9 -= var14;
            var11 += var14;
            var10 += var14;
        }
        if var9 > 0 && var8 > 0 {
            let var15 = (var5 - s.clip_min_y) * mask.wi + (var4 - s.clip_min_x);
            let var16 = mask.wi - var9;
            Self::plot_scanline(&mut s.pixels, &self.data, 0, var7, var6, var15, var9, var8, var10, var11, var16, &mask.data);
        }
    }

    // @ObfuscatedName("fq.cq([I[IIIIIIIIII[B)V") — Pix32.plotScanline (static)
    pub fn plot_scanline(
        dst: &mut [i32], src: &[i32], _arg2: i32,
        mut src_off: i32, mut dst_off: i32, mut mask_off: i32, w: i32, h: i32,
        dst_step: i32, src_step: i32, mask_step: i32, mask: &[u8],
    ) {
        let var12 = -(w >> 2);
        let var13 = -(w & 0x3);
        for _ in (-h)..0 {
            for _ in var12..0 {
                for _ in 0..4 {
                    let var16 = src[src_off as usize];
                    src_off += 1;
                    if var16 != 0 && mask[mask_off as usize] == 0 {
                        dst[dst_off as usize] = var16;
                        dst_off += 1;
                    } else {
                        dst_off += 1;
                    }
                    mask_off += 1;
                }
            }
            for _ in var13..0 {
                let var21 = src[src_off as usize];
                src_off += 1;
                if var21 != 0 && mask[mask_off as usize] == 0 {
                    dst[dst_off as usize] = var21;
                    dst_off += 1;
                } else {
                    dst_off += 1;
                }
                mask_off += 1;
            }
            dst_off += dst_step;
            src_off += src_step;
            mask_off += mask_step;
        }
    }
}

impl Default for Pix32 {
    fn default() -> Self {
        Self::new_empty()
    }
}
