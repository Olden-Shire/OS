// @ObfuscatedName("dj")
//
// Java original is an abstract Applet subclass that owns the AWT Frame +
// Canvas, the timing loop, and the red Jagex progress bar. In the port we
// keep the same field layout (per-member @ObfuscatedName) but split
// behaviour: the gamepack statics live on `ShellState`, the abstract hooks
// live on the `GameShellLifecycle` trait, and the drawing routines are free
// functions that take a `Framebuffer`.

#![allow(dead_code, non_upper_case_globals)]

use std::sync::Mutex;
use crate::host::Instant;

use ab_glyph::{Font, FontVec, PxScale, ScaleFont};

// custom — Jagex red default tint baked into GameShell.drawProgress
// (`new Color(140, 17, 17)`).
pub const PROGRESS_DEFAULT_COLOR: u32 = pack_rgb(140, 17, 17);

// custom — 304x34 box dimensions baked into GameShell.drawProgress.
pub const PROGRESS_BOX_W: i32 = 304;
pub const PROGRESS_BOX_H: i32 = 34;

// custom — `new Font("Helvetica", Font.BOLD, 13)` baked size.
pub const PROGRESS_FONT_PX: f32 = 13.0;

// custom container — every `public static` field on @ObfuscatedName("dj") is
// laid out below with its own annotation. Held in one struct so call sites
// read like the gamepack (SHELL.lock().fullredraw = true;).
pub struct ShellState {
    // @ObfuscatedName("dj.l")
    pub loaded: i32,

    // @ObfuscatedName("dj.m")
    pub killtime: i64,

    // @ObfuscatedName("dj.c")
    pub already_shutdown: bool,

    // @ObfuscatedName("dj.n") — instance field in the gamepack, hoisted
    // onto the singleton state because Client is the only shell.
    pub already_errored: bool,

    // @ObfuscatedName("dj.j")
    pub update_count: i32,

    // @ObfuscatedName("dj.z")
    pub deltime: i32,

    // @ObfuscatedName("dj.g")
    pub mindel: i32,

    // @ObfuscatedName("dj.q")
    pub fps: i32,

    // custom — average ms/frame over the 32-frame draw_time window,
    // derived alongside `fps` in mainredrawwrapper for the debug overlay.
    pub frame_ms: i32,

    // @ObfuscatedName("dj.u")
    pub draw_time: [i64; 32],

    // @ObfuscatedName("bm.v")
    pub draw_pos: i32,

    // @ObfuscatedName("dj.w")
    pub update_time: [i64; 32],

    // @ObfuscatedName("cv.e")
    pub update_pos: i32,

    // @ObfuscatedName("dj.b")
    pub s_wid: u32,

    // @ObfuscatedName("ao.t")
    pub s_hei: u32,

    // @ObfuscatedName("dj.p")
    pub fullredraw: bool,

    // @ObfuscatedName("dj.ac")
    pub redraw_num: i32,

    // @ObfuscatedName("dj.aa")
    pub canvas_replace_recommended: bool,

    // @ObfuscatedName("dj.as")
    pub last_canvas_replace: i64,

    // @ObfuscatedName("dj.am")
    pub focus_in: bool,

    // @ObfuscatedName("z.ap")
    pub focus: bool,
}

impl ShellState {
    // custom ctor — Java init is per-static-field in the class body.
    pub const fn new() -> Self {
        Self {
            loaded: 0,
            killtime: 0,
            already_shutdown: false,
            already_errored: false,
            update_count: 0,
            deltime: 20,
            mindel: 1,
            fps: 0,
            frame_ms: 0,
            draw_time: [0; 32],
            draw_pos: 0,
            update_time: [0; 32],
            update_pos: 0,
            s_wid: 0,
            s_hei: 0,
            fullredraw: true,
            redraw_num: 500,
            canvas_replace_recommended: false,
            last_canvas_replace: 0,
            focus_in: true,
            focus: false,
        }
    }
}

