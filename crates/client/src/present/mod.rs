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
