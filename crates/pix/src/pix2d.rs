//! Direct port of `jagex3.graphics.Pix2D` (rev1 client). The pixel buffer + clip rect +
//! primitive draws (line, rect, fillRect, gradient). Higher-level renderers (Pix3D,
//! Pix8/Pix32, PixFontGeneric) write into a `Pix2D` instance.
//!
//! Java keeps all this state in static fields; in Rust we wrap it in a struct. Method
//! signatures match Java's parameter order and names.

/// A bounded mutable framebuffer with a clip rect. Pixel format is ARGB packed `u32`
/// (`0xAARRGGBB`).
pub struct Pix2D {
    pub pixels: Vec<u32>,
    pub width: i32,
    pub height: i32,
    pub clip_min_x: i32,
    pub clip_min_y: i32,
    pub clip_max_x: i32,
    pub clip_max_y: i32,
}

impl Pix2D {
    /// Create a fresh `width × height` buffer (cleared to 0). Matches Java's
    /// `Pix2D.setPixels(new int[w*h], w, h)`.
    #[must_use]
    pub fn new(width: i32, height: i32) -> Self {
        let w = width.max(0) as usize;
        let h = height.max(0) as usize;
        Self {
            pixels: vec![0; w * h],
            width,
            height,
            clip_min_x: 0,
            clip_min_y: 0,
            clip_max_x: width,
            clip_max_y: height,
        }
    }

    /// Wrap an existing buffer. Matches `Pix2D.setPixels(arg0, arg1, arg2)`.
    #[must_use]
    pub fn from_buffer(pixels: Vec<u32>, width: i32, height: i32) -> Self {
        let mut me = Self {
            pixels,
            width,
            height,
            clip_min_x: 0,
            clip_min_y: 0,
            clip_max_x: 0,
            clip_max_y: 0,
        };
        me.set_clipping(0, 0, width, height);
        me
    }

    /// Reallocate the pixel buffer at a new size and reset clipping.
    pub fn resize(&mut self, width: i32, height: i32) {
        self.width = width;
        self.height = height;
        let w = width.max(0) as usize;
        let h = height.max(0) as usize;
        self.pixels.clear();
        self.pixels.resize(w * h, 0);
        self.reset_clipping();
    }

    /// `Pix2D.resetClipping`.
    pub fn reset_clipping(&mut self) {
        self.clip_min_x = 0;
        self.clip_min_y = 0;
        self.clip_max_x = self.width;
        self.clip_max_y = self.height;
    }

    /// `Pix2D.setClipping(x, y, w, h)`. Note: Java's `w` and `h` parameters are actually
    /// the MAX X/Y (clip ends), not widths — preserved here.
    pub fn set_clipping(&mut self, x: i32, y: i32, w: i32, h: i32) {
        self.clip_min_x = x.max(0);
        self.clip_min_y = y.max(0);
        self.clip_max_x = w.min(self.width);
        self.clip_max_y = h.min(self.height);
    }

    /// `Pix2D.setSubClipping` — intersect the current clip with a smaller rect (clip
    /// only shrinks, never grows).
    pub fn set_sub_clipping(&mut self, x0: i32, y0: i32, x1: i32, y1: i32) {
        if self.clip_min_x < x0 {
            self.clip_min_x = x0;
        }
        if self.clip_min_y < y0 {
            self.clip_min_y = y0;
        }
        if self.clip_max_x > x1 {
            self.clip_max_x = x1;
        }
        if self.clip_max_y > y1 {
            self.clip_max_y = y1;
        }
    }

    /// `Pix2D.saveClipping` / `restoreClipping`. Returns/accepts `[min_x, min_y, max_x, max_y]`.
    #[must_use]
    pub fn save_clipping(&self) -> [i32; 4] {
        [self.clip_min_x, self.clip_min_y, self.clip_max_x, self.clip_max_y]
    }

