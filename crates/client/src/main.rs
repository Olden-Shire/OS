// Thin desktop entry — all client code lives in the library crate so
// tools (jaged) can link the same 1:1 ports. See src/app.rs for the
// winit host.
fn main() {
    install_crash_logger();
    client::app::run();
}

/// Append any panic (message, location, full backtrace) to `client_crash.log`
/// in the working dir, then run the default stderr hook — so a crash is
/// recorded even when the window/console is gone.
fn install_crash_logger() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let bt = std::backtrace::Backtrace::force_capture();
        let thread = std::thread::current().name().unwrap_or("<unnamed>").to_string();
        let body = format!("\n==== CLIENT PANIC (thread: {thread}) ====\n{info}\n--- backtrace ---\n{bt}\n");
        if let Ok(mut f) =
            std::fs::OpenOptions::new().create(true).append(true).open("client_crash.log")
        {
            use std::io::Write;
            let _ = f.write_all(body.as_bytes());
        }
        default(info);
    }));
}
