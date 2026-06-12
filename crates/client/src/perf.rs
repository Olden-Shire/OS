// custom — frame-time profiler feeding the imgui benchmark overlay.
//
// Not part of the gamepack. Each instrumented system wraps its work in
// `perf::scope(Scope::X)` (a drop guard that accumulates elapsed time into
// the current frame's bucket); the host loop calls `end_frame()` once per
// presented frame to rotate the ring buffer the overlay graphs.

#![allow(dead_code)]

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;
use crate::host::Instant;

// ── Heap counter (Java Runtime.totalMemory()-freeMemory() stand-in) ────
//
// The `::fpson` overlay's "Mem:Nk" line needs live heap usage; Rust has no
// GC runtime to ask, so the binary installs this counting wrapper around
// the system allocator (see main.rs #[global_allocator]).

pub static HEAP_BYTES: AtomicUsize = AtomicUsize::new(0);

pub struct CountingAllocator;

unsafe impl std::alloc::GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
        let p = unsafe { std::alloc::System.alloc(layout) };
        if !p.is_null() {
            HEAP_BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        }
        p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: std::alloc::Layout) {
        unsafe { std::alloc::System.dealloc(ptr, layout) };
        HEAP_BYTES.fetch_sub(layout.size(), Ordering::Relaxed);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: std::alloc::Layout, new_size: usize) -> *mut u8 {
        let p = unsafe { std::alloc::System.realloc(ptr, layout, new_size) };
        if !p.is_null() {
            if new_size >= layout.size() {
                HEAP_BYTES.fetch_add(new_size - layout.size(), Ordering::Relaxed);
            } else {
                HEAP_BYTES.fetch_sub(layout.size() - new_size, Ordering::Relaxed);
            }
        }
        p
    }
}

// Used heap in KiB — the Java overlay prints (totalMemory-freeMemory)/1024.
pub fn heap_used_kb() -> i32 {
    (HEAP_BYTES.load(Ordering::Relaxed) / 1024) as i32
}

// ── Per-system frame timing ─────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    // Client.mainloop — game logic, net, pathing.
    Logic = 0,
    // interface_render::draw_chrome minus the nested Scene/Minimap scopes
    // (the overlay derives "ui 2d" by subtraction).
    Chrome = 1,
    // scene::draw_viewport — the 3D world render.
    Scene = 2,
    // minimap::draw.
    Minimap = 3,
    // Present path: stretch_blit / surface copy.
    Blit = 4,
    // The overlay itself (build + software raster), measured last frame.
    Imgui = 5,
}

pub const SCOPE_COUNT: usize = 6;

// ~5.6 seconds of history at 50 fps; one pixel per frame in the graph.
pub const HISTORY: usize = 280;

pub struct PerfState {
    // ms accumulated this frame, per scope.
    pub current: [f32; SCOPE_COUNT],
    // ring buffer of completed frames.
    pub history: Vec<[f32; SCOPE_COUNT]>,
    // next write position in `history`.
    pub head: usize,
    // wall-clock interval between end_frame calls (full frame incl. sleep).
    pub frame_interval_ms: f32,
    last_frame_end: Option<Instant>,
}

pub static PERF: Mutex<PerfState> = Mutex::new(PerfState {
    current: [0.0; SCOPE_COUNT],
    history: Vec::new(),
    head: 0,
    frame_interval_ms: 0.0,
    last_frame_end: None,
});

// Whether the benchmark overlay is drawn — toggled by the (custom, dev-only)
// `::perf` cheat. On by default: it replaces the old always-on debug text.
pub static OVERLAY_VISIBLE: AtomicBool = AtomicBool::new(true);

pub fn toggle_overlay() {
    OVERLAY_VISIBLE.fetch_xor(true, Ordering::Relaxed);
}

pub fn overlay_visible() -> bool {
    OVERLAY_VISIBLE.load(Ordering::Relaxed)
}

pub struct ScopeGuard {
    scope: Scope,
    start: Instant,
}

impl Drop for ScopeGuard {
    fn drop(&mut self) {
        let ms = self.start.elapsed().as_secs_f32() * 1000.0;
        let mut p = PERF.lock().unwrap();
        p.current[self.scope as usize] += ms;
    }
}

// Time a section: `let _t = perf::scope(perf::Scope::Scene);`
pub fn scope(scope: Scope) -> ScopeGuard {
    ScopeGuard { scope, start: Instant::now() }
}

// Rotate the ring after a frame is presented.
pub fn end_frame() {
    let mut p = PERF.lock().unwrap();
    if p.history.is_empty() {
        p.history = vec![[0.0; SCOPE_COUNT]; HISTORY];
    }
    let head = p.head;
    p.history[head] = p.current;
    p.head = (head + 1) % HISTORY;
    p.current = [0.0; SCOPE_COUNT];
    let now = Instant::now();
    if let Some(prev) = p.last_frame_end {
        p.frame_interval_ms = now.duration_since(prev).as_secs_f32() * 1000.0;
    }
    p.last_frame_end = Some(now);
}

// Copy out (history in oldest→newest order, frame interval) for the overlay.
pub fn snapshot() -> (Vec<[f32; SCOPE_COUNT]>, f32) {
    let p = PERF.lock().unwrap();
    if p.history.is_empty() {
        return (vec![[0.0; SCOPE_COUNT]; HISTORY], p.frame_interval_ms);
    }
    let mut out = Vec::with_capacity(HISTORY);
    out.extend_from_slice(&p.history[p.head..]);
    out.extend_from_slice(&p.history[..p.head]);
    (out, p.frame_interval_ms)
}
