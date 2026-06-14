// Client library root — every gamepack subsystem lives here so tools
// (jaged) can reuse the 1:1 ports directly; the thin `client` bin and
// the wasm entry both drive it through `app`.

// wasm: stderr goes nowhere in the browser — shadow eprintln! crate-wide
// (textual macro scope: defined at the crate root before the mods) so all
// the existing diagnostics land in the devtools console instead.
#[cfg(target_arch = "wasm32")]
macro_rules! eprintln {
    ($($t:tt)*) => {
        web_sys::console::log_1(&format!($($t)*).into())
    };
}

/// `OS_DEBUG` gate for verbose diagnostic logging — checked once, cached.
/// Set the env var `OS_DEBUG` (to any value) to surface gated `dbg_log!` output.
/// Defined at the crate root before the mods so the macro is in textual scope
/// (same reason the wasm `eprintln!` shadow above lives here).
pub fn debug_enabled() -> bool {
    use std::sync::atomic::{AtomicU8, Ordering};
    static STATE: AtomicU8 = AtomicU8::new(0);
    match STATE.load(Ordering::Relaxed) {
        1 => false,
        2 => true,
        _ => {
            let on = std::env::var_os("OS_DEBUG").is_some();
            STATE.store(if on { 2 } else { 1 }, Ordering::Relaxed);
            on
        }
    }
}

/// `eprintln!` that only fires when `OS_DEBUG` is set (gated diagnostics).
#[macro_export]
macro_rules! dbg_log {
    ($($arg:tt)*) => {
        if $crate::debug_enabled() { eprintln!($($arg)*); }
    };
}

pub mod app;
pub mod applet;
pub mod client;
pub mod client_build;
pub mod client_script;
pub mod config;
pub mod dash3d;
pub mod datastruct;
pub mod game_canvas;
pub mod game_shell;
pub mod graphics;
pub mod host;
pub mod imgui_overlay;
pub mod input;
pub mod interface_loop;
pub mod interface_render;
pub mod io;
pub mod client_inv_cache;
pub mod debug_depth;
pub mod debug_opts;
pub mod friend;
pub mod js5;
pub mod jag_exception;
pub mod javconfig;
pub mod jstring;
pub mod login;
pub mod mem_report;
pub mod midi2;
pub mod minimap;
pub mod namespace;
pub mod obfuscation;
pub mod overlays;
pub mod perf;
pub mod present;
pub mod reflection_checker;
pub mod scene;
pub mod script_runner;
pub mod settings;
pub mod skills;
pub mod sound;
pub mod string_constants;
pub mod text;
pub mod title_screen;
pub mod util;
pub mod wasm_libc;
pub mod wordpack;
pub mod world_entry;

// The ::fpson overlay's "Mem:Nk" line reads live heap usage (Java asks the
// GC runtime; we count allocations instead). Lives at the lib root so the
// bin and any tool linking this crate share the counting allocator.
#[global_allocator]
static ALLOCATOR: perf::CountingAllocator = perf::CountingAllocator;
