// @ObfuscatedName("fv")
// jag::oldscape::graphics::Pix2D
//
// Static pixel buffer + clipping window the rest of the graphics stack
// draws into. Java extends Linkable2 (so Pix2D instances can sit in
// linked lists for pooling), but the static drawing surface is the only
// part the title screen needs, so the Linkable2 embedding is preserved
// for diff-against-future-revision purposes but never linked at runtime.

#![allow(dead_code, non_upper_case_globals)]

use std::sync::Mutex;

// custom container — every `public static` field on @ObfuscatedName("fv").
pub struct Pix2DState {
    // @ObfuscatedName("fv.n")
    pub pixels: Vec<i32>,

    // @ObfuscatedName("fv.j")
    pub width: i32,

    // @ObfuscatedName("fv.z")
    pub height: i32,

    // @ObfuscatedName("fv.g")
    pub clip_min_y: i32,

    // @ObfuscatedName("fv.q")
    pub clip_max_y: i32,

    // @ObfuscatedName("fv.i")
    pub clip_min_x: i32,

    // @ObfuscatedName("fv.s")
    pub clip_max_x: i32,
}

impl Pix2DState {
    pub const fn new() -> Self {
        Self {
            pixels: Vec::new(),
            width: 0,
            height: 0,
            clip_min_y: 0,
            clip_max_y: 0,
            clip_min_x: 0,
            clip_max_x: 0,
        }
    }
}

pub static STATE: Mutex<Pix2DState> = Mutex::new(Pix2DState::new());

// @ObfuscatedName("fv.z([III)V") — Pix2D.setPixels
pub fn set_pixels(arg0: Vec<i32>, arg1: i32, arg2: i32) {
    let mut s = STATE.lock().unwrap();
    s.pixels = arg0;
    s.width = arg1;
    s.height = arg2;
    drop(s);
    set_clipping(0, 0, arg1, arg2);
}

// Swap the bound pixel buffer (Java PixMap.bind equivalent): returns
// the previously bound (pixels, width, height) and clips to the new
// image. Lets the 3D viewport render into its own image like Java's
// areaViewport PixMap, so rasterizer overdraw can never touch UI.
pub fn swap_pixels(pixels: Vec<i32>, w: i32, h: i32) -> (Vec<i32>, i32, i32) {
    let mut s = STATE.lock().unwrap();
    let old = (std::mem::replace(&mut s.pixels, pixels), s.width, s.height);
    s.width = w;
    s.height = h;
    drop(s);
    set_clipping(0, 0, w, h);
    old
}

// @ObfuscatedName("fv.g()V") — Pix2D.resetClipping
pub fn reset_clipping() {
    let mut s = STATE.lock().unwrap();
    s.clip_min_x = 0;
    s.clip_min_y = 0;
    s.clip_max_x = s.width;
    s.clip_max_y = s.height;
}

// @ObfuscatedName("fv.q(IIII)V") — Pix2D.setClipping
pub fn set_clipping(mut x: i32, mut y: i32, mut w: i32, mut h: i32) {
    let mut s = STATE.lock().unwrap();
    if x < 0 { x = 0; }
    if y < 0 { y = 0; }
    if w > s.width { w = s.width; }
    if h > s.height { h = s.height; }
    s.clip_min_x = x;
    s.clip_min_y = y;
    s.clip_max_x = w;
    s.clip_max_y = h;
}

// @ObfuscatedName("fv.i(IIII)V") — Pix2D.setSubClipping
pub fn set_sub_clipping(arg0: i32, arg1: i32, arg2: i32, arg3: i32) {
    let mut s = STATE.lock().unwrap();
    if s.clip_min_x < arg0 { s.clip_min_x = arg0; }
    if s.clip_min_y < arg1 { s.clip_min_y = arg1; }
    if s.clip_max_x > arg2 { s.clip_max_x = arg2; }
    if s.clip_max_y > arg3 { s.clip_max_y = arg3; }
}

// @ObfuscatedName("fv.s([I)V") — Pix2D.saveClipping
pub fn save_clipping(dst: &mut [i32; 4]) {
    let s = STATE.lock().unwrap();
    dst[0] = s.clip_min_x;
    dst[1] = s.clip_min_y;
    dst[2] = s.clip_max_x;
    dst[3] = s.clip_max_y;
}

