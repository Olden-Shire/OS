// Thin desktop entry — all client code lives in the library crate so
// tools (jaged) can link the same 1:1 ports. See src/app.rs for the
// winit host.
fn main() {
    client::app::run();
}
