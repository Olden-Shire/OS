// @ObfuscatedName("fs")
// jag::oldscape::jstring::PixfontGeneric
//
// Glyph blitter + state for drawString/centreString/etc. The static
// state (currentCol, currentShadow, alpha, etc.) lives on a single
// PixFontStatics mutex so the gamepack-style "set then plot" idiom ports
// cleanly. Markup (`<col=...>` `<str>` `<u>` `<shad>` `<br>` `<lt>`
// `<gt>` `<img=N>`) is parsed inline by drawStringInner / stringWid,
// matching PixFont.java verbatim.

#![allow(dead_code, non_upper_case_globals)]

use std::sync::Mutex;

use super::pix2d;
use super::pix8::Pix8;

// @ObfuscatedName("fs.al") — PixFont.modicons: the mod-crown Pix8
// sprites referenced by `<img=N>` markup. Loaded from the sprites
// archive ("mod_icons") during client startup.
pub static MODICONS: Mutex<Vec<Pix8>> = Mutex::new(Vec::new());

pub fn install_modicons(icons: Vec<Pix8>) {
    *MODICONS.lock().unwrap() = icons;
}

#[derive(Clone)]
pub struct PixFont {
    // @ObfuscatedName("fs.u")
    pub glyphs: Vec<Vec<u8>>,

    // @ObfuscatedName("fs.v")
    pub char_advance: Vec<i32>,

    // @ObfuscatedName("fs.w")
    pub glyph_width: Vec<i32>,

    // @ObfuscatedName("fs.e")
    pub glyph_height: Vec<i32>,

    // @ObfuscatedName("fs.b")
    pub glyph_offset_x: Vec<i32>,

    // @ObfuscatedName("fs.y")
    pub glyph_offset_y: Vec<i32>,

    // @ObfuscatedName("fs.t")
    pub ascent: i32,

    // @ObfuscatedName("fs.f")
    pub max_ascent: i32,

    // @ObfuscatedName("fs.k")
    pub max_descent: i32,

    // @ObfuscatedName("fs.a")
    pub kerning_pairs: Vec<i8>,
}

// custom — pulls the per-call `fs.h/x/p/ad/ac/aa/as/am/ap` statics into a
// single Mutex so drawString/plotLetter can read/write coherently.
pub struct PixFontStatics {
    // @ObfuscatedName("fs.h")
    pub strikeout: i32,
    // @ObfuscatedName("fs.x")
    pub underline: i32,
    // @ObfuscatedName("fs.p")
    pub default_shadow: i32,
    // @ObfuscatedName("fs.ad")
    pub current_shadow: i32,
    // @ObfuscatedName("fs.ac")
    pub default_col: i32,
    // @ObfuscatedName("fs.aa")
    pub current_col: i32,
    // @ObfuscatedName("fs.as")
    pub alpha: i32,
    // @ObfuscatedName("fs.am")
    pub extra_space_width: i32,
    // @ObfuscatedName("fs.ap")
    pub extra_space_pos: i32,
}

impl PixFontStatics {
    pub const fn new() -> Self {
        Self {
            strikeout: -1,
            underline: -1,
            default_shadow: -1,
            current_shadow: -1,
            default_col: 0,
            current_col: 0,
            alpha: 256,
            extra_space_width: 0,
            extra_space_pos: 0,
        }
    }
}

pub static STATICS: Mutex<PixFontStatics> = Mutex::new(PixFontStatics::new());

// java.util.Random's 48-bit LCG — drawStringAntiMacro's jitter must
// match Java bit-for-bit so the rendered feedback line diffs clean.
pub struct JavaRandom {
    seed: i64,
}

impl JavaRandom {
    pub fn new(seed: i64) -> Self {
        Self { seed: (seed ^ 0x5DEECE66D) & ((1 << 48) - 1) }
    }
    pub fn next_int(&mut self) -> i32 {
        self.seed = self.seed.wrapping_mul(0x5DEECE66D).wrapping_add(0xB) & ((1 << 48) - 1);
        (self.seed >> 16) as i32
    }
}

impl PixFont {
    pub fn new() -> Self {
        Self {
            glyphs: (0..256).map(|_| Vec::new()).collect(),
            char_advance: vec![0; 256],
            glyph_width: Vec::new(),
            glyph_height: Vec::new(),
            glyph_offset_x: Vec::new(),
            glyph_offset_y: Vec::new(),
            ascent: 0,
            max_ascent: 0,
            max_descent: 0,
            kerning_pairs: Vec::new(),
        }
    }

