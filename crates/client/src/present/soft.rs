// custom — softbuffer Present backend: pure-CPU stretch blit + software
// imgui raster. The fallback path; runs anywhere winit does (softbuffer
// presents via ANativeWindow on Android too). Not compiled on wasm —
// the browser path is GL-only.

#![cfg(not(target_arch = "wasm32"))]

use std::num::NonZeroU32;
use std::rc::Rc;

use softbuffer::{Context, Surface};
use winit::window::Window;

use crate::imgui_overlay::{raster_draw_data, PerfOverlay};
use crate::perf;

pub struct SoftPresent {
    surface: Surface<Rc<Window>, Rc<Window>>,
    // Keeps the display connection alive for the surface's lifetime.
    _context: Context<Rc<Window>>,
}

impl SoftPresent {
    pub fn new(window: Rc<Window>) -> Result<Self, Box<dyn std::error::Error>> {
        let context = Context::new(window.clone()).map_err(|e| e.to_string())?;
        let surface = Surface::new(&context, window).map_err(|e| e.to_string())?;
        Ok(Self { surface, _context: context })
    }
}

impl super::Present for SoftPresent {
    fn name(&self) -> &'static str {
        "soft"
    }

    fn resize(&mut self, _width: u32, _height: u32) {
        // The surface is resized lazily in present() from the live window
        // size — softbuffer requires it before buffer_mut() anyway.
    }

    fn present(
        &mut self,
        frame: &[u32],
        fw: u32,
        fh: u32,
        win_w: u32,
        win_h: u32,
        overlay: &mut PerfOverlay,
        mouse: (f32, f32),
        buttons: (bool, bool),
    ) {
        let (Some(w_nz), Some(h_nz)) = (NonZeroU32::new(win_w), NonZeroU32::new(win_h)) else {
            return;
        };
        if self.surface.resize(w_nz, h_nz).is_err() {
            return;
        }
        let Ok(mut buffer) = self.surface.buffer_mut() else {
            return;
        };

        {
            let _t = perf::scope(perf::Scope::Blit);
            if win_w == fw && win_h == fh {
                buffer.copy_from_slice(frame);
            } else {
                stretch_blit(frame, fw as usize, fh as usize, &mut buffer, win_w as usize, win_h as usize);
            }
        }

        overlay.frame_with(win_w, win_h, mouse, buttons, |draw_data, atlas| {
            raster_draw_data(draw_data, atlas, &mut buffer, win_w, win_h);
        });

        let _ = buffer.present();
    }
}

// Nearest-neighbour stretch of the game frame to the window surface.
// Integer column/row maps keep it cheap enough to run per frame in software.
fn stretch_blit(src: &[u32], sw: usize, sh: usize, dst: &mut [u32], dw: usize, dh: usize) {
    let mut xmap = vec![0usize; dw];
    for (x, m) in xmap.iter_mut().enumerate() {
        *m = (x * sw / dw).min(sw - 1);
    }
    for y in 0..dh {
        let sy = (y * sh / dh).min(sh - 1);
        let srow = &src[sy * sw..sy * sw + sw];
        let drow = &mut dst[y * dw..y * dw + dw];
        for (d, &m) in drow.iter_mut().zip(xmap.iter()) {
            *d = srow[m];
        }
    }
}
