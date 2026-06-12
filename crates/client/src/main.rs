// Client.main — no @ObfuscatedName (custom entry point).
//
// In the Java client main() does:
//
//     Client app = new Client();
//     app.startApplication(765, 503, 1);
//
// startApplication creates the AWT Frame ("Jagex"), sets it visible, then
// calls init() which calls @ObfuscatedName("dj.z(IIIB)V") (startCommon)
// which spawns a run thread via SignLink. The run thread loops
// @ObfuscatedName("dj.i(I)V") / @ObfuscatedName("dj.s(I)V")
// (mainloopwrapper / mainredrawwrapper). We mirror that here on winit: the
// host owns the window + frame budget and dispatches to the Client lifecycle.

use std::time::Duration;
use crate::host::Instant;

use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::keyboard::{Key, NamedKey, PhysicalKey};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

// wasm: stderr goes nowhere in the browser — shadow eprintln! crate-wide
// (textual macro scope: defined at the crate root before the mods) so all
// the existing diagnostics land in the devtools console instead.
#[cfg(target_arch = "wasm32")]
macro_rules! eprintln {
    ($($t:tt)*) => {
        web_sys::console::log_1(&format!($($t)*).into())
    };
}

mod applet;
mod client;
mod client_build;
mod client_script;
mod config;
mod dash3d;
mod datastruct;
mod game_canvas;
mod game_shell;
mod graphics;
mod host;
#[cfg(not(target_arch = "wasm32"))]
mod imgui_overlay;
// imgui-sys is C++ and needs a wasm-capable clang; the perf overlay is
// desktop dev tooling, so the browser build gets an inert stand-in with
// the same surface the present backends touch.
#[cfg(target_arch = "wasm32")]
mod imgui_overlay {
    pub struct PerfOverlay {
        pub want_mouse: bool,
    }
    impl PerfOverlay {
        pub fn new(_scale_factor: f32) -> Self {
            Self { want_mouse: false }
        }
        pub fn build_rgba_font_atlas(&mut self) -> (Vec<u8>, u32, u32) {
            (Vec::new(), 0, 0)
        }
    }
}
mod input;
mod interface_loop;
mod interface_render;
mod io;
mod client_inv_cache;
mod debug_depth;
mod debug_opts;
mod friend;
mod js5;
mod jag_exception;
mod javconfig;
mod jstring;
mod login;
mod mem_report;
mod midi2;
mod minimap;
mod namespace;
mod obfuscation;
mod overlays;
mod perf;
mod present;
mod reflection_checker;
mod scene;
mod script_runner;
mod settings;
mod skills;
mod sound;
mod string_constants;
mod text;
mod title_screen;
mod util;
mod wordpack;
mod world_entry;

use client::Client;
use game_shell::{Framebuffer, GameShellLifecycle, SHELL};

// The ::fpson overlay's "Mem:Nk" line reads live heap usage (Java asks the
// GC runtime; we count allocations instead). See perf::CountingAllocator.
#[global_allocator]
static ALLOCATOR: perf::CountingAllocator = perf::CountingAllocator;

// startApplication constants — no @ObfuscatedName (literals in Client.main).
const WIDTH: u32 = 765;
const HEIGHT: u32 = 503;
const REVISION: i32 = 1;

// @ObfuscatedName("dj.z") — default `deltime` (20ms == 50Hz mainloop).
const FRAME_INTERVAL: Duration = Duration::from_millis(20);

// custom — winit host glue, not part of the gamepack.
struct App {
    client: Client,
    window: Option<std::rc::Rc<Window>>,
    // GL-first presentation with softbuffer fallback (see present/mod.rs).
    present: Option<Box<dyn present::Present>>,
    next_tick: Instant,
    inited: bool,
    // custom — fixed 765x503 game framebuffer; redraw() stretch-blits it to
    // the window surface so the window can be any size.
    frame: Vec<u32>,
    // custom — imgui benchmark overlay + the raw (window-space) mouse state
    // it consumes; the game's MOUSE gets the game-space transform instead.
    overlay: Option<imgui_overlay::PerfOverlay>,
    ui_mouse: (f32, f32),
    ui_buttons: (bool, bool),
}