    // Java ctor `PixFont(byte[] metrics, int[] xof, int[] yof, int[] wi, int[] hi, int[] adv, byte[][] glyphs)`
    pub fn from_parts(
        metrics: &[u8],
        glyph_offset_x: Vec<i32>,
        glyph_offset_y: Vec<i32>,
        glyph_width: Vec<i32>,
        glyph_height: Vec<i32>,
        _adv: Vec<i32>,
        glyphs: Vec<Vec<u8>>,
    ) -> Self {
        let mut me = Self::new();
        me.glyph_offset_x = glyph_offset_x;
        me.glyph_offset_y = glyph_offset_y;
        me.glyph_width = glyph_width;
        me.glyph_height = glyph_height;
        me.unpack_metrics(metrics);
        me.glyphs = glyphs;
        let mut min_y = i32::MAX;
        let mut max_y = i32::MIN;
        for var10 in 0..256.min(me.glyph_offset_y.len()) {
            if var10 < me.glyph_height.len() && me.glyph_offset_y[var10] < min_y && me.glyph_height[var10] != 0 {
                min_y = me.glyph_offset_y[var10];
            }
            if var10 < me.glyph_height.len() && me.glyph_height[var10] + me.glyph_offset_y[var10] > max_y {
                max_y = me.glyph_height[var10] + me.glyph_offset_y[var10];
            }
        }
        if min_y == i32::MAX { min_y = 0; }
        if max_y == i32::MIN { max_y = 0; }
        me.max_ascent = me.ascent - min_y;
        me.max_descent = max_y - me.ascent;
        me
    }

    // @ObfuscatedName("fs.bm([B)V") — PixFont.unpackMetrics
    pub fn unpack_metrics(&mut self, src: &[u8]) {
        self.char_advance = vec![0; 256];
        if src.len() == 257 {
            for var2 in 0..self.char_advance.len() {
                self.char_advance[var2] = src[var2] as i32 & 0xFF;
            }
            self.ascent = src[256] as i32 & 0xFF;
            return;
        }
        let mut var3 = 0usize;
        for var4 in 0..256 {
            self.char_advance[var4] = src[var3] as i32 & 0xFF;
            var3 += 1;
        }
        let mut var5 = vec![0i32; 256]; // glyph wi
        let mut var6 = vec![0i32; 256]; // glyph hi
        for var7 in 0..256 {
            var5[var7] = src[var3] as i32 & 0xFF;
            var3 += 1;
        }
        for var8 in 0..256 {
            var6[var8] = src[var3] as i32 & 0xFF;
            var3 += 1;
        }
        // Delta-decoded left/right glyph edge profiles (Java var9/var13),
        // then the 65536-entry kerning pair table via kernPair.
        let mut var9: Vec<Vec<i8>> = Vec::with_capacity(256);
        for var10 in 0..256 {
            let mut profile = vec![0i8; var5[var10].max(0) as usize];
            let mut var11: i8 = 0;
            for slot in profile.iter_mut() {
                var11 = var11.wrapping_add(src[var3] as i8);
                var3 += 1;
                *slot = var11;
            }
            var9.push(profile);
        }
        let mut var13: Vec<Vec<i8>> = Vec::with_capacity(256);
        for var14 in 0..256 {
            let mut profile = vec![0i8; var5[var14].max(0) as usize];
            let mut var15: i8 = 0;
            for slot in profile.iter_mut() {
                var15 = var15.wrapping_add(src[var3] as i8);
                var3 += 1;
                *slot = var15;
            }
            var13.push(profile);
        }
        self.kerning_pairs = vec![0i8; 65536];
        for var17 in 0..256 {
            if var17 != 32 && var17 != 160 {
                for var18 in 0..256 {
                    if var18 != 32 && var18 != 160 {
                        self.kerning_pairs[(var17 << 8) + var18] =
                            Self::kern_pair(&var9, &var13, &var6, &self.char_advance, &var5, var17, var18) as i8;
                    }
                }
            }
        }
        self.ascent = var5[32] + var6[32];
    }

    // @ObfuscatedName("fs.bn([[B[[B[I[I[III)I") — PixFont.kernPair.
    // arg0 = left-edge profiles, arg1 = right-edge profiles, arg2 = the
    // per-char profile y-offsets (var6), arg3 = charAdvance, arg4 = the
    // per-char profile lengths (var5).
    fn kern_pair(
        arg0: &[Vec<i8>], arg1: &[Vec<i8>], arg2: &[i32],
        arg3: &[i32], arg4: &[i32], arg5: usize, arg6: usize,
    ) -> i32 {
        let var7 = arg2[arg5];
        let var8 = arg4[arg5] + var7;
        let var9 = arg2[arg6];
        let var10 = arg4[arg6] + var9;
        let mut var11 = var7;
        if var9 > var7 {
            var11 = var9;
        }
        let mut var12 = var8;
        if var10 < var8 {
            var12 = var10;
        }
        let mut var13 = arg3[arg5];
        if arg3[arg6] < var13 {
            var13 = arg3[arg6];
        }
        let var14 = &arg1[arg5];
        let var15 = &arg0[arg6];
        let mut var16 = (var11 - var7) as usize;
        let mut var17 = (var11 - var9) as usize;
        for _ in var11..var12 {
            let var19 = var14[var16] as i32 + var15[var17] as i32;
            var16 += 1;
            var17 += 1;
            if var19 < var13 {
                var13 = var19;
            }
        }
        -var13
    }

