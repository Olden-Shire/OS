// @ObfuscatedName("bi") — jag::oldscape::dash3d::TextureManager implements
// jag::oldscape::dash3d::TextureProvider ("aw").
//
// Loads texture metadata from the textures archive (slot 9). Each
// texture's header records an `averageRgb` colour and the opaque flag —
// enough for the renderer to tint a face without the full texel sampler.
// Full Pix8 + gamma + palette load lives in Texture.loadTexture; we
// defer that until textured triangle rasterization is wired.

#![allow(dead_code)]

use std::sync::Mutex;
use std::sync::atomic::{AtomicI32, Ordering};

use crate::io::packet::Packet;
use crate::js5::js5_loader::Js5Loader;
use crate::js5::js5_net;

// @ObfuscatedName("bi.j") — textures archive slot.
pub static TEXTURES_SLOT: AtomicI32 = AtomicI32::new(-1);
// custom — Java holds this as a separate field on TextureManager.
pub static SPRITES_SLOT: AtomicI32 = AtomicI32::new(-1);

#[derive(Debug, Clone, Default)]
pub struct Texture {
    // @ObfuscatedName("er.z") — averaged RGB stored as RGB565 in the
    // file, expanded to 24-bit RGB at load time.
    pub average_rgb: i32,
    // @ObfuscatedName("er.g")
    pub opaque: bool,
    // @ObfuscatedName("er.q")
    pub file_ids: Vec<i32>,
    // @ObfuscatedName("er.i") / "er.s" / "er.u"
    pub op1: Vec<i32>,
    pub op2: Vec<i32>,
    pub op3: Vec<i32>,
    // @ObfuscatedName("er.v") / "er.w"
    pub animation_direction: i32,
    pub animation_speed: i32,
    // @ObfuscatedName("er.e") — 128×128 (or 64×64) RGB texels expanded
    // from Pix8 palette + gamma. None until load_texels lands them.
    // Wrapped in Arc so callers can share without cloning the 64 KB
    // buffer per face (Java returns the raw int[] reference).
    pub texels: Option<std::sync::Arc<Vec<i32>>>,
    // Texels side length (128 or 64).
    pub size: i32,
    // @ObfuscatedName("er.field1689") — set true by get_texels each time
    // the texture is sampled this frame; runAnims only animates textures
    // that were actually drawn (and clears the flag afterwards).
    pub used: bool,
}

impl Texture {
    // @ObfuscatedName("er") — Texture(Packet) constructor.
    //
    // Verbatim port: averageRgb stays as the RAW g2() value (Java
    // Texture.java:47 — `this.averageRgb = buf.g2()`). The downstream
    // consumer is Pix3D::textureLightColour which masks with 0x7F and
    // 0xFF80 — i.e. it treats this as an HSL palette index, NOT
    // RGB888. The previous expansion to 24-bit broke the fallback
    // gouraud path for unloaded-texture rendering.
    pub fn decode(p: &mut Packet) -> Option<Self> {
        let average_rgb = p.g2();
        let opaque = p.g1() == 1;
        let op_count = p.g1();
        if op_count < 1 || op_count > 4 { return None; }
        let mut t = Texture {
            average_rgb,
            opaque,
            ..Default::default()
        };
        t.file_ids = (0..op_count).map(|_| p.g2()).collect();
        if op_count > 1 {
            t.op1 = (0..op_count - 1).map(|_| p.g1()).collect();
            t.op2 = (0..op_count - 1).map(|_| p.g1()).collect();
        }
        t.op3 = (0..op_count).map(|_| p.g4()).collect();
        t.animation_direction = p.g1();
        t.animation_speed = p.g1();
        Some(t)
    }

