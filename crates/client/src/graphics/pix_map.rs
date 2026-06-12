// @ObfuscatedName("fr") — jag::oldscape::graphics::PixMap.
//
// Abstract bind target for Pix2D. In Java the hierarchy is:
//   PixMap (abstract) ← JavaPixMap (BufferedImage backing) ←
//                       JavaSafePixMap (defensive copy)
//
// GameShell allocates a JavaSafePixMap for the on-screen framebuffer
// then passes it to Pix2D.setPixels so subsequent draws land in the
// underlying int[]. We use a single concrete type since we don't need
// the JVM-specific BufferedImage juggling.

#![allow(dead_code)]

pub struct PixMap {
    // @ObfuscatedName("fr.j") — backing pixel buffer.
    pub data: Vec<i32>,
    // @ObfuscatedName("fr.z")
    pub width: i32,
    // @ObfuscatedName("fr.g")
    pub height: i32,
}

impl PixMap {
    pub fn new(width: i32, height: i32) -> Self {
        Self {
            data: vec![0i32; (width * height) as usize],
            width,
            height,
        }
    }

    // @ObfuscatedName("fr.l(II)V") — PixMap.bind. Pushes this map's
    // buffer + dimensions into Pix2D's STATE. Subsequent fill_rect /
    // draw_rect / etc render into `self.data`.
    pub fn bind(&mut self) {
        let mut s = crate::graphics::pix2d::STATE.lock().unwrap();
        s.pixels = std::mem::take(&mut self.data);
        s.width = self.width;
        s.height = self.height;
        s.clip_min_x = 0;
        s.clip_min_y = 0;
        s.clip_max_x = self.width;
        s.clip_max_y = self.height;
    }

    // @ObfuscatedName("fr.m()V") — PixMap.unbind. Pulls the buffer
    // back out of Pix2D's STATE so the caller can present it.
    pub fn unbind(&mut self) {
        let mut s = crate::graphics::pix2d::STATE.lock().unwrap();
        self.data = std::mem::take(&mut s.pixels);
    }
}