    // @ObfuscatedName("fs.be(I)I") — PixFont.charWid
    pub fn char_wid(&self, mut arg0: i32) -> i32 {
        if arg0 == 160 {
            arg0 = 32;
        }
        self.char_advance[(arg0 & 0xFF) as usize]
    }

    // @ObfuscatedName("fs.cc(Ljava/lang/String;I)V") —
    // PixFont.calculateSpaceWidth. Verbatim port of PixFont.java:
    // 607-623, but refactored as a pure function that returns the
    // computed extra-space-width instead of writing to the static
    // extra_space_width slot. Returns 0 if the line has no spaces
    // outside markup (Java's no-op branch). The result is in 8.8
    // fixed point — callers add this to each space's advance to
    // justify the line to `target_width`.
    pub fn calc_extra_space_width(&self, line: &str, target_width: i32) -> i32 {
        let mut spaces: i32 = 0;
        let mut in_tag = false;
        for ch in line.chars() {
            if ch == '<' { in_tag = true; continue; }
            if ch == '>' { in_tag = false; continue; }
            if !in_tag && ch == ' ' { spaces += 1; }
        }
        if spaces <= 0 { return 0; }
        ((target_width - self.string_wid(line)) << 8) / spaces
    }

    // Kerning-aware advance: returns char_advance[c] plus the per-pair
    // kerning delta from kerning_pairs[(prev << 8) | c]. Java's
    // stringWid loop applies this delta inline; we hoist it so the
    // eventual markup-aware width / draw paths can share one source.
    // `prev` < 0 means "no previous char" (line start) — kerning skipped.
    pub fn char_wid_kern(&self, prev: i32, c: i32) -> i32 {
        let cu = (c & 0xFF) as usize;
        let mut w = self.char_advance.get(cu).copied().unwrap_or(0);
        if prev >= 0 && !self.kerning_pairs.is_empty() {
            let key = (((prev & 0xFF) << 8) | (c & 0xFF)) as usize;
            if let Some(&delta) = self.kerning_pairs.get(key) {
                w += delta as i32;
            }
        }
        w
    }

    // @ObfuscatedName("fs.bp(Ljava/lang/String;)I") — PixFont.stringWid.
    // Verbatim port of PixFont.java:218-263: markup-aware width —
    // tags contribute nothing, `<lt>`/`<gt>` count as one glyph,
    // `<img=N>` counts the icon width, kerning pairs apply.
    pub fn string_wid(&self, arg0: &str) -> i32 {
        let chars: Vec<char> = arg0.chars().collect();
        let mut tag_start: i32 = -1;
        let mut prev: i32 = -1;
        let mut width = 0i32;
        for i in 0..chars.len() {
            let mut ch = chars[i] as i32;
            if ch == '<' as i32 {
                tag_start = i as i32;
                continue;
            }
            if ch == '>' as i32 && tag_start != -1 {
                let tag: String = chars[(tag_start + 1) as usize..i].iter().collect();
                tag_start = -1;
                if tag == "lt" {
                    ch = '<' as i32;
                } else if tag == "gt" {
                    ch = '>' as i32;
                } else {
                    if let Some(rest) = tag.strip_prefix("img=") {
                        if let Ok(idx) = rest.parse::<usize>() {
                            if let Some(icon) = MODICONS.lock().unwrap().get(idx) {
                                width += icon.owi;
                                prev = -1;
                            }
                        }
                    }
                    continue;
                }
            }
            if ch == 160 {
                ch = ' ' as i32;
            }
            if tag_start == -1 && (0..256).contains(&ch) {
                width += self.char_advance[ch as usize];
                if !self.kerning_pairs.is_empty() && prev != -1 {
                    width += self.kerning_pairs[((prev << 8) + ch) as usize] as i32;
                }
                prev = ch;
            }
        }
        width
    }

    // @ObfuscatedName("fs.bv(II)V") — PixFont.resetState
    pub fn reset_state(&self, rgb: i32, shadow: i32) {
        let mut s = STATICS.lock().unwrap();
        s.default_col = rgb;
        s.current_col = rgb;
        s.default_shadow = shadow;
        s.current_shadow = shadow;
        s.alpha = 256;
        s.strikeout = -1;
        s.underline = -1;
        s.extra_space_width = 0;
        s.extra_space_pos = 0;
    }