    // @ObfuscatedName("er.c(I)V") — Texture.animate.
    //
    // Verbatim port of Texture.java:175-240. Scrolls the texel buffer by
    // `cycle * speed` (the per-frame tick delta runAnims passes), wrapping
    // with a power-of-two mask. Directions 1/3 scroll the whole buffer
    // (vertical/column scroll); 2/4 scroll each row independently
    // (horizontal scroll). 1/2 scroll negative, 3/4 positive. The result
    // replaces `texels`, so the next get_texels hands the rasterizer the
    // advanced frame.
    pub fn animate(&mut self, cycle: i32) {
        let Some(src) = self.texels.as_ref() else { return };
        let len = src.len();
        // Java: `texels.length == 4096 ? 64 : 128`.
        let dim = if len == 4096 { 64 } else { 128 };
        let mut out = vec![0i32; len];

        if self.animation_direction == 1 || self.animation_direction == 3 {
            let mut off = cycle * dim * self.animation_speed;
            let mask = (len - 1) as i32;
            if self.animation_direction == 1 {
                off = -off;
            }
            for i in 0..len {
                let j = (off.wrapping_add(i as i32) & mask) as usize;
                out[i] = src[j];
            }
        } else if self.animation_direction == 2 || self.animation_direction == 4 {
            let mut off = self.animation_speed * cycle;
            let mask = dim - 1;
            if self.animation_direction == 2 {
                off = -off;
            }
            let mut row = 0usize;
            while row < len {
                for x in 0..dim as usize {
                    let dst = row + x;
                    let read_x = (off.wrapping_add(x as i32) & mask) as usize;
                    out[dst] = src[read_x + row];
                }
                row += dim as usize;
            }
        } else {
            return;
        }

        self.texels = Some(std::sync::Arc::new(out));
    }
}

// @ObfuscatedName("bi.w(I)V") — TextureManager.runAnims.
//
// Called once per frame after world.renderAll. Advances every texture
// that has a scroll direction AND was sampled this frame (the `used`
// flag get_texels sets), then clears the flag. `cycle` is Java's
// worldUpdateNum — the count of game ticks that elapsed this frame, so
// the scroll speed tracks game time rather than render framerate.
pub fn run_anims(cycle: i32) {
    let mut store = STORE.lock().unwrap();
    for slot in store.textures.values_mut() {
        if let Some(t) = slot.as_mut() {
            if t.animation_direction != 0 && t.used {
                t.animate(cycle);
                t.used = false;
            }
        }
    }
}

pub struct TextureStore {
    // @ObfuscatedName("bi.r")
    pub textures: std::collections::HashMap<i32, Option<Texture>>,
}
pub static STORE: std::sync::LazyLock<Mutex<TextureStore>> = std::sync::LazyLock::new(|| {
    Mutex::new(TextureStore { textures: std::collections::HashMap::new() })
});

// @ObfuscatedName("aw.d(II)I") — TextureProvider.getAverageRgb
pub fn get_average_rgb(texture_id: i32) -> i32 {
    if let Some(slot) = STORE.lock().unwrap().textures.get(&texture_id) {
        return slot.as_ref().map(|t| t.average_rgb).unwrap_or(0);
    }
    let t_slot = TEXTURES_SLOT.load(Ordering::Relaxed);
    if t_slot < 0 { return 0; }
    let bytes_opt = {
        let mut reg = js5_net::LOADERS.lock().unwrap();
        reg.get_mut(t_slot as usize)
            .and_then(|o| o.as_mut())
            .and_then(|l| l.fetch_file(0, texture_id))
    };
    // Don't cache a failed fetch — the textures archive may stream in
    // a few frames later, and we want the next call to retry.
    let Some(bytes) = bytes_opt else { return 0; };
    let mut p = Packet::from_vec(bytes);
    let texture = Texture::decode(&mut p);
    let avg = texture.as_ref().map(|t| t.average_rgb).unwrap_or(0);
    STORE.lock().unwrap().textures.insert(texture_id, texture);
    avg
}

