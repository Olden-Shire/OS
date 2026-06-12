// Temporary diagnostic: recursion-depth guard for hunting the
// interface stack overflow. Panics (with backtrace) instead of
// silently blowing the stack, so the cycle is visible in the log.

use std::cell::Cell;

thread_local! {
    static DEPTH: Cell<usize> = const { Cell::new(0) };
}

pub struct DepthGuard;

impl DepthGuard {
    pub fn enter(site: &str, limit: usize) -> DepthGuard {
        DEPTH.with(|d| {
            let n = d.get() + 1;
            d.set(n);
            assert!(n <= limit, "recursion depth {n} exceeded at {site}");
        });
        DepthGuard
    }
}

impl Drop for DepthGuard {
    fn drop(&mut self) {
        DEPTH.with(|d| d.set(d.get() - 1));
    }
}
