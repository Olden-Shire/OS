// @ObfuscatedName("al")
// jag::oldscape::graphics::pixloader
//
// Sprite-archive unpacker. The Java statics that hold the per-call state
// while depack() runs are preserved here with their @ObfuscatedName tags.
// Each makePix*/makePixFont consumes the statics and resets them, matching
// Java's "depack populates, factory drains" idiom.

#![allow(dead_code)]

use std::sync::Mutex;

use crate::io::packet::Packet;
use crate::js5::js5_loader::Js5Loader;

use super::pix8::Pix8;
use super::pix32::Pix32;
use super::pix_font::PixFont;
use super::pix_font_generic::PixFontGeneric;

pub struct PixLoaderState {
    // @ObfuscatedName("al.r")
    pub count: i32,

    // @ObfuscatedName("al.d")
    pub owi: i32,

    // @ObfuscatedName("al.l")
    pub ohi: i32,

    // @ObfuscatedName("al.m")
    pub xof: Vec<i32>,

    // @ObfuscatedName("al.c")
    pub yof: Vec<i32>,

    // @ObfuscatedName("m.n")
    pub wi: Vec<i32>,

    // @ObfuscatedName("cl.j")
    pub hi: Vec<i32>,

    // @ObfuscatedName("al.z")
    pub bpal: Vec<i32>,

    // @ObfuscatedName("bp.g")
    pub bspr: Vec<Vec<u8>>,
}

impl PixLoaderState {
    pub const fn new() -> Self {
        Self {
            count: 0,
            owi: 0,
            ohi: 0,
            xof: Vec::new(),
            yof: Vec::new(),
            wi: Vec::new(),
            hi: Vec::new(),
            bpal: Vec::new(),
            bspr: Vec::new(),
        }
    }

    fn reset(&mut self) {
        self.xof.clear();
        self.yof.clear();
        self.wi.clear();
        self.hi.clear();
        self.bpal.clear();
        self.bspr.clear();
    }
}

pub static STATE: Mutex<PixLoaderState> = Mutex::new(PixLoaderState::new());

// @ObfuscatedName("a.s([BB)V") — PixLoader.depack(byte[])
//
// Sprite-archive layout (read backwards):
//   [last 2]:   count
//   [last 7..]: owi(2) ohi(2) pal_count(1) then count*8 bytes of x/y/w/h tables
//   [pal block]: (pal_count - 1) * 3 bytes BE colours
//   [front]:    per-sprite raw pixel data
pub fn depack(arg0: &[u8]) {
    let mut s = STATE.lock().unwrap();
    let mut var1 = Packet::from_vec(arg0.to_vec());

    var1.pos = arg0.len() as i32 - 2;
    s.count = var1.g2();
    let count = s.count as usize;
    s.xof = vec![0; count];
    s.yof = vec![0; count];
    s.wi = vec![0; count];
    s.hi = vec![0; count];
    s.bspr = (0..count).map(|_| Vec::new()).collect();

    var1.pos = arg0.len() as i32 - 7 - count as i32 * 8;
    s.owi = var1.g2();
    s.ohi = var1.g2();
    let var2 = (var1.g1() & 0xFF) + 1;

    for var3 in 0..count {
        s.xof[var3] = var1.g2();
    }
    for var4 in 0..count {
        s.yof[var4] = var1.g2();
    }
    for var5 in 0..count {
        s.wi[var5] = var1.g2();
    }
    for var6 in 0..count {
        s.hi[var6] = var1.g2();
    }

    var1.pos = arg0.len() as i32 - 7 - count as i32 * 8 - (var2 - 1) * 3;
    s.bpal = vec![0i32; var2 as usize];
    for var7 in 1..var2 as usize {
        s.bpal[var7] = var1.g3();
        if s.bpal[var7] == 0 {
            s.bpal[var7] = 1;
        }
    }

    var1.pos = 0;
    for var8 in 0..count {
        let var9 = s.wi[var8];
        let var10 = s.hi[var8];
        let var11 = var9 * var10;
        let mut var12 = vec![0u8; var11 as usize];
        let var13 = var1.g1();
        if var13 == 0 {
            for var14 in 0..var11 as usize {
                var12[var14] = var1.g1b() as u8;
            }
        } else if var13 == 1 {
            for var15 in 0..var9 {
                for var16 in 0..var10 {
                    var12[(var9 * var16 + var15) as usize] = var1.g1b() as u8;
                }
            }
        }
        s.bspr[var8] = var12;
    }
}

