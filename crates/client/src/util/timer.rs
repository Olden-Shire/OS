// jag::oldscape::util::{Timer, MillisTimer, NanoTimer}.
//
// Tick-pacing abstraction the game-loop uses to maintain a steady
// frame rate. `count(deltime, mindel)` returns how many catch-up
// ticks to run THIS frame plus updates the internal clock; we mirror
// the same shape so callers don't need to special-case either
// backend.

#![allow(dead_code)]

use crate::host::Instant;

pub trait Timer {
    /// @ObfuscatedName("z.r(II)I") — Timer.count.
    ///
    /// `deltime` = target ms-per-tick, `mindel` = minimum sleep ms.
    /// Returns the number of catch-up ticks needed to bring the
    /// timer up to "real" time. Java's impl walks an internal clock
    /// counter and returns the elapsed-tick count.
    fn count(&mut self, deltime: i32, mindel: i32) -> i32;

    /// Reset the timer's internal clock to the current moment.
    fn reset(&mut self);
}

// @ObfuscatedName("au") — jag::oldscape::util::MillisTimer.
//
// 1ms-resolution timer driven by SystemTime. The OS-clock backend the
// gamepack falls back to when the high-res nano-timer is unavailable.
pub struct MillisTimer {
    last_ms: i64,
}

impl MillisTimer {
    pub fn new() -> Self { Self { last_ms: super::monotonic_time::current_time() } }
}

impl Default for MillisTimer { fn default() -> Self { Self::new() } }

impl Timer for MillisTimer {
    fn count(&mut self, deltime: i32, mindel: i32) -> i32 {
        let now = super::monotonic_time::current_time();
        let elapsed = (now - self.last_ms) as i32;
        if elapsed < mindel { return 0; }
        let ticks = (elapsed / deltime.max(1)).max(1);
        self.last_ms = now;
        ticks
    }
    fn reset(&mut self) { self.last_ms = super::monotonic_time::current_time(); }
}

// @ObfuscatedName("y") — jag::oldscape::util::NanoTimer.
//
// Nanosecond-resolution timer driven by Instant. Preferred backend on
// modern hosts; falls back to MillisTimer where unsupported.
pub struct NanoTimer {
    last: Instant,
}

impl NanoTimer {
    pub fn new() -> Self { Self { last: Instant::now() } }
}

impl Default for NanoTimer { fn default() -> Self { Self::new() } }

impl Timer for NanoTimer {
    fn count(&mut self, deltime: i32, mindel: i32) -> i32 {
        let now = Instant::now();
        let elapsed_ms = (now - self.last).as_millis() as i32;
        if elapsed_ms < mindel { return 0; }
        let ticks = (elapsed_ms / deltime.max(1)).max(1);
        self.last = now;
        ticks
    }
    fn reset(&mut self) { self.last = Instant::now(); }
}