// @ObfuscatedName("fv.u([I)V") — Pix2D.restoreClipping
pub fn restore_clipping(src: &[i32; 4]) {
    let mut s = STATE.lock().unwrap();
    s.clip_min_x = src[0];
    s.clip_min_y = src[1];
    s.clip_max_x = src[2];
    s.clip_max_y = src[3];
}

// @ObfuscatedName("fv.v()V") — Pix2D.cls
pub fn cls() {
    let mut s = STATE.lock().unwrap();
    let total = s.width * s.height;
    let bound = total - 7;
    let mut i = 0i32;
    while i < bound {
        s.pixels[i as usize] = 0; i += 1;
        s.pixels[i as usize] = 0; i += 1;
        s.pixels[i as usize] = 0; i += 1;
        s.pixels[i as usize] = 0; i += 1;
        s.pixels[i as usize] = 0; i += 1;
        s.pixels[i as usize] = 0; i += 1;
        s.pixels[i as usize] = 0; i += 1;
        s.pixels[i as usize] = 0; i += 1;
    }
    while i < total {
        s.pixels[i as usize] = 0;
        i += 1;
    }
}

// @ObfuscatedName("fv.w(IIIIII)V") — Pix2D.fillRectTrans
pub fn fill_rect_trans(mut x: i32, mut y: i32, mut w: i32, mut h: i32, rgb: i32, alpha: i32) {
    let mut s = STATE.lock().unwrap();
    if x < s.clip_min_x { w -= s.clip_min_x - x; x = s.clip_min_x; }
    if y < s.clip_min_y { h -= s.clip_min_y - y; y = s.clip_min_y; }
    if x + w > s.clip_max_x { w = s.clip_max_x - x; }
    if y + h > s.clip_max_y { h = s.clip_max_y - y; }
    let var6 = ((((rgb & 0xFF00FF) as u32 * alpha as u32) >> 8) & 0xFF00FF) as i32
        + ((((rgb & 0xFF00) as u32 * alpha as u32) >> 8) & 0xFF00) as i32;
    let var7 = 256 - alpha;
    let var8 = s.width - w;
    let mut var9 = s.width * y + x;
    for _ in 0..h {
        for _ in 0..w {
            let var12 = s.pixels[var9 as usize] as u32;
            let var13 = (((var12 & 0xFF00FF) * var7 as u32) >> 8) & 0xFF00FF;
            let var13b = (((var12 & 0xFF00) * var7 as u32) >> 8) & 0xFF00;
            s.pixels[var9 as usize] = var6 + (var13 + var13b) as i32;
            var9 += 1;
        }
        var9 += var8;
    }
}

// @ObfuscatedName("fv.e(IIIII)V") — Pix2D.fillRect
pub fn fill_rect(mut x: i32, mut y: i32, mut w: i32, mut h: i32, rgb: i32) {
    let mut s = STATE.lock().unwrap();
    if x < s.clip_min_x { w -= s.clip_min_x - x; x = s.clip_min_x; }
    if y < s.clip_min_y { h -= s.clip_min_y - y; y = s.clip_min_y; }
    if x + w > s.clip_max_x { w = s.clip_max_x - x; }
    if y + h > s.clip_max_y { h = s.clip_max_y - y; }
    if w <= 0 || h <= 0 {
        return;
    }
    let var5 = s.width - w;
    let mut var6 = s.width * y + x;
    for _ in 0..h {
        for _ in 0..w {
            s.pixels[var6 as usize] = rgb;
            var6 += 1;
        }
        var6 += var5;
    }
}

// @ObfuscatedName("fv.b(IIIIII)V") — Pix2D.fillRectVGrad
pub fn fill_rect_v_grad(mut arg0: i32, mut arg1: i32, mut arg2: i32, mut arg3: i32, arg4: i32, arg5: i32) {
    let mut s = STATE.lock().unwrap();
    let mut var6: i32 = 0;
    let var7 = 65536 / arg3;
    if arg0 < s.clip_min_x { arg2 -= s.clip_min_x - arg0; arg0 = s.clip_min_x; }
    if arg1 < s.clip_min_y {
        var6 += (s.clip_min_y - arg1) * var7;
        arg3 -= s.clip_min_y - arg1;
        arg1 = s.clip_min_y;
    }
    if arg0 + arg2 > s.clip_max_x { arg2 = s.clip_max_x - arg0; }
    if arg1 + arg3 > s.clip_max_y { arg3 = s.clip_max_y - arg1; }
    let var8 = s.width - arg2;
    let mut var9 = s.width * arg1 + arg0;
    for _ in 0..arg3 {
        let var11 = ((65536 - var6) >> 8) as u32;
        let var12 = (var6 >> 8) as u32;
        let var13 = ((((arg4 & 0xFF00FF) as u32 * var11 + (arg5 & 0xFF00FF) as u32 * var12) & 0xFF00FF00) as i32
            + ((((arg4 & 0xFF00) as u32 * var11 + (arg5 & 0xFF00) as u32 * var12) & 0xFF0000) as i32)) as u32
            >> 8;
        for _ in 0..arg2 {
            s.pixels[var9 as usize] = var13 as i32;
            var9 += 1;
        }
        var9 += var8;
        var6 += var7;
    }
}