    pub fn restore_clipping(&mut self, c: [i32; 4]) {
        self.clip_min_x = c[0];
        self.clip_min_y = c[1];
        self.clip_max_x = c[2];
        self.clip_max_y = c[3];
    }

    /// `Pix2D.cls` — clear to black (0).
    pub fn cls(&mut self) {
        for p in &mut self.pixels {
            *p = 0;
        }
    }

    /// `Pix2D.fillRect(x, y, w, h, rgb)` — solid fill, clipped.
    pub fn fill_rect(&mut self, x: i32, y: i32, mut w: i32, mut h: i32, rgb: u32) {
        let (x, y) = self.clip_rect(x, y, &mut w, &mut h);
        if w <= 0 || h <= 0 {
            return;
        }
        let stride = self.width - w;
        let mut idx = (self.width * y + x) as usize;
        for _ in 0..h {
            for _ in 0..w {
                self.pixels[idx] = rgb;
                idx += 1;
            }
            idx += stride as usize;
        }
    }

    /// `Pix2D.fillRectTrans(x, y, w, h, rgb, alpha)` — per-pixel alpha blend (0=transparent, 256=opaque).
    pub fn fill_rect_trans(
        &mut self,
        x: i32,
        y: i32,
        mut w: i32,
        mut h: i32,
        rgb: u32,
        alpha: u32,
    ) {
        let (x, y) = self.clip_rect(x, y, &mut w, &mut h);
        if w <= 0 || h <= 0 {
            return;
        }
        let pre =
            (((rgb & 0xFF00FF) * alpha >> 8) & 0xFF00FF) + (((rgb & 0xFF00) * alpha >> 8) & 0xFF00);
        let inv = 256 - alpha;
        let stride = self.width - w;
        let mut idx = (self.width * y + x) as usize;
        for _ in 0..h {
            for _ in 0..w {
                let dst = self.pixels[idx];
                let blend = (((dst & 0xFF00FF) * inv >> 8) & 0xFF00FF)
                    + (((dst & 0xFF00) * inv >> 8) & 0xFF00);
                self.pixels[idx] = pre + blend;
                idx += 1;
            }
            idx += stride as usize;
        }
    }

    /// `Pix2D.drawRect` — outline only (1px lines).
    pub fn draw_rect(&mut self, x: i32, y: i32, w: i32, h: i32, rgb: u32) {
        self.hline(x, y, w, rgb);
        self.hline(x, y + h - 1, w, rgb);
        self.vline(x, y, h, rgb);
        self.vline(x + w - 1, y, h, rgb);
    }

    /// `Pix2D.drawRectTrans` — outline with per-pixel alpha.
    pub fn draw_rect_trans(&mut self, x: i32, y: i32, w: i32, h: i32, rgb: u32, alpha: u32) {
        self.hline_trans(x, y, w, rgb, alpha);
        self.hline_trans(x, y + h - 1, w, rgb, alpha);
        if h >= 3 {
            self.vline_trans(x, y + 1, h - 2, rgb, alpha);
            self.vline_trans(x + w - 1, y + 1, h - 2, rgb, alpha);
        }
    }

    /// `Pix2D.hline` — horizontal line at row `y`, length `w`.
    pub fn hline(&mut self, mut x: i32, y: i32, mut w: i32, rgb: u32) {
        if y < self.clip_min_y || y >= self.clip_max_y {
            return;
        }
        if x < self.clip_min_x {
            w -= self.clip_min_x - x;
            x = self.clip_min_x;
        }
        if x + w > self.clip_max_x {
            w = self.clip_max_x - x;
        }
        if w <= 0 {
            return;
        }
        let base = (self.width * y + x) as usize;
        for i in 0..w as usize {
            self.pixels[base + i] = rgb;
        }
    }