    // @ObfuscatedName("fs.cu(Ljava/lang/String;)V") — PixFont.updateState.
    // Verbatim port of PixFont.java:573-603. Consumes a markup tag
    // (the chars *between* `<` and `>`) and mutates the active draw
    // state. Java wraps the body in `try { } catch (Exception) {}` —
    // we mirror that by silently dropping any malformed hex/numeric.
    pub fn update_state(&self, tag: &str) {
        let mut s = STATICS.lock().unwrap();
        let parse_hex = |t: &str| i32::from_str_radix(t, 16).ok();
        if let Some(rest) = tag.strip_prefix("col=") {
            if let Some(v) = parse_hex(rest) { s.current_col = v; }
        } else if tag == "/col" {
            s.current_col = s.default_col;
        } else if let Some(rest) = tag.strip_prefix("str=") {
            if let Some(v) = parse_hex(rest) { s.strikeout = v; }
        } else if tag == "str" {
            s.strikeout = 0x800000;
        } else if tag == "/str" {
            s.strikeout = -1;
        } else if let Some(rest) = tag.strip_prefix("u=") {
            if let Some(v) = parse_hex(rest) { s.underline = v; }
        } else if tag == "u" {
            s.underline = 0;
        } else if tag == "/u" {
            s.underline = -1;
        } else if let Some(rest) = tag.strip_prefix("shad=") {
            if let Some(v) = parse_hex(rest) { s.current_shadow = v; }
        } else if tag == "shad" {
            s.current_shadow = 0;
        } else if tag == "/shad" {
            s.current_shadow = s.default_shadow;
        } else if tag == "br" {
            // Java calls resetState(defaultCol, defaultShadow); inline
            // here since we already hold the lock.
            let dc = s.default_col;
            let ds = s.default_shadow;
            s.default_col = dc;
            s.current_col = dc;
            s.default_shadow = ds;
            s.current_shadow = ds;
            s.alpha = 256;
            s.strikeout = -1;
            s.underline = -1;
            s.extra_space_width = 0;
            s.extra_space_pos = 0;
        }
    }

    // @ObfuscatedName("fs.bd(Ljava/lang/String;IIII)V") — PixFont.drawString
    pub fn draw_string(&self, str: &str, x: i32, y: i32, rgb: i32, shadow: i32) {
        self.reset_state(rgb, shadow);
        self.draw_string_inner(str, x, y);
    }

    // @ObfuscatedName("fs.cs(Ljava/lang/String;IIII)V") — PixFont.centreString
    pub fn centre_string(&self, str: &str, x: i32, y: i32, rgb: i32, shadow: i32) {
        self.reset_state(rgb, shadow);
        self.draw_string_inner(str, x - self.string_wid(str) / 2, y);
    }

    // @ObfuscatedName("fs.cr(Ljava/lang/String;IIII)V") — PixFont.rightString
    pub fn right_string(&self, str: &str, x: i32, y: i32, rgb: i32, shadow: i32) {
        self.reset_state(rgb, shadow);
        self.draw_string_inner(str, x - self.string_wid(str), y);
    }

    // @ObfuscatedName("fs.cm(Ljava/lang/String;II)V") — PixFont.drawStringInner.
    // Verbatim port of PixFont.java:627-709 via the shared offsets
    // walker (the custom variant with both offset arrays absent is
    // byte-for-byte the same logic).
    pub fn draw_string_inner(&self, str: &str, x: i32, y: i32) {
        self.draw_string_inner_offsets(str, x, y, None, None);
    }