impl App {
    fn new() -> Self {
        let mut client = Client::new();
        // app.startApplication(765, 503, 1);
        client.start_application(WIDTH, HEIGHT, REVISION);
        Self {
            client,
            window: None,
            present: None,
            next_tick: Instant::now(),
            inited: false,
            frame: vec![0u32; (WIDTH * HEIGHT) as usize],
            overlay: None,
            ui_mouse: (0.0, 0.0),
            ui_buttons: (false, false),
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        // startApplication body — frame.setTitle("Jagex"); frame.setBackground(Color.BLACK);
        // PhysicalSize so the canvas is exactly 765x503 pixels regardless
        // of monitor DPI scaling (AWT in the gamepack draws into a 765x503
        // pixel buffer; LogicalSize would up-scale on hi-DPI displays).
        let attrs = WindowAttributes::default()
            .with_title("Jagex")
            .with_inner_size(PhysicalSize::new(WIDTH, HEIGHT))
            .with_resizable(true);

        // GL present unless overridden or context creation fails; the
        // softbuffer CPU path stays as the universal fallback on native.
        // On wasm the browser path is GL(WebGL2)-only.
        type Chosen = (
            std::rc::Rc<Window>,
            imgui_overlay::PerfOverlay,
            Box<dyn present::Present>,
        );
        let force_soft = std::env::var("CLIENT_PRESENT").is_ok_and(|v| v == "soft");
        let mut chosen: Option<Chosen> = None;
        if !force_soft {
            match present::gl::GlPresent::create(event_loop, attrs.clone()) {
                Ok((window, mut gl)) => {
                    let mut overlay =
                        imgui_overlay::PerfOverlay::new(window.scale_factor() as f32);
                    gl.attach_overlay_fonts(&mut overlay);
                    chosen = Some((window, overlay, Box::new(gl)));
                }
                Err(e) => {
                    eprintln!("[present] GL unavailable ({e}); falling back to softbuffer");
                }
            }
        }
        #[cfg(target_arch = "wasm32")]
        let (window, overlay, present) = chosen.expect("WebGL2 required in the browser");
        #[cfg(not(target_arch = "wasm32"))]
        let (window, overlay, present) = chosen.unwrap_or_else(|| {
            let w = std::rc::Rc::new(event_loop.create_window(attrs).expect("create_window"));
            let soft = present::soft::SoftPresent::new(w.clone()).expect("softbuffer present");
            let o = imgui_overlay::PerfOverlay::new(w.scale_factor() as f32);
            (w, o, Box::new(soft) as Box<dyn present::Present>)
        });
        eprintln!("[present] backend: {}", present.name());
        self.overlay = Some(overlay);
        self.window = Some(window);
        self.present = Some(present);

        // Client.init -> startCommon -> SignLink.threadreq(this, 1) -> run().
        if !self.inited {
            self.client.init();
            self.client.maininit();
            self.inited = true;
        }

        event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + FRAME_INTERVAL));
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                // GameShell.windowClosing -> destroy() -> @ObfuscatedName("dj.u(I)V") shutdown()
                self.client.mainquit();
                self.client.on_killed();
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                // The game keeps drawing into the fixed 765x503 frame
                // (@ObfuscatedName("dj.g(I)V") addcanvas pins the AWT canvas
                // to (sWid, sHei)); only the presented stretch changes, so a
                // repaint of the whole frame is all that's needed.
                if let Some(p) = self.present.as_mut() {
                    p.resize(size.width, size.height);
                }
                let mut shell = SHELL.lock().unwrap();
                shell.fullredraw = true;
            }
            WindowEvent::Focused(focused) => {
                // GameShell.focusGained / focusLost — no @ObfuscatedName (FocusListener overrides).
                let mut shell = SHELL.lock().unwrap();
                shell.focus_in = focused;
                if focused {
                    shell.fullredraw = true;
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                // Raw window-space cursor for the imgui overlay (drawn at
                // native window resolution, not game resolution).
                self.ui_mouse = (position.x as f32, position.y as f32);
                // Map window coords back into the fixed 765x503 game space —
                // the presented image is stretched to the window, so the
                // inverse scale puts the cursor where the game thinks it is.
                let (new_x, new_y) = match self.window.as_ref() {
                    Some(w) => {
                        let size = w.inner_size();
                        (
                            (position.x * WIDTH as f64 / size.width.max(1) as f64) as i32,
                            (position.y * HEIGHT as f64 / size.height.max(1) as f64) as i32,
                        )
                    }
                    None => (position.x as i32, position.y as i32),
                };
                let mut m = crate::input::MOUSE.lock().unwrap();
                if m.middle_down {
                    m.drag_delta_x += new_x - m.last_middle_x;
                    m.drag_delta_y += new_y - m.last_middle_y;
                    m.last_middle_x = new_x;
                    m.last_middle_y = new_y;
                }
                m.mouse_x = new_x;
                m.mouse_y = new_y;
                drop(m);
                // MouseTracking daemon sample (anti-bot op-72 feed).
                crate::input::mouse_tracking::sample(new_x, new_y);
            }
            WindowEvent::MouseInput { state, button, .. } => {
                match button {
                    MouseButton::Left => self.ui_buttons.0 = state == ElementState::Pressed,
                    MouseButton::Right => self.ui_buttons.1 = state == ElementState::Pressed,
                    _ => {}
                }
                // While the cursor is over (or dragging) the imgui overlay,
                // the click belongs to it — don't also feed the game.
                if self.overlay.as_ref().is_some_and(|o| o.want_mouse) {
                    return;
                }
                let mut m = crate::input::MOUSE.lock().unwrap();
                match button {
                    MouseButton::Middle => {
                        m.middle_down = state == ElementState::Pressed;
                        if m.middle_down {
                            m.last_middle_x = m.mouse_x;
                            m.last_middle_y = m.mouse_y;
                        }
                    }
                    _ => {
                        let id = match button {
                            MouseButton::Left => 1,
                            MouseButton::Right => 2,
                            _ => 0,
                        };
                        if state == ElementState::Pressed {
                            m.mouse_click_button = id;
                            m.mouse_click_x = m.mouse_x;
                            m.mouse_click_y = m.mouse_y;
                            m.mouse_button = id;
                            m.mouse_click_time = crate::host::SystemTime::now()
                                .duration_since(crate::host::UNIX_EPOCH)
                                .map(|d| d.as_millis() as i64)
                                .unwrap_or(0);
                        } else if m.mouse_button == id {
                            m.mouse_button = 0;
                        }
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                use winit::event::MouseScrollDelta;
                let dy = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y as i32,
                    MouseScrollDelta::PixelDelta(p) => (p.y / 20.0) as i32,
                };
                let mut m = crate::input::MOUSE.lock().unwrap();
                m.scroll_delta += dy;
            }
            WindowEvent::KeyboardInput { event, .. } => {
                use crate::input::*;
                let pressed = event.state == ElementState::Pressed;
                // Java's KEY_CODE_MAP collapses AWT VK_* into a tiny
                // gamepack set. We translate winit keys directly.
                let (code, ch) = match &event.logical_key {
                    Key::Named(NamedKey::Backspace) => (KEY_BACKSPACE, '\0'),
                    Key::Named(NamedKey::Tab) => (KEY_TAB, '\0'),
                    Key::Named(NamedKey::Enter) => (KEY_ENTER, '\0'),
                    Key::Named(NamedKey::ArrowLeft) => (KEY_LEFT, '\0'),
                    Key::Named(NamedKey::ArrowRight) => (KEY_RIGHT, '\0'),
                    Key::Named(NamedKey::ArrowUp) => (KEY_UP, '\0'),
                    Key::Named(NamedKey::ArrowDown) => (KEY_DOWN, '\0'),
                    Key::Character(s) => {
                        let c = s.chars().next().unwrap_or('\0');
                        (0, c)
                    }
                    _ => (0, '\0'),
                };
                if code > 0 {
                    let mut kb = KEYBOARD.lock().unwrap();
                    if (code as usize) < kb.key_held.len() {
                        kb.key_held[code as usize] = pressed;
                    }
                }
                if pressed && (code != 0 || ch != '\0') {
                    let mut kb = KEYBOARD.lock().unwrap();
                    kb.queue.push_back(KeyEvt { code, ch });
                }
            }
            WindowEvent::RedrawRequested => {
                self.redraw();
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // GameShell.run — `while (killtime == 0L || ... < killtime) { for each update mainloopwrapper(); mainredrawwrapper(); }`
        let now = Instant::now();
        if now >= self.next_tick {
            // @ObfuscatedName("dj.i(I)V") mainloopwrapper -> mainloop
            {
                let _t = perf::scope(perf::Scope::Logic);
                self.client.mainloop();
            }
            // @ObfuscatedName("dj.s(I)V") mainredrawwrapper -> mainredraw (we trigger via RedrawRequested)
            if let Some(w) = &self.window {
                w.request_redraw();
            }
            self.next_tick = now + FRAME_INTERVAL;
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_tick));
    }
}