// @ObfuscatedName("dj.d") — the singleton `shell` reference. Guarded by a
// Mutex so the `if (shell != null)` reentry check in dj.z(IIIB)V can be
// expressed as a lock-and-test.
pub static SHELL: Mutex<ShellState> = Mutex::new(ShellState::new());

// @ObfuscatedName(— GameShell.focusGained). Verbatim port of
// GameShell.java:461-465. Window-focus sink: marks `focus_in` true
// and forces a full repaint on the next redraw. Host must call this
// from its winit FocusChanged(true) event.
pub fn on_focus_gained() {
    let mut s = SHELL.lock().unwrap();
    s.focus_in = true;
    s.fullredraw = true;
}

// @ObfuscatedName(— GameShell.focusLost). Verbatim port of
// GameShell.java:467-468. Window-focus sink: clears `focus_in`.
pub fn on_focus_lost() {
    let mut s = SHELL.lock().unwrap();
    s.focus_in = false;
}

// @ObfuscatedName("bk.v(B)V") — GameShell.doneslowupdate. Verbatim
// port of GameShell.java:394-407. Called after a slow-update stall
// (e.g. GC pause, window resize, focus regain) to zero the 32-entry
// drawTime/updateTime rolling windows and reset updateCount, so the
// FPS / update-rate display reads accurately once the loop resumes.
pub fn done_slow_update() {
    let mut s = SHELL.lock().unwrap();
    for i in 0..32 { s.draw_time[i] = 0; }
    for i in 0..32 { s.update_time[i] = 0; }
    s.update_count = 0;
}

// jagex3.util.MonotonicTime.currentTime — millisecond-precision monotonic
// clock used by GameShell for killtime / drawTime / updateTime. The class
// has its own @ObfuscatedName which will be filled in when MonotonicTime is
// ported as its own module.
pub fn monotonic_ms() -> i64 {
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    let t0 = *START.get_or_init(Instant::now);
    t0.elapsed().as_millis() as i64
}

// custom trait — the six `abstract` methods on @ObfuscatedName("dj").
// Concrete impl lives on @ObfuscatedName("client").
pub trait GameShellLifecycle {
    // override of java.applet.Applet.init — no @ObfuscatedName
    fn init(&mut self);

    // @ObfuscatedName("dj.w(I)V")
    fn maininit(&mut self);

    // @ObfuscatedName("dj.e(B)V")
    fn mainloop(&mut self);

    // @ObfuscatedName("dj.b(I)V") — pixel buffer + dims provided by the host loop
    fn mainredraw(&mut self, fb: &mut Framebuffer);

    // @ObfuscatedName("dj.y(B)V")
    fn mainquit(&mut self);

    // @ObfuscatedName("dj.f(I)V")
    fn on_killed(&mut self);
}

// custom — softbuffer framebuffer wrapper structured to look like the
// PixMap / Pix32 abstraction GameShell.drawProgress draws into. PixMap has
// its own @ObfuscatedName which will be filled in when it's ported.
//
// 0x00RRGGBB packing — matches softbuffer's expected u32 layout and AWT
// Color.getRGB() encoding.
pub const fn pack_rgb(r: u8, g: u8, b: u8) -> u32 {
    ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
}

pub struct Framebuffer<'a> {
    pub pixels: &'a mut [u32],
    pub width: i32,
    pub height: i32,
}

impl<'a> Framebuffer<'a> {
    pub fn new(pixels: &'a mut [u32], width: i32, height: i32) -> Self {
        Self { pixels, width, height }
    }

    // java.awt.Graphics.fillRect — clipped to the framebuffer bounds.
    pub fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: u32) {
        let x0 = x.max(0);
        let y0 = y.max(0);
        let x1 = (x + w).min(self.width);
        let y1 = (y + h).min(self.height);
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        for py in y0..y1 {
            let row = (py * self.width) as usize;
            for px in x0..x1 {
                self.pixels[row + px as usize] = color;
            }
        }
    }

    // java.awt.Graphics.drawRect — 1px outline, inclusive of right/bottom
    // edges (AWT drawRect(x,y,w,h) draws (w+1) x (h+1) pixels).
    pub fn draw_rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: u32) {
        self.fill_rect(x, y, w + 1, 1, color);
        self.fill_rect(x, y + h, w + 1, 1, color);
        self.fill_rect(x, y, 1, h + 1, color);
        self.fill_rect(x + w, y, 1, h + 1, color);
    }
}