    // @ObfuscatedName("fs.cw(Ljava/lang/String;II[I[I)V") —
    // PixFont.drawStringInnerCustomOffsetsAndColours. Verbatim port of
    // PixFont.java:713-822: full markup walk (`<col=>` `<str>` `<u>`
    // `<shad>` `<br>` `<lt>` `<gt>` `<img=N>`), kerning pairs,
    // per-visible-char (x, y) offsets for the wave/shake chat effects,
    // strikeout / underline hlines, extra-space justification.
    pub fn draw_string_inner_offsets(&self, str: &str, mut x: i32, y: i32,
                                     x_offsets: Option<&[i32]>, y_offsets: Option<&[i32]>) {
        let base_y = y - self.ascent;
        let chars: Vec<char> = str.chars().collect();
        let mut tag_start: i32 = -1;
        let mut prev: i32 = -1;
        let mut offset_idx = 0usize;
        for i in 0..chars.len() {
            let mut ch = chars[i] as i32;
            if ch == '<' as i32 {
                tag_start = i as i32;
                continue;
            }
            if ch == '>' as i32 && tag_start != -1 {
                let tag: String = chars[(tag_start + 1) as usize..i].iter().collect();
                tag_start = -1;
                if tag == "lt" {
                    ch = '<' as i32;
                } else if tag == "gt" {
                    ch = '>' as i32;
                } else {
                    if let Some(rest) = tag.strip_prefix("img=") {
                        let dx = x_offsets.and_then(|o| o.get(offset_idx).copied()).unwrap_or(0);
                        let dy = y_offsets.and_then(|o| o.get(offset_idx).copied()).unwrap_or(0);
                        offset_idx += 1;
                        if let Ok(idx) = rest.parse::<usize>() {
                            let icons = MODICONS.lock().unwrap();
                            if let Some(icon) = icons.get(idx) {
                                icon.plot_sprite(x + dx, self.ascent + base_y - icon.ohi + dy);
                                x += icon.owi;
                                prev = -1;
                            }
                        }
                    } else {
                        self.update_state(&tag);
                    }
                    continue;
                }
            }
            if ch == 160 {
                ch = ' ' as i32;
            }
            if tag_start != -1 || !(0..256).contains(&ch) {
                continue;
            }
            let cu = ch as usize;
            if !self.kerning_pairs.is_empty() && prev != -1 {
                x += self.kerning_pairs[((prev << 8) + ch) as usize] as i32;
            }
            let (current_col, current_shadow, alpha, strikeout, underline,
                 extra_space_width) = {
                let s = STATICS.lock().unwrap();
                (s.current_col, s.current_shadow, s.alpha, s.strikeout, s.underline,
                 s.extra_space_width)
            };
            let glyph_w = self.glyph_width.get(cu).copied().unwrap_or(0);
            let glyph_h = self.glyph_height.get(cu).copied().unwrap_or(0);
            let dx = x_offsets.and_then(|o| o.get(offset_idx).copied()).unwrap_or(0);
            let dy = y_offsets.and_then(|o| o.get(offset_idx).copied()).unwrap_or(0);
            offset_idx += 1;
            if ch == ' ' as i32 {
                if extra_space_width > 0 {
                    let mut s = STATICS.lock().unwrap();
                    s.extra_space_pos += s.extra_space_width;
                    x += s.extra_space_pos >> 8;
                    s.extra_space_pos &= 0xFF;
                }
            } else if alpha == 256 {
                if current_shadow != -1 {
                    Self::plot_letter(&self.glyphs[cu],
                                      self.glyph_offset_x[cu] + x + 1 + dx,
                                      self.glyph_offset_y[cu] + base_y + 1 + dy,
                                      glyph_w, glyph_h, current_shadow);
                }
                Self::plot_letter(&self.glyphs[cu],
                                  self.glyph_offset_x[cu] + x + dx,
                                  self.glyph_offset_y[cu] + base_y + dy,
                                  glyph_w, glyph_h, current_col);
            } else {
                if current_shadow != -1 {
                    Self::plot_letter_trans(&self.glyphs[cu],
                                            self.glyph_offset_x[cu] + x + 1 + dx,
                                            self.glyph_offset_y[cu] + base_y + 1 + dy,
                                            glyph_w, glyph_h, current_shadow, alpha);
                }
                Self::plot_letter_trans(&self.glyphs[cu],
                                        self.glyph_offset_x[cu] + x + dx,
                                        self.glyph_offset_y[cu] + base_y + dy,
                                        glyph_w, glyph_h, current_col, alpha);
            }
            let advance = self.char_advance[cu];
            if strikeout != -1 {
                pix2d::fill_rect(x, (self.ascent as f64 * 0.7) as i32 + base_y,
                                 advance, 1, strikeout);
            }
            if underline != -1 {
                // Java's two inner walkers disagree by one pixel: plain
                // drawStringInner underlines at ascent+y+1 (PixFont.java:702)
                // while the custom-offsets variant — the one the wave/shake
                // effects use — omits the +1 (PixFont.java:815).
                let plain = x_offsets.is_none() && y_offsets.is_none();
                pix2d::fill_rect(x, self.ascent + base_y + if plain { 1 } else { 0 },
                                 advance, 1, underline);
            }
            x += advance;
            prev = ch;
        }
    }

    // @ObfuscatedName("fs.cl(Ljava/lang/String;IIIII)V") —
    // PixFont.centerStringWave. Verbatim port of PixFont.java:492-502.
    pub fn centre_string_wave(&self, str: &str, x: i32, y: i32, rgb: i32, shadow: i32,
                              cycle: i32) {
        self.reset_state(rgb, shadow);
        let n = str.chars().count();
        let y_off: Vec<i32> = (0..n)
            .map(|i| ((cycle as f64 / 5.0 + i as f64 / 2.0).sin() * 5.0) as i32)
            .collect();
        self.draw_string_inner_offsets(str, x - self.string_wid(str) / 2, y,
                                       None, Some(&y_off));
    }

    // @ObfuscatedName("fs.cp(Ljava/lang/String;IIIII)V") —
    // PixFont.centreStringWave2. Verbatim port of PixFont.java:506-518.
    pub fn centre_string_wave2(&self, str: &str, x: i32, y: i32, rgb: i32, shadow: i32,
                               cycle: i32) {
        self.reset_state(rgb, shadow);
        let n = str.chars().count();
        let x_off: Vec<i32> = (0..n)
            .map(|i| ((cycle as f64 / 5.0 + i as f64 / 5.0).sin() * 5.0) as i32)
            .collect();
        let y_off: Vec<i32> = (0..n)
            .map(|i| ((cycle as f64 / 5.0 + i as f64 / 3.0).sin() * 5.0) as i32)
            .collect();
        self.draw_string_inner_offsets(str, x - self.string_wid(str) / 2, y,
                                       Some(&x_off), Some(&y_off));
    }