impl App {
    fn redraw(&mut self) {
        let Some(window) = self.window.as_ref() else { return };
        let size = window.inner_size();

        {
            let mut shell = SHELL.lock().unwrap();
            // Java GameShell.mainredrawwrapper (GameShell.java:330-338):
            // stamp the draw-time ring and derive fps from the elapsed ms
            // across the last 32 frames — `((dt>>1)+32000)/dt` is
            // 32000/dt (32 frames × 1000 ms) with round-to-nearest.
            let now = crate::game_shell::monotonic_ms();
            let pos = shell.draw_pos as usize;
            let prev = shell.draw_time[pos];
            shell.draw_time[pos] = now;
            shell.draw_pos = (shell.draw_pos + 1) & 0x1F;
            if prev != 0 && now > prev {
                let dt = (now - prev) as i32;
                shell.fps = ((dt >> 1) + 32000) / dt;
                // Average ms/frame across the 32-frame window — handy as a
                // smoother companion to the integer fps.
                shell.frame_ms = dt / 32;
            }
        }

        // The game always draws into the fixed 765x503 frame; the Present
        // backend stretches it to the window and draws the perf overlay on
        // top at native resolution.
        let mut fb = Framebuffer::new(&mut self.frame, WIDTH as i32, HEIGHT as i32);
        self.client.mainredraw(&mut fb);

        if let (Some(p), Some(overlay)) = (self.present.as_mut(), self.overlay.as_mut()) {
            p.present(
                &self.frame,
                WIDTH,
                HEIGHT,
                size.width,
                size.height,
                overlay,
                self.ui_mouse,
                self.ui_buttons,
            );
        }
        perf::end_frame();
    }
}

fn main() {
    // Browser: panics land in the console; winit drives the loop off
    // requestAnimationFrame, so spawn_app (run_app can't block on wasm).
    #[cfg(target_arch = "wasm32")]
    {
        console_error_panic_hook::set_once();
        use winit::platform::web::EventLoopExtWebSys;
        let event_loop = EventLoop::new().expect("event loop");
        event_loop.set_control_flow(ControlFlow::Wait);
        event_loop.spawn_app(App::new());
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        let event_loop = EventLoop::new().expect("event loop");
        event_loop.set_control_flow(ControlFlow::Wait);
        let mut app = App::new();
        event_loop.run_app(&mut app).expect("run_app");
    }
}