// @ObfuscatedName("fv.y(IIIII)V") — Pix2D.drawRect
pub fn draw_rect(x: i32, y: i32, w: i32, h: i32, rgb: i32) {
    hline(x, y, w, rgb);
    hline(x, y + h - 1, w, rgb);
    vline(x, y, h, rgb);
    vline(x + w - 1, y, h, rgb);
}

// @ObfuscatedName("fv.t(IIIIII)V") — Pix2D.drawRectTrans
pub fn draw_rect_trans(x: i32, y: i32, w: i32, h: i32, rgb: i32, alpha: i32) {
    hline_trans(x, y, w, rgb, alpha);
    hline_trans(x, y + h - 1, w, rgb, alpha);
    if h >= 3 {
        vline_trans(x, y + 1, h - 2, rgb, alpha);
        vline_trans(x + w - 1, y + 1, h - 2, rgb, alpha);
    }
}

// @ObfuscatedName("fv.f(IIII)V") — Pix2D.hline
pub fn hline(mut x: i32, y: i32, mut w: i32, rgb: i32) {
    let mut s = STATE.lock().unwrap();
    if y < s.clip_min_y || y >= s.clip_max_y { return; }
    if x < s.clip_min_x { w -= s.clip_min_x - x; x = s.clip_min_x; }
    if x + w > s.clip_max_x { w = s.clip_max_x - x; }
    if w <= 0 {
        return;
    }
    let var4 = s.width * y + x;
    for var5 in 0..w {
        s.pixels[(var4 + var5) as usize] = rgb;
    }
}

// @ObfuscatedName("fv.k(IIIII)V") — Pix2D.hlineTrans
pub fn hline_trans(mut x: i32, y: i32, mut w: i32, rgb: i32, alpha: i32) {
    let mut s = STATE.lock().unwrap();
    if y < s.clip_min_y || y >= s.clip_max_y { return; }
    if x < s.clip_min_x { w -= s.clip_min_x - x; x = s.clip_min_x; }
    if x + w > s.clip_max_x { w = s.clip_max_x - x; }
    let var5 = 256 - alpha;
    let var6 = (rgb >> 16 & 0xFF) * alpha;
    let var7 = (rgb >> 8 & 0xFF) * alpha;
    let var8 = (rgb & 0xFF) * alpha;
    let mut var9 = s.width * y + x;
    for _ in 0..w {
        let var11 = (s.pixels[var9 as usize] >> 16 & 0xFF) * var5;
        let var12 = (s.pixels[var9 as usize] >> 8 & 0xFF) * var5;
        let var13 = (s.pixels[var9 as usize] & 0xFF) * var5;
        let var14 = ((var8 + var13) >> 8) + (((var6 + var11) >> 8) << 16) + (((var7 + var12) >> 8) << 8);
        s.pixels[var9 as usize] = var14;
        var9 += 1;
    }
}

// @ObfuscatedName("fv.o(IIII)V") — Pix2D.vline
pub fn vline(x: i32, mut y: i32, mut h: i32, rgb: i32) {
    let mut s = STATE.lock().unwrap();
    if x < s.clip_min_x || x >= s.clip_max_x { return; }
    if y < s.clip_min_y { h -= s.clip_min_y - y; y = s.clip_min_y; }
    if y + h > s.clip_max_y { h = s.clip_max_y - y; }
    if h <= 0 {
        return;
    }
    let width = s.width;
    let var4 = width * y + x;
    for var5 in 0..h {
        s.pixels[(width * var5 + var4) as usize] = rgb;
    }
}