    /// `Pix2D.hlineTrans`.
    pub fn hline_trans(&mut self, mut x: i32, y: i32, mut w: i32, rgb: u32, alpha: u32) {
        if y < self.clip_min_y || y >= self.clip_max_y {
            return;
        }
        if x < self.clip_min_x {
            w -= self.clip_min_x - x;
            x = self.clip_min_x;
        }
        if x + w > self.clip_max_x {
            w = self.clip_max_x - x;
        }
        if w <= 0 {
            return;
        }
        let inv = 256 - alpha;
        let pr = ((rgb >> 16) & 0xFF) * alpha;
        let pg = ((rgb >> 8) & 0xFF) * alpha;
        let pb = (rgb & 0xFF) * alpha;
        let mut idx = (self.width * y + x) as usize;
        for _ in 0..w {
            let dst = self.pixels[idx];
            let dr = ((dst >> 16) & 0xFF) * inv;
            let dg = ((dst >> 8) & 0xFF) * inv;
            let db = (dst & 0xFF) * inv;
            self.pixels[idx] =
                ((pb + db) >> 8) + (((pr + dr) >> 8) << 16) + (((pg + dg) >> 8) << 8);
            idx += 1;
        }
    }

    /// `Pix2D.vline`.
    pub fn vline(&mut self, x: i32, mut y: i32, mut h: i32, rgb: u32) {
        if x < self.clip_min_x || x >= self.clip_max_x {
            return;
        }
        if y < self.clip_min_y {
            h -= self.clip_min_y - y;
            y = self.clip_min_y;
        }
        if y + h > self.clip_max_y {
            h = self.clip_max_y - y;
        }
        if h <= 0 {
            return;
        }
        let base = (self.width * y + x) as usize;
        for i in 0..h as usize {
            self.pixels[base + i * self.width as usize] = rgb;
        }
    }

    /// `Pix2D.vlineTrans`.
    pub fn vline_trans(&mut self, x: i32, mut y: i32, mut h: i32, rgb: u32, alpha: u32) {
        if x < self.clip_min_x || x >= self.clip_max_x {
            return;
        }
        if y < self.clip_min_y {
            h -= self.clip_min_y - y;
            y = self.clip_min_y;
        }
        if y + h > self.clip_max_y {
            h = self.clip_max_y - y;
        }
        if h <= 0 {
            return;
        }
        let inv = 256 - alpha;
        let pr = ((rgb >> 16) & 0xFF) * alpha;
        let pg = ((rgb >> 8) & 0xFF) * alpha;
        let pb = (rgb & 0xFF) * alpha;
        let mut idx = (self.width * y + x) as usize;
        for _ in 0..h {
            let dst = self.pixels[idx];
            let dr = ((dst >> 16) & 0xFF) * inv;
            let dg = ((dst >> 8) & 0xFF) * inv;
            let db = (dst & 0xFF) * inv;
            self.pixels[idx] =
                ((pb + db) >> 8) + (((pr + dr) >> 8) << 16) + (((pg + dg) >> 8) << 8);
            idx += self.width as usize;
        }
    }

    /// `Pix2D.fillRectVGrad(x, y, w, h, top_rgb, bottom_rgb)` — vertical gradient.
    pub fn fill_rect_vgrad(
        &mut self,
        mut x: i32,
        mut y: i32,
        mut w: i32,
        mut h: i32,
        top_rgb: u32,
        bottom_rgb: u32,
    ) {
        let mut grad = 0i32;
        let step = if h > 0 { 65536 / h } else { 0 };
        if x < self.clip_min_x {
            w -= self.clip_min_x - x;
            x = self.clip_min_x;
        }
        if y < self.clip_min_y {
            grad += (self.clip_min_y - y) * step;
            h -= self.clip_min_y - y;
            y = self.clip_min_y;
        }
        if x + w > self.clip_max_x {
            w = self.clip_max_x - x;
        }
        if y + h > self.clip_max_y {
            h = self.clip_max_y - y;
        }
        if w <= 0 || h <= 0 {
            return;
        }
        let stride = self.width - w;
        let mut idx = (self.width * y + x) as usize;
        for _ in 0..h {
            let top = ((65536 - grad) >> 8) as u32;
            let bot = (grad >> 8) as u32;
            let blended = ((top_rgb & 0xFF00FF) * top + (bottom_rgb & 0xFF00FF) * bot
                & 0xFF00_FF00)
                + ((top_rgb & 0xFF00) * top + (bottom_rgb & 0xFF00) * bot & 0xFF0000)
                >> 8;
            for _ in 0..w {
                self.pixels[idx] = blended;
                idx += 1;
            }
            idx += stride as usize;
            grad += step;
        }
    }

