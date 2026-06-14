//! World tick, entities (Player/Npc), entity-update protocol
//! builders, and the RuneScript runtime (mirrors the Engine-TS
//! reference `src/engine`; Engine2007/Engine-TS are reference only).

pub mod base37;
pub mod collision;
pub mod entity;
pub mod info;
pub mod script;
pub mod skills;
pub mod world;
pub mod zone;

pub use world::{ChatLine, CycleStats, World};

/// `OS_DEBUG` gate for verbose diagnostic logging — checked once, cached.
/// Set the env var `OS_DEBUG` (to any value) to surface gated `dbg_log!` output.
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
