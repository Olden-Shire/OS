fn main() {
    // The original Java client ran with a much larger default stack than Rust's
    // 1 MB main thread. Some cache decoders (e.g. the archive-8 sprite groups)
    // recurse deeply enough to overflow it, so reserve a large main-thread
    // stack. cfg!(windows) in a build script tests the HOST, not the target —
    // read TARGET so cross-builds (wasm32) don't get MSVC flags.
    let target = std::env::var("TARGET").unwrap_or_default();
    if target.contains("windows-msvc") {
        // MSVC linker: /STACK:reserve[,commit]. The OS only commits pages as
        // they are touched, so the reservation is essentially free.
        println!("cargo:rustc-link-arg=/STACK:268435456"); // 256 MB reserve
    } else if target.starts_with("wasm32") {
        // wasm-ld: the wasm shadow stack is fixed at link time (default 1 MB).
        println!("cargo:rustc-link-arg=-zstack-size=16777216"); // 16 MB
    }
}