    /// `Pix2D.line(x1, y1, x2, y2, rgb)` — 1px line via Bresenham-ish DDA.
    #[allow(clippy::cognitive_complexity)]
    pub fn line(&mut self, mut x1: i32, mut y1: i32, x2: i32, y2: i32, rgb: u32) {
        let mut dx = x2 - x1;
        let mut dy = y2 - y1;
        if dy == 0 {
            if dx >= 0 {
                self.hline(x1, y1, dx + 1, rgb);
            } else {
                self.hline(x1 + dx, y1, -dx + 1, rgb);
            }
            return;
        }
        if dx == 0 {
            if dy >= 0 {
                self.vline(x1, y1, dy + 1, rgb);
            } else {
                self.vline(x1, y1 + dy, -dy + 1, rgb);
            }
            return;
        }
        if dx + dy < 0 {
            x1 += dx;
            dx = -dx;
            y1 += dy;
            dy = -dy;
        }
        if dx > dy {
            let y_fine = y1 << 16;
            let mut y_off = y_fine + 32768;
            let dy_fine = dy << 16;
            let y_step = ((dy_fine as f64 / dx as f64) + 0.5).floor() as i32;
            let mut end_x = x1 + dx;
            if x1 < self.clip_min_x {
                y_off += (self.clip_min_x - x1) * y_step;
                x1 = self.clip_min_x;
            }
            if end_x >= self.clip_max_x {
                end_x = self.clip_max_x - 1;
            }
            while x1 <= end_x {
                let draw_y = y_off >> 16;
                if draw_y >= self.clip_min_y && draw_y < self.clip_max_y {
                    self.pixels[(self.width * draw_y + x1) as usize] = rgb;
                }
                y_off += y_step;
                x1 += 1;
            }
        } else {
            let x_fine = x1 << 16;
            let mut x_off = x_fine + 32768;
            let dx_fine = dx << 16;
            let x_step = ((dx_fine as f64 / dy as f64) + 0.5).floor() as i32;
            let mut end_y = y1 + dy;
            if y1 < self.clip_min_y {
                x_off += (self.clip_min_y - y1) * x_step;
                y1 = self.clip_min_y;
            }
            if end_y >= self.clip_max_y {
                end_y = self.clip_max_y - 1;
            }
            while y1 <= end_y {
                let draw_x = x_off >> 16;
                if draw_x >= self.clip_min_x && draw_x < self.clip_max_x {
                    self.pixels[(self.width * y1 + draw_x) as usize] = rgb;
                }
                x_off += x_step;
                y1 += 1;
            }
        }
    }

    /// Internal helper: clamp a rect to the clip area, modifying `w` and `h` in place.
    /// Returns the clamped origin `(x, y)`.
    fn clip_rect(&self, mut x: i32, mut y: i32, w: &mut i32, h: &mut i32) -> (i32, i32) {
        if x < self.clip_min_x {
            *w -= self.clip_min_x - x;
            x = self.clip_min_x;
        }
        if y < self.clip_min_y {
            *h -= self.clip_min_y - y;
            y = self.clip_min_y;
        }
        if x + *w > self.clip_max_x {
            *w = self.clip_max_x - x;
        }
        if y + *h > self.clip_max_y {
            *h = self.clip_max_y - y;
        }
        (x, y)
    }
}