// @ObfuscatedName("er.c(DILch;)Z") — Texture.loadTexture
//
// Decodes the sprite pointed to by file_ids[0] via the sprites JS5
// archive, expands the palette into a `size × size` RGB texel buffer
// with gamma 0.8, and stores it on the texture. Subsequent texture
// lookups go straight to `get_texels`. Returns false if the sprite
// isn't yet available — caller should retry next frame.
fn load_texels_into(t: &mut Texture, size: i32) -> bool {
    use crate::graphics::pix_loader;
    let s_slot = SPRITES_SLOT.load(Ordering::Relaxed);
    if s_slot < 0 || t.file_ids.is_empty() { return false; }
    let sprite_id = t.file_ids[0];
    // Sprites archive stores each sprite at group=sprite_id, file=0
    // (every sprite is a 1-entry group). Fetch the raw archive bytes,
    // hand them to PixLoader::depack, then drain via make_pix8_single.
    let sprite_bytes = {
        let mut reg = js5_net::LOADERS.lock().unwrap();
        let Some(loader) = reg.get_mut(s_slot as usize).and_then(|o| o.as_mut()) else { return false; };
        loader.fetch_file(sprite_id, 0)
    };
    let Some(bytes) = sprite_bytes else { return false; };
    pix_loader::depack(&bytes);
    let mut p8 = pix_loader::make_pix8_single();
    p8.trim();
    // Apply gamma 0.8 to the palette before expanding.
    for c in p8.bpal.iter_mut() {
        let r = (*c >> 16) & 0xFF;
        let g = (*c >> 8) & 0xFF;
        let b = *c & 0xFF;
        let rr = (((r as f64) / 256.0).powf(0.8) * 256.0) as i32;
        let gg = (((g as f64) / 256.0).powf(0.8) * 256.0) as i32;
        let bb = (((b as f64) / 256.0).powf(0.8) * 256.0) as i32;
        *c = (rr.clamp(0, 255) << 16) | (gg.clamp(0, 255) << 8) | bb.clamp(0, 255);
    }
    // Expand and resize to `size × size`. The sprite is typically
    // already 128×128, but Java also handles 64→128 upscaling and
    // 128→64 downscaling so we mirror that here.
    let total = (size * size) as usize;
    let mut tex = vec![0i32; total];
    if p8.wi == size {
        for i in 0..total {
            tex[i] = p8.bpal[(p8.data[i] as i32 & 0xFF) as usize];
        }
    } else if p8.wi == 64 && size == 128 {
        let mut k = 0usize;
        for y in 0..size {
            for x in 0..size {
                let src_idx = ((y >> 1) << 6) + (x >> 1);
                tex[k] = p8.bpal[(p8.data[src_idx as usize] as i32 & 0xFF) as usize];
                k += 1;
            }
        }
    } else if p8.wi == 128 && size == 64 {
        let mut k = 0usize;
        for y in 0..size {
            for x in 0..size {
                let src_idx = ((y << 1) << 7) + (x << 1);
                tex[k] = p8.bpal[(p8.data[src_idx as usize] as i32 & 0xFF) as usize];
                k += 1;
            }
        }
    } else {
        return false;
    }
    t.texels = Some(std::sync::Arc::new(tex));
    t.size = size;
    true
}

// @ObfuscatedName("aw.b(I)[I") — TextureProvider.getTexels
//
// Returns an Arc<Vec<i32>> handle to the 128×128 RGB texel buffer.
// Java returns the raw `int[]` reference; we wrap in Arc so the
// rasterizer can borrow without cloning 64 KB per textured face.
// Returns None if the texture has no sprite or if the sprite archive
// hasn't streamed in yet.
pub fn get_texels(texture_id: i32) -> Option<std::sync::Arc<Vec<i32>>> {
    {
        // Java getTexels marks the texture "used this frame" (field1689)
        // so runAnims only animates textures that were actually drawn.
        let mut store = STORE.lock().unwrap();
        if let Some(Some(t)) = store.textures.get_mut(&texture_id) {
            if let Some(texels) = &t.texels {
                let arc = std::sync::Arc::clone(texels);
                t.used = true;
                return Some(arc);
            }
        }
    }
    // Texture not yet decoded: bring it in via the average-RGB path
    // (that fetches the header), then load its texels and re-read.
    let _ = get_average_rgb(texture_id);
    let mut store = STORE.lock().unwrap();
    let entry = store.textures.get_mut(&texture_id)?;
    let t = entry.as_mut()?;
    if t.texels.is_none() {
        if !load_texels_into(t, 128) { return None; }
    }
    t.used = true;
    t.texels.as_ref().map(std::sync::Arc::clone)
}

// @ObfuscatedName("aw.l(II)Z") — TextureProvider.isOpaque
pub fn is_opaque(texture_id: i32) -> bool {
    if let Some(slot) = STORE.lock().unwrap().textures.get(&texture_id) {
        return slot.as_ref().map(|t| t.opaque).unwrap_or(false);
    }
    let _ = get_average_rgb(texture_id);
    STORE.lock().unwrap().textures.get(&texture_id)
        .and_then(|s| s.as_ref())
        .map(|t| t.opaque)
        .unwrap_or(false)
}

pub fn init(_archive: &Js5Loader) {}

pub fn install_archives(textures_slot: i32, sprites_slot: i32) {
    TEXTURES_SLOT.store(textures_slot, Ordering::Relaxed);
    SPRITES_SLOT.store(sprites_slot, Ordering::Relaxed);
}