// @ObfuscatedName("bn.q(Lch;III)Z") — PixLoader.depack(Js5, group, file)
pub fn depack_from(loader: &mut Js5Loader, group_id: i32, file_id: i32) -> bool {
    // Use Js5Loader::fetch_file so missed groups trigger a download.
    let Some(data) = loader.fetch_file(group_id, file_id) else {
        return false;
    };
    depack(&data);
    true
}

// @ObfuscatedName("ao.j(I)Lft;") — PixLoader.makePix8 (single-sprite factory)
pub fn make_pix8_single() -> Pix8 {
    let mut s = STATE.lock().unwrap();
    let mut var0 = Pix8::new();
    var0.owi = s.owi;
    var0.ohi = s.ohi;
    var0.xof = s.xof[0];
    var0.yof = s.yof[0];
    var0.wi = s.wi[0];
    var0.hi = s.hi[0];
    var0.bpal = s.bpal.clone();
    var0.data = s.bspr[0].clone();
    s.reset();
    var0
}

// @ObfuscatedName("ak.d(Lch;Ljava/lang/String;Ljava/lang/String;I)Lft;") — by name
pub fn make_pix8(loader: &mut Js5Loader, group_name: &str, file_name: &str) -> Option<Pix8> {
    let var3 = loader.base.get_group_id(group_name);
    if var3 < 0 {
        return None;
    }
    let var4 = loader.base.get_file_id(var3, file_name);
    if var4 < 0 {
        return None;
    }
    if depack_from(loader, var3, var4) {
        Some(make_pix8_single())
    } else {
        None
    }
}

// @ObfuscatedName("al.r(Lch;Ljava/lang/String;Ljava/lang/String;B)[Lft;") — array form
pub fn make_pix8_array(loader: &mut Js5Loader, group_name: &str, file_name: &str) -> Option<Vec<Pix8>> {
    let var3 = loader.base.get_group_id(group_name);
    if var3 < 0 {
        return None;
    }
    let var4 = loader.base.get_file_id(var3, file_name);
    if var4 < 0 {
        return None;
    }
    if !depack_from(loader, var3, var4) {
        return None;
    }
    let mut s = STATE.lock().unwrap();
    let mut var6 = Vec::with_capacity(s.count as usize);
    for var7 in 0..s.count as usize {
        let mut var8 = Pix8::new();
        var8.owi = s.owi;
        var8.ohi = s.ohi;
        var8.xof = s.xof[var7];
        var8.yof = s.yof[var7];
        var8.wi = s.wi[var7];
        var8.hi = s.hi[var7];
        var8.bpal = s.bpal.clone();
        var8.data = s.bspr[var7].clone();
        var6.push(var8);
    }
    s.reset();
    Some(var6)
}

// @ObfuscatedName("bi.z(I)Lfq;") — PixLoader.makePix32 (single-sprite factory)
pub fn make_pix32_single() -> Pix32 {
    let mut s = STATE.lock().unwrap();
    let mut var0 = Pix32::new_empty();
    var0.owi = s.owi;
    var0.ohi = s.ohi;
    var0.xof = s.xof[0];
    var0.yof = s.yof[0];
    var0.wi = s.wi[0];
    var0.hi = s.hi[0];
    let var1 = var0.wi * var0.hi;
    let var2 = s.bspr[0].clone();
    var0.data = (0..var1 as usize)
        .map(|i| s.bpal[(var2[i] as i32 & 0xFF) as usize])
        .collect();
    s.reset();
    var0
}

// @ObfuscatedName("r.m(Lch;Ljava/lang/String;Ljava/lang/String;I)Lfq;")
pub fn make_pix32(loader: &mut Js5Loader, group_name: &str, file_name: &str) -> Option<Pix32> {
    let var3 = loader.base.get_group_id(group_name);
    if var3 < 0 {
        return None;
    }
    let var4 = loader.base.get_file_id(var3, file_name);
    if var4 < 0 {
        return None;
    }
    if depack_from(loader, var3, var4) {
        Some(make_pix32_single())
    } else {
        None
    }
}

