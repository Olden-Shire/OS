// custom — host presentation layer (not part of the gamepack).
//
// The game core renders into a fixed 765x503 CPU framebuffer (the 1:1
// Pix2D/Pix3D software pipeline — that buffer IS the byte-faithful output).
// A Present backend owns the OS surface and is responsible for stretching
// that frame to the window and drawing the imgui perf overlay on top at
// native window resolution:
//
//   soft — softbuffer CPU blit (fallback; works everywhere winit does)
//   gl   — glow/glutin textured quad (desktop GL today; the same glow code
//          targets GLES on Android and WebGL2 on web)
//
// Backends deliberately consume the *finished* CPU frame, so the game core
// has zero knowledge of the windowing/GPU stack. A future GPU scene
// renderer (Pix3D-on-GPU as an optional, non-1:1 backend) would extend this
// trait with a render-target hook rather than replacing it — the CPU frame
// path must keep working as the fidelity reference.

pub mod gl;
pub mod soft;

// Destination rect (x, y, w, h, top-left origin in window coords) of the
// game frame inside the window, top-centred like the Java applet on the
// game page. Default scales the 765x503 frame by the LARGEST WHOLE
// FACTOR that fits the window (lossless — every game pixel becomes an
// exact k×k block; both backends sample nearest). The "integer scaling"
// toggle off pins it to 1:1; the "stretched" toggle fills the window
// (non-lossless). Both backends AND the window→game mouse transform
// must use this one function or clicks drift from pixels.
pub fn layout_rect(fw: u32, fh: u32, win_w: u32, win_h: u32) -> (i32, i32, u32, u32) {
    if crate::debug_opts::stretched() {
        return (0, 0, win_w.max(1), win_h.max(1));
    }
    let k = if crate::debug_opts::integer_scale() {
        (win_w / fw.max(1)).min(win_h / fh.max(1)).max(1)
    } else {
        1
    };
    let dw = (fw * k).max(1);
    let dh = (fh * k).max(1);
    ((win_w.saturating_sub(dw) / 2) as i32, 0, dw, dh)
}

pub trait Present {
    // Short name for logs / the perf overlay.
    fn name(&self) -> &'static str;

    // Window grew/shrank — resize the swapchain/surface to match.
    fn resize(&mut self, width: u32, height: u32);

    // Stretch the finished game frame (`frame`, `fw` x `fh`) to the window
    // (`win_w` x `win_h`), draw the overlay, and flip.
    #[allow(clippy::too_many_arguments)]
    fn present(
        &mut self,
        frame: &[u32],
        fw: u32,
        fh: u32,
        win_w: u32,
        win_h: u32,
        overlay: &mut crate::imgui_overlay::PerfOverlay,
        mouse: (f32, f32),
        buttons: (bool, bool),
    );
}
