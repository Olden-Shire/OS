// custom — host-platform shims (not part of the gamepack).
//
// std::time::Instant/SystemTime panic on wasm32-unknown-unknown; web-time
// provides API-identical types over performance.now()/Date.now(). Every
// module that needs a clock imports it from here instead of std.

#![allow(unused_imports)]

#[cfg(not(target_arch = "wasm32"))]
pub use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[cfg(target_arch = "wasm32")]
pub use web_time::{Instant, SystemTime, UNIX_EPOCH};