    // @ObfuscatedName("fs.co(Ljava/lang/String;IIIII)V") —
    // PixFont.drawStringAntiMacro. Verbatim port of PixFont.java:
    // 540-556: java.util.Random seeded with `seed` jitters the alpha
    // (192..223) and accumulates 1px x-drift on ~25% of glyphs — the
    // anti-OCR treatment on the top-left minimenu feedback line.
    pub fn draw_string_anti_macro(&self, str: &str, x: i32, y: i32, rgb: i32, shadow: i32,
                                  seed: i32) {
        self.reset_state(rgb, shadow);
        let mut rng = JavaRandom::new(seed as i64);
        STATICS.lock().unwrap().alpha = (rng.next_int() & 0x1F) + 192;
        let n = str.chars().count();
        let mut x_off = vec![0i32; n];
        let mut drift = 0;
        for slot in x_off.iter_mut() {
            *slot = drift;
            if (rng.next_int() & 0x3) == 0 {
                drift += 1;
            }
        }
        self.draw_string_inner_offsets(str, x, y, Some(&x_off), None);
    }

    // @ObfuscatedName("fs.ca(Ljava/lang/String;IIIIII)V") —
    // PixFont.centreStringWave3. Verbatim port of PixFont.java:522-536.
    pub fn centre_string_wave3(&self, str: &str, x: i32, y: i32, rgb: i32, shadow: i32,
                               cycle: i32, elapsed: i32) {
        self.reset_state(rgb, shadow);
        let mut amplitude = 7.0 - elapsed as f64 / 8.0;
        if amplitude < 0.0 {
            amplitude = 0.0;
        }
        let n = str.chars().count();
        let y_off: Vec<i32> = (0..n)
            .map(|i| ((cycle as f64 + i as f64 / 1.5).sin() * amplitude) as i32)
            .collect();
        self.draw_string_inner_offsets(str, x - self.string_wid(str) / 2, y,
                                       None, Some(&y_off));
    }

    // @ObfuscatedName("fs.ct([BIIIII)V") — PixFont.plotLetter
    pub fn plot_letter(arg0: &[u8], mut arg1: i32, mut arg2: i32, mut arg3: i32, mut arg4: i32, arg5: i32) {
        if arg0.is_empty() || arg3 <= 0 || arg4 <= 0 {
            return;
        }
        let mut s = pix2d::STATE.lock().unwrap();
        let width = s.width;
        let clip_min_x = s.clip_min_x;
        let clip_min_y = s.clip_min_y;
        let clip_max_x = s.clip_max_x;
        let clip_max_y = s.clip_max_y;
        let mut var6 = width * arg2 + arg1;
        let mut var7 = width - arg3;
        let mut var8 = 0i32;
        let mut var9 = 0i32;
        if arg2 < clip_min_y {
            let var10 = clip_min_y - arg2;
            arg4 -= var10;
            arg2 = clip_min_y;
            var9 += arg3 * var10;
            var6 += width * var10;
        }
        if arg2 + arg4 > clip_max_y {
            arg4 -= arg2 + arg4 - clip_max_y;
        }
        if arg1 < clip_min_x {
            let var11 = clip_min_x - arg1;
            arg3 -= var11;
            arg1 = clip_min_x;
            var9 += var11;
            var6 += var11;
            var8 += var11;
            var7 += var11;
        }
        if arg1 + arg3 > clip_max_x {
            let var12 = arg1 + arg3 - clip_max_x;
            arg3 -= var12;
            var8 += var12;
            var7 += var12;
        }
        if arg3 > 0 && arg4 > 0 {
            // Inline of `plot` — for each glyph byte that's non-zero,
            // write the current colour.
            let mut src = var9 as usize;
            let mut dst = var6 as usize;
            for _ in 0..arg4 {
                for _ in 0..arg3 {
                    if src < arg0.len() && dst < s.pixels.len() {
                        if arg0[src] != 0 {
                            s.pixels[dst] = arg5;
                        }
                    }
                    src += 1;
                    dst += 1;
                }
                dst = dst.wrapping_add(var7 as usize);
                src = src.wrapping_add(var8 as usize);
            }
        }
    }