// custom container — the three GameShell statics drawProgress mutates.
// Kept together so resetProgress can clear them in one place.
pub struct ProgressCache {
    // @ObfuscatedName("cd.ad") — the 304x34 backing Image in Java. softbuffer
    // already provides the backing surface, so this slot stays empty in the
    // port but the annotation is preserved for diffs against future revisions.
    pub bar: Option<()>,

    // @ObfuscatedName("ca.f") + @ObfuscatedName("fr.k") — progressFont and
    // progressFontMetrics, fused into a single ab_glyph FontVec.
    pub font: Option<FontVec>,

    // true when `font` is the embedded Ubuntu-Light fallback (a light weight), so
    // the message is rendered faux-bold to approximate Java's Helvetica BOLD.
    pub faux_bold: bool,
}

impl ProgressCache {
    pub const fn new() -> Self {
        Self { bar: None, font: None, faux_bold: false }
    }
}

pub static PROGRESS: Mutex<ProgressCache> = Mutex::new(ProgressCache::new());

// drawProgress — no @ObfuscatedName (custom helper added on top of the
// gamepack class body in the deob source).
//
// progress is the 0..100 fill amount, message is the centred text, color is
// the bar tint (None falls back to the Jagex red).
pub fn draw_progress(fb: &mut Framebuffer, progress: i32, message: &str, color: Option<u32>) {
    let s_wid = fb.width;
    let s_hei = fb.height;

    // if (fullredraw) { g.setColor(Color.black); g.fillRect(0,0,sWid,sHei); }
    {
        let mut shell = SHELL.lock().unwrap();
        if shell.fullredraw {
            shell.fullredraw = false;
            drop(shell);
            fb.fill_rect(0, 0, s_wid, s_hei, 0x000000);
        }
    }

    let color = color.unwrap_or(PROGRESS_DEFAULT_COLOR);

    // g.drawImage(progressBar, sWid / 2 - 152, sHei / 2 - 18, null);
    let x = s_wid / 2 - 152;
    let y = s_hei / 2 - 18;

    // bar.setColor(color); bar.drawRect(0, 0, 303, 33);
    fb.draw_rect(x, y, 303, 33, color);

    // bar.fillRect(2, 2, progress * 3, 30);
    fb.fill_rect(x + 2, y + 2, progress * 3, 30, color);

    // bar.setColor(Color.black); bar.drawRect(1, 1, 301, 31);
    fb.draw_rect(x + 1, y + 1, 301, 31, 0x000000);

    // bar.fillRect(progress * 3 + 2, 2, 300 - progress * 3, 30);
    fb.fill_rect(x + progress * 3 + 2, y + 2, 300 - progress * 3, 30, 0x000000);

    // bar.setFont(progressFont); bar.setColor(Color.white);
    // bar.drawString(message, (304 - progressFontMetrics.stringWidth(message)) / 2, 22);
    draw_progress_text(fb, x, y, message);
}

// resetProgress — no @ObfuscatedName (custom helper).
pub fn reset_progress() {
    let mut cache = PROGRESS.lock().unwrap();
    cache.bar = None;
    cache.font = None;
    cache.faux_bold = false;
}