// @ObfuscatedName("bx.l(Lch;Ljava/lang/String;Ljava/lang/String;I)[Lfq;") — array form
pub fn make_pix32_array(loader: &mut Js5Loader, group_name: &str, file_name: &str) -> Option<Vec<Pix32>> {
    let var3 = loader.base.get_group_id(group_name);
    if var3 < 0 {
        return None;
    }
    let var4 = loader.base.get_file_id(var3, file_name);
    if var4 < 0 {
        return None;
    }
    if !depack_from(loader, var3, var4) {
        return None;
    }
    Some(drain_pix32_array())
}

// Decode a sprite-sheet group from its raw (decompressed) bytes into the
// per-sprite Pix32 array — the byte-level entry point behind
// `make_pix32_array`, for callers that already hold the group bytes
// (e.g. tooling browsing the sprites archive directly).
pub fn decode_pix32_array(bytes: &[u8]) -> Vec<Pix32> {
    depack(bytes);
    drain_pix32_array()
}

// Build the Pix32 array from the freshly-depacked STATE, then reset it.
fn drain_pix32_array() -> Vec<Pix32> {
    let mut s = STATE.lock().unwrap();
    let mut var6 = Vec::with_capacity(s.count as usize);
    for var7 in 0..s.count as usize {
        let mut var8 = Pix32::new_empty();
        var8.owi = s.owi;
        var8.ohi = s.ohi;
        var8.xof = s.xof[var7];
        var8.yof = s.yof[var7];
        var8.wi = s.wi[var7];
        var8.hi = s.hi[var7];
        let var9 = var8.wi * var8.hi;
        let var10 = s.bspr[var7].clone();
        var8.data = (0..var9 as usize)
            .map(|i| s.bpal[(var10[i] as i32 & 0xFF) as usize])
            .collect();
        var6.push(var8);
    }
    s.reset();
    var6
}

// @ObfuscatedName("y.g([BI)Lfm;") — PixLoader.makePixFont from raw metrics.
// Java PixLoader.java:228 — `new PixFontGeneric(arg0, xof, yof, wi, hi,
// bpal, bspr)`. The bpal palette IS passed through (we previously
// dropped it as Vec::new(), which left fonts without their colour
// palette).
pub fn make_pix_font_raw(arg0: Option<Vec<u8>>) -> Option<PixFontGeneric> {
    let arg0 = arg0?;
    let s = STATE.lock().unwrap();
    let base = PixFont::from_parts(
        &arg0,
        s.xof.clone(),
        s.yof.clone(),
        s.wi.clone(),
        s.hi.clone(),
        s.bpal.clone(),
        s.bspr.clone(),
    );
    drop(s);
    STATE.lock().unwrap().reset();
    Some(PixFontGeneric { base })
}

// @ObfuscatedName("bw.c(Lch;Lch;Ljava/lang/String;Ljava/lang/String;I)Lfm;")
pub fn make_pix_font(
    sprites: &mut Js5Loader,
    font_metrics: &mut Js5Loader,
    name: &str,
    suffix: &str,
) -> Option<PixFontGeneric> {
    let var4 = sprites.base.get_group_id(name);
    if var4 < 0 {
        return None;
    }
    let var5 = sprites.base.get_file_id(var4, suffix);
    if var5 < 0 {
        return None;
    }
    if !depack_from(sprites, var4, var5) {
        return None;
    }
    // Java fetches the metrics from the fontmetrics archive at the SAME
    // group/file ids — must use Js5Loader::fetch_file so the metrics
    // group is requested on first miss.
    let metrics = font_metrics.fetch_file(var4, var5);
    make_pix_font_raw(metrics)
}

// JPEG decoder for the title.jpg binary archive entry. Pix32(byte[]
// jpeg, Component) in Java uses the AWT JPEG decoder; we use the image
// crate with the same RGB→ARGB-i32 layout the rest of the code assumes.
pub fn pix32_from_jpeg(bytes: &[u8]) -> Option<Pix32> {
    let img = image::load_from_memory_with_format(bytes, image::ImageFormat::Jpeg).ok()?;
    let rgba = img.to_rgba8();
    let (w, h) = (rgba.width() as i32, rgba.height() as i32);
    let data: Vec<i32> = rgba
        .pixels()
        .map(|p| ((p[0] as i32) << 16) | ((p[1] as i32) << 8) | p[2] as i32)
        .collect();
    Some(Pix32::from_pixels(data, w, h))
}