    // @ObfuscatedName("fs.cy([BIIIIII)V") — PixFont.plotLetterTrans
    pub fn plot_letter_trans(arg0: &[u8], mut arg1: i32, mut arg2: i32, mut arg3: i32, mut arg4: i32, arg5: i32, arg6: i32) {
        if arg0.is_empty() || arg3 <= 0 || arg4 <= 0 {
            return;
        }
        let mut s = pix2d::STATE.lock().unwrap();
        let width = s.width;
        let clip_min_x = s.clip_min_x;
        let clip_min_y = s.clip_min_y;
        let clip_max_x = s.clip_max_x;
        let clip_max_y = s.clip_max_y;
        let mut var7 = width * arg2 + arg1;
        let mut var8 = width - arg3;
        let mut var9 = 0i32;
        let mut var10 = 0i32;
        if arg2 < clip_min_y {
            let var11 = clip_min_y - arg2;
            arg4 -= var11;
            arg2 = clip_min_y;
            var10 += arg3 * var11;
            var7 += width * var11;
        }
        if arg2 + arg4 > clip_max_y {
            arg4 -= arg2 + arg4 - clip_max_y;
        }
        if arg1 < clip_min_x {
            let var12 = clip_min_x - arg1;
            arg3 -= var12;
            arg1 = clip_min_x;
            var10 += var12;
            var7 += var12;
            var9 += var12;
            var8 += var12;
        }
        if arg1 + arg3 > clip_max_x {
            let var13 = arg1 + arg3 - clip_max_x;
            arg3 -= var13;
            var9 += var13;
            var8 += var13;
        }
        if arg3 > 0 && arg4 > 0 {
            let var10_ = ((((arg5 & 0xFF00FF) as u32 * arg6 as u32) & 0xFF00FF00)
                + (((arg5 & 0xFF00) as u32 * arg6 as u32) & 0xFF0000)) >> 8;
            let var11 = 256 - arg6;
            let mut src = var10 as usize;
            let mut dst = var7 as usize;
            for _ in 0..arg4 {
                for _ in 0..arg3 {
                    if src < arg0.len() && dst < s.pixels.len() {
                        if arg0[src] != 0 {
                            let var14 = s.pixels[dst] as u32;
                            let blended = ((((var14 & 0xFF00FF) * var11 as u32) & 0xFF00FF00)
                                + (((var14 & 0xFF00) * var11 as u32) & 0xFF0000)) >> 8;
                            s.pixels[dst] = (blended + var10_) as i32;
                        }
                    }
                    src += 1;
                    dst += 1;
                }
                dst = dst.wrapping_add(var8 as usize);
                src = src.wrapping_add(var9 as usize);
            }
        }
    }
}

impl Default for PixFont {
    fn default() -> Self {
        Self::new()
    }
}

// @ObfuscatedName("fs.bn([[B[[B[I[I[III)I") — PixFont.kernPair.
// Verbatim port of PixFont.java:177-205. Computes the tight kerning
// distance between two glyphs by sweeping the overlap region of
// their left/right edge profile arrays and returning the minimum
// horizontal slack (negated so callers can `x += kernPair(...)`).
//
// `left_profile` / `right_profile` are the per-row left/right edge
// arrays for the glyph cache; `widths` / `heights` / `vertical_offsets`
// are the per-glyph metrics; `glyph_a` precedes `glyph_b` in the
// text — the result is the amount you can shift `glyph_b` left
// before its outline collides with `glyph_a`.
pub fn kern_pair(
    left_profile: &[Vec<i8>],
    right_profile: &[Vec<i8>],
    vertical_offsets: &[i32],
    heights: &[i32],
    widths: &[i32],
    glyph_a: usize,
    glyph_b: usize,
) -> i32 {
    let v7 = vertical_offsets[glyph_a];
    let v8 = heights[glyph_a] + v7;
    let v9 = vertical_offsets[glyph_b];
    let v10 = heights[glyph_b] + v9;
    let mut v11 = v7;
    if v9 > v7 { v11 = v9; }
    let mut v12 = v8;
    if v10 < v8 { v12 = v10; }
    let mut v13 = widths[glyph_a];
    if widths[glyph_b] < v13 { v13 = widths[glyph_b]; }
    let v14 = &right_profile[glyph_a];
    let v15 = &left_profile[glyph_b];
    let mut v16 = (v11 - v7) as usize;
    let mut v17 = (v11 - v9) as usize;
    for _ in v11..v12 {
        let v19 = (v14[v16] as i32) + (v15[v17] as i32);
        if v19 < v13 { v13 = v19; }
        v16 += 1;
        v17 += 1;
    }
    -v13
}

impl PixFont {
    // @ObfuscatedName("y.aw(Ljava/lang/String;B)Ljava/lang/String;") —
    // PixFont.escape. Converts raw `<` / `>` into `<lt>` / `<gt>` so
    // user-supplied text doesn't accidentally trigger the markup
    // parser. Verbatim port of PixFont.java:382.
    pub fn escape(s: &str) -> String {
        s.replace('<', "<lt>").replace('>', "<gt>")
    }