// custom helper — drawProgress's `bar.drawString(message, ..., 22)` line.
// Loads Arial Bold from the OS font dir on first use (Helvetica isn't
// shipped on Windows; AWT falls back to Arial via its font substitution).
fn draw_progress_text(fb: &mut Framebuffer, box_x: i32, box_y: i32, message: &str) {
    let mut cache = PROGRESS.lock().unwrap();
    if cache.font.is_none() {
        if let Some((bytes, faux_bold)) = load_system_bold_font() {
            if let Ok(f) = FontVec::try_from_vec(bytes) {
                cache.font = Some(f);
                cache.faux_bold = faux_bold;
            }
        }
    }
    let faux_bold = cache.faux_bold;
    let Some(font) = cache.font.as_ref() else { return };

    // AWT renders 13pt at 96dpi which is ~17px tall.
    let scale = PxScale::from(PROGRESS_FONT_PX * 96.0 / 72.0);
    let scaled = font.as_scaled(scale);

    // progressFontMetrics.stringWidth(message)
    let text_w: f32 = message.chars().map(|c| scaled.h_advance(scaled.glyph_id(c))).sum();

    // x + (304 - progressFontMetrics.stringWidth(message)) / 2
    let pen_x = box_x as f32 + (PROGRESS_BOX_W as f32 - text_w) / 2.0;
    // y + 22 (AWT baseline coordinate inside the 34px box)
    let baseline_y = box_y as f32 + 22.0;

    let mut cursor = pen_x;
    for ch in message.chars() {
        let glyph_id = scaled.glyph_id(ch);
        let h_advance = scaled.h_advance(glyph_id);
        let glyph = glyph_id.with_scale_and_position(scale, ab_glyph::point(cursor, baseline_y));
        if let Some(outlined) = font.outline_glyph(glyph) {
            let bounds = outlined.px_bounds();
            outlined.draw(|gx, gy, coverage| {
                if coverage <= 0.0 {
                    return;
                }
                let px = bounds.min.x as i32 + gx as i32;
                let py = bounds.min.y as i32 + gy as i32;
                // bar.setColor(Color.white). Faux-bold inks the pixel to the right
                // too (only when on the light embedded fallback font).
                blend_white_px(fb, px, py, coverage);
                if faux_bold {
                    blend_white_px(fb, px + 1, py, coverage);
                }
            });
        }
        cursor += h_advance;
    }
}

// Embedded fallback so the early loading text renders even where no OS font is
// reachable — notably wasm (no filesystem). Ubuntu-Light (Ubuntu Font License;
// see assets/Ubuntu-Light-LICENSE.txt). It's a light weight, so it's drawn
// faux-bold to approximate Java's Helvetica BOLD.
const EMBEDDED_PROGRESS_FONT: &[u8] = include_bytes!("../assets/Ubuntu-Light.ttf");

// custom helper — the font for the progress message. Returns `(bytes, faux_bold)`:
// a real OS bold (AWT's Helvetica->Arial substitution, the faithful look) when one
// is reachable, else the embedded Ubuntu-Light drawn faux-bold. wasm has no
// filesystem, so it always lands on the embedded font.
fn load_system_bold_font() -> Option<(Vec<u8>, bool)> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        const CANDIDATES: &[&str] = &[
            "C:\\Windows\\Fonts\\arialbd.ttf",
            "C:\\Windows\\Fonts\\Arial Bold.ttf",
            "/System/Library/Fonts/Supplemental/Arial Bold.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
            "/usr/share/fonts/TTF/DejaVuSans-Bold.ttf",
        ];
        for path in CANDIDATES {
            if let Ok(bytes) = std::fs::read(path) {
                return Some((bytes, false));
            }
        }
    }
    Some((EMBEDDED_PROGRESS_FONT.to_vec(), true))
}

// Alpha-blend white into one framebuffer pixel — the progress message ink.
fn blend_white_px(fb: &mut Framebuffer, px: i32, py: i32, coverage: f32) {
    if px < 0 || py < 0 || px >= fb.width || py >= fb.height {
        return;
    }
    let idx = (py * fb.width + px) as usize;
    let prev = fb.pixels[idx];
    let pr = ((prev >> 16) & 0xff) as f32;
    let pg = ((prev >> 8) & 0xff) as f32;
    let pb = (prev & 0xff) as f32;
    let a = coverage.clamp(0.0, 1.0);
    let nr = (pr * (1.0 - a) + 255.0 * a) as u8;
    let ng = (pg * (1.0 - a) + 255.0 * a) as u8;
    let nb = (pb * (1.0 - a) + 255.0 * a) as u8;
    fb.pixels[idx] = pack_rgb(nr, ng, nb);
}