// @ObfuscatedName("fv.a(IIIII)V") — Pix2D.vlineTrans
pub fn vline_trans(x: i32, mut y: i32, mut h: i32, rgb: i32, alpha: i32) {
    let mut s = STATE.lock().unwrap();
    if x < s.clip_min_x || x >= s.clip_max_x { return; }
    if y < s.clip_min_y { h -= s.clip_min_y - y; y = s.clip_min_y; }
    if y + h > s.clip_max_y { h = s.clip_max_y - y; }
    let var5 = 256 - alpha;
    let var6 = (rgb >> 16 & 0xFF) * alpha;
    let var7 = (rgb >> 8 & 0xFF) * alpha;
    let var8 = (rgb & 0xFF) * alpha;
    let width = s.width;
    let mut var9 = width * y + x;
    for _ in 0..h {
        let var11 = (s.pixels[var9 as usize] >> 16 & 0xFF) * var5;
        let var12 = (s.pixels[var9 as usize] >> 8 & 0xFF) * var5;
        let var13 = (s.pixels[var9 as usize] & 0xFF) * var5;
        let var14 = ((var8 + var13) >> 8) + (((var6 + var11) >> 8) << 16) + (((var7 + var12) >> 8) << 8);
        s.pixels[var9 as usize] = var14;
        var9 += width;
    }
}

// @ObfuscatedName("fv.h(IIIII)V") — Pix2D.line
pub fn line(mut x1: i32, mut y1: i32, x2: i32, y2: i32, rgb: i32) {
    let mut dx = x2 - x1;
    let mut dy = y2 - y1;
    if dy == 0 {
        if dx >= 0 { hline(x1, y1, dx + 1, rgb); }
        else       { hline(x1 + dx, y1, -dx + 1, rgb); }
    } else if dx == 0 {
        if dy >= 0 { vline(x1, y1, dy + 1, rgb); }
        else       { vline(x1, y1 + dy, -dy + 1, rgb); }
    } else {
        if dx + dy < 0 { x1 += dx; dx = -dx; y1 += dy; dy = -dy; }
        let s = STATE.lock().unwrap();
        let width = s.width;
        let clip_min_x = s.clip_min_x;
        let clip_max_x = s.clip_max_x;
        let clip_min_y = s.clip_min_y;
        let clip_max_y = s.clip_max_y;
        drop(s);
        if dx > dy {
            let y_fine = y1 << 16;
            let mut y_offset = y_fine + 32768;
            let dy_fine = dy << 16;
            let y_step = ((dy_fine as f64 / dx as f64) + 0.5).floor() as i32;
            let mut end_x = x1 + dx;
            if x1 < clip_min_x { y_offset += (clip_min_x - x1) * y_step; x1 = clip_min_x; }
            if end_x >= clip_max_x { end_x = clip_max_x - 1; }
            let mut s = STATE.lock().unwrap();
            while x1 <= end_x {
                let draw_y = y_offset >> 16;
                if draw_y >= clip_min_y && draw_y < clip_max_y {
                    s.pixels[(width * draw_y + x1) as usize] = rgb;
                }
                y_offset += y_step;
                x1 += 1;
            }
        } else {
            let x_fine = x1 << 16;
            let mut x_offset = x_fine + 32768;
            let dx_fine = dx << 16;
            let x_step = ((dx_fine as f64 / dy as f64) + 0.5).floor() as i32;
            let mut end_y = y1 + dy;
            if y1 < clip_min_y { x_offset += (clip_min_y - y1) * x_step; y1 = clip_min_y; }
            if end_y >= clip_max_y { end_y = clip_max_y - 1; }
            let mut s = STATE.lock().unwrap();
            while y1 <= end_y {
                let draw_x = x_offset >> 16;
                if draw_x >= clip_min_x && draw_x < clip_max_x {
                    s.pixels[(width * y1 + draw_x) as usize] = rgb;
                }
                x_offset += x_step;
                y1 += 1;
            }
        }
    }
}

// @ObfuscatedName("fv.x(III[I[I)V") — Pix2D.fillScanLine
pub fn fill_scan_line(arg0: i32, arg1: i32, arg2: i32, arg3: &[i32], arg4: &[i32]) {
    let mut s = STATE.lock().unwrap();
    let width = s.width;
    let mut var5 = width * arg1 + arg0;
    for var6 in 0..arg3.len() {
        let mut var7 = arg3[var6] + var5;
        for _ in (-arg4[var6])..0 {
            s.pixels[var7 as usize] = arg2;
            var7 += 1;
        }
        var5 += width;
    }
}