    // @ObfuscatedName("y.ar(Ljava/lang/String;II)[Ljava/lang/String;") —
    // PixFont.splitString. Splits `s` into rendered lines that each
    // fit within `max_width` pixels. Word-wrap is by spaces; an
    // explicit `<br>` forces a break. Returned strings keep their
    // original casing but strip the `<br>` markers.
    //
    // Simplified port: no markup-aware kerning, no `<col=>` handling
    // (we measure ASCII-equivalent widths). Sufficient for the
    // chatbox / tooltip / dialogue paths that don't embed colour
    // codes mid-line.
    pub fn split_string(&self, s: &str, max_width: i32) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        let mut current = String::new();
        let mut current_w = 0i32;
        let space_w = self.string_wid(" ");
        // Java's parser walks looking for `<br>` and spaces; mirror
        // that.
        for chunk in s.split("<br>") {
            for word in chunk.split_whitespace() {
                let ww = self.string_wid(word);
                let needed = if current.is_empty() { ww } else { current_w + space_w + ww };
                if needed > max_width && !current.is_empty() {
                    lines.push(std::mem::take(&mut current));
                    current_w = 0;
                }
                if current.is_empty() {
                    current.push_str(word);
                    current_w = ww;
                } else {
                    current.push(' ');
                    current.push_str(word);
                    current_w += space_w + ww;
                }
            }
            // `<br>` forces a line break.
            lines.push(std::mem::take(&mut current));
            current_w = 0;
        }
        // The split above always pushes an empty trailing line — drop
        // it if so.
        if let Some(last) = lines.last() {
            if last.is_empty() {
                lines.pop();
            }
        }
        if lines.is_empty() && !s.is_empty() {
            lines.push(s.to_string());
        }
        lines
    }

    // @ObfuscatedName("y.ai(Ljava/lang/String;II)I") —
    // PixFont.predictWidthMultiline. Returns the widest line's width
    // after split_string. Used by tooltip + interface containers to
    // size their bounding boxes.
    pub fn predict_width_multiline(&self, s: &str, max_width: i32) -> i32 {
        self.split_string(s, max_width)
            .iter()
            .map(|line| self.string_wid(line))
            .max()
            .unwrap_or(0)
    }

    // @ObfuscatedName("y.aj(Ljava/lang/String;II)I") —
    // PixFont.predictLinesMultiline. Returns the line count after
    // split_string.
    pub fn predict_lines_multiline(&self, s: &str, max_width: i32) -> i32 {
        self.split_string(s, max_width).len() as i32
    }

    // @ObfuscatedName("y.ay(Ljava/lang/String;IIIIIIIBI)V") —
    // PixFont.drawStringMultiline. Verbatim port of PixFont.java:
    // 440-488. `v_align`: 0 top, 1 centre, 2 bottom, 3 spread;
    // `h_align`: 0 left, 1 centre, 2 right, 3 justify. `line_h` 0 =
    // default (ascent). Baselines anchor on max_ascent/max_descent —
    // approximating with the line height sits centred text a few
    // pixels low. State resets ONCE up front so markup (colour /
    // underline) carries across wrapped lines like Java.
    pub fn draw_string_multiline(
        &self,
        s: &str,
        x: i32, y: i32, w: i32, h: i32,
        colour: i32, shadow: i32,
        h_align: i32, v_align: i32,
        line_h: i32,
    ) {
        self.reset_state(colour, shadow);
        let mut lh = if line_h == 0 { self.ascent } else { line_h };
        // Java 449-451: when the component can't fit two lines, wrapping
        // is disabled outright (splitString gets a null width array).
        let wrap_width = if h < self.max_ascent + self.max_descent + lh && h < lh + lh {
            i32::MAX
        } else {
            w
        };
        let lines = self.split_string(s, wrap_width);
        let n = lines.len() as i32;
        let mut v_align = v_align;
        if v_align == 3 && n == 1 {
            v_align = 1;
        }
        let mut baseline = match v_align {
            0 => y + self.max_ascent,
            1 => (h - self.max_ascent - self.max_descent - (n - 1) * lh) / 2
                + self.max_ascent + y,
            2 => y + h - self.max_descent - (n - 1) * lh,
            _ => {
                let pad = ((h - self.max_ascent - self.max_descent - (n - 1) * lh)
                    / (n + 1)).max(0);
                lh += pad;
                y + self.max_ascent + pad
            }
        };
        for (i, line) in lines.iter().enumerate() {
            match h_align {
                0 => self.draw_string_inner(line, x, baseline),
                1 => self.draw_string_inner(
                    line, x + (w - self.string_wid(line)) / 2, baseline),
                2 => self.draw_string_inner(
                    line, x + w - self.string_wid(line), baseline),
                _ => {
                    if i == lines.len() - 1 {
                        // Last line of a justified block is drawn plain.
                        self.draw_string_inner(line, x, baseline);
                    } else {
                        let extra = self.calc_extra_space_width(line, w);
                        {
                            let mut st = STATICS.lock().unwrap();
                            st.extra_space_width = extra;
                            st.extra_space_pos = 0;
                        }
                        self.draw_string_inner(line, x, baseline);
                        STATICS.lock().unwrap().extra_space_width = 0;
                    }
                }
            }
            baseline += lh;
        }
    }
}
