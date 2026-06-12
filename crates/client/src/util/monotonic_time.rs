// @ObfuscatedName("aj") — jag::oldscape::util::MonotonicTime.
//
// Provides a monotonic millisecond clock that can't tick backwards.
// Java's version reads System.currentTimeMillis(), guards against
// leap-second / NTP rollback by saving the highest seen value, and
// returns it whenever the OS clock goes backwards.

#![allow(dead_code)]

use std::sync::Mutex;
use crate::host::{SystemTime, UNIX_EPOCH};

static STATE: Mutex<i64> = Mutex::new(0);

// @ObfuscatedName("aj.r()J") — MonotonicTime.currentTime
pub fn current_time() -> i64 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let mut last = STATE.lock().unwrap();
    if now > *last { *last = now; }
    *last
}
