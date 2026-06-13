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
use crate::{imgui_overlay, perf, present};

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
use crate::client::Client;
use crate::game_shell::{Framebuffer, GameShellLifecycle, SHELL};

// The ::fpson overlay's "Mem:Nk" line reads live heap usage (Java asks the
// GC runtime; we count allocations instead). See perf::CountingAllocator.

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
                } else {
                    // ClientKeyboardListener.focusLost clears the held-key
                    // bitmap; ClientMouseListener.focusLost clears the held
                    // button — alt-tabbing away must not leave keys stuck.
                    crate::input::KEYBOARD.lock().unwrap().focus_lost();
                    let mut m = crate::input::MOUSE.lock().unwrap();
                    m.mouse_button = 0;
                    m.middle_down = false;
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                // Raw window-space cursor for the imgui overlay (drawn at
                // native window resolution, not game resolution).
                self.ui_mouse = (position.x as f32, position.y as f32);
                // Map window coords back into the fixed 765x503 game space
                // through the SAME layout rect the present backends draw
                // with — 1:1 minus the top-centre offset in vanilla layout,
                // an inverse scale in stretched mode.
                let (new_x, new_y) = match self.window.as_ref() {
                    Some(w) => {
                        let size = w.inner_size();
                        let (dx, dy, dw, dh) =
                            present::layout_rect(WIDTH, HEIGHT, size.width, size.height);
                        (
                            ((position.x - dx as f64) * WIDTH as f64 / dw.max(1) as f64) as i32,
                            ((position.y - dy as f64) * HEIGHT as f64 / dh.max(1) as f64) as i32,
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
                            // Java: MonotonicTime.currentTime() — a steady
                            // clock, not wall time (op-161 click deltas).
                            m.mouse_click_time = crate::game_shell::monotonic_ms();
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
                // Mirror AWT exactly: keyPressed maps the VK through
                // KEY_CODE_MAP and queues a code entry; keyTyped queues a
                // separate Cp1252-filtered char entry; keyReleased clears
                // the held bitmap (with the 0x80 numpad unmasking quirk).
                // Repeats flow through like AWT's auto-repeat events.
                let vk = awt_vk(&event);
                let mut kb = crate::input::KEYBOARD.lock().unwrap();
                if event.state == ElementState::Pressed {
                    if vk >= 0 {
                        kb.press(vk);
                    }
                    if let Some(text) = event.text.as_ref() {
                        for ch in text.chars() {
                            // AWT reports Enter's typed char as '\n'.
                            kb.typed(if ch == '\r' { '\n' } else { ch });
                        }
                    } else if event.logical_key == Key::Named(NamedKey::Escape) {
                        // AWT fires keyTyped(0x1B) for escape; winit has
                        // no text for it.
                        kb.typed('\u{1b}');
                    }
                } else if vk >= 0 {
                    kb.release(vk);
                }
            }
            WindowEvent::RedrawRequested => {
                self.redraw();
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // GameShell.run — `while (...) { for each update mainloopwrapper(); mainredrawwrapper(); }`
        // Java runs MULTIPLE logic updates per cycle when the timer slipped,
        // keeping the long-run cadence at exactly 50Hz. That catch-up
        // matters especially on web, where setTimeout fires at-or-after the
        // deadline and redraws snap to the rAF grid — rescheduling from the
        // late wake time would leak ~5ms per frame (50 → 40 fps).
        let now = Instant::now();
        if now >= self.next_tick {
            let mut updates = 0;
            while now >= self.next_tick && updates < 5 {
                // @ObfuscatedName("dj.i(I)V") mainloopwrapper -> mainloop
                {
                    let _t = perf::scope(perf::Scope::Logic);
                    self.client.mainloop();
                }
                self.next_tick += FRAME_INTERVAL;
                updates += 1;
            }
            if self.next_tick <= now {
                // Hopelessly behind (tab in background, breakpoint) —
                // resync rather than burst-spin to catch up.
                self.next_tick = now + FRAME_INTERVAL;
            }
            // @ObfuscatedName("dj.s(I)V") mainredrawwrapper -> mainredraw (we trigger via RedrawRequested)
            if let Some(w) = &self.window {
                w.request_redraw();
            }
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

        // Debug ground truth: CLIENT_FRAME_DUMP=path dumps the finished CPU
        // frame every 300 redraws — verifies mainredraw output without any
        // window/GL/focus dependency.
        if let Ok(path) = std::env::var("CLIENT_FRAME_DUMP") {
            use std::sync::atomic::{AtomicU32, Ordering};
            static FRAMES: AtomicU32 = AtomicU32::new(0);
            if FRAMES.fetch_add(1, Ordering::Relaxed) % 300 == 150 {
                let rgb: Vec<u8> = self.frame.iter()
                    .flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8])
                    .collect();
                let _ = image::save_buffer(&path, &rgb, WIDTH, HEIGHT, image::ColorType::Rgb8);
                eprintln!("[framedump] wrote {path}");
            }
        }
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

// AWT VK_* code for a winit key event — feeds input::key_code_map, which
// is the verbatim ClientKeyboardListener.KEY_CODE_MAP table. Numpad comes
// from the physical key (the logical key collapses to plain digits).
// Limitation: shifted punctuation (e.g. Shift+1 → '!') has no unshifted VK
// here, so its keyPressed code entry is missed — keyTyped still delivers
// the char, which is all the gamepack consumes for those keys.
fn awt_vk(event: &winit::event::KeyEvent) -> i32 {
    use winit::keyboard::KeyCode;
    if let PhysicalKey::Code(code) = event.physical_key {
        let vk = match code {
            KeyCode::Numpad0 => 96,
            KeyCode::Numpad1 => 97,
            KeyCode::Numpad2 => 98,
            KeyCode::Numpad3 => 99,
            KeyCode::Numpad4 => 100,
            KeyCode::Numpad5 => 101,
            KeyCode::Numpad6 => 102,
            KeyCode::Numpad7 => 103,
            KeyCode::Numpad8 => 104,
            KeyCode::Numpad9 => 105,
            KeyCode::NumpadMultiply => 106,
            KeyCode::NumpadAdd => 107,
            KeyCode::NumpadSubtract => 109,
            KeyCode::NumpadDecimal => 110,
            KeyCode::NumpadDivide => 111,
            _ => -1,
        };
        if vk != -1 {
            return vk;
        }
    }
    match &event.logical_key {
        Key::Named(named) => match named {
            NamedKey::Backspace => 8,
            NamedKey::Tab => 9,
            NamedKey::Enter => 10,
            NamedKey::Shift => 16,
            NamedKey::Control => 17,
            NamedKey::Alt => 18,
            NamedKey::Escape => 27,
            NamedKey::Space => 32,
            NamedKey::PageUp => 33,
            NamedKey::PageDown => 34,
            NamedKey::End => 35,
            NamedKey::Home => 36,
            NamedKey::ArrowLeft => 37,
            NamedKey::ArrowUp => 38,
            NamedKey::ArrowRight => 39,
            NamedKey::ArrowDown => 40,
            NamedKey::Delete => 127,
            NamedKey::Insert => 155,
            NamedKey::F1 => 112,
            NamedKey::F2 => 113,
            NamedKey::F3 => 114,
            NamedKey::F4 => 115,
            NamedKey::F5 => 116,
            NamedKey::F6 => 117,
            NamedKey::F7 => 118,
            NamedKey::F8 => 119,
            NamedKey::F9 => 120,
            NamedKey::F10 => 121,
            NamedKey::F11 => 122,
            NamedKey::F12 => 123,
            _ => -1,
        },
        Key::Character(s) => {
            match s.chars().next().unwrap_or('\0').to_ascii_uppercase() {
                c @ ('A'..='Z' | '0'..='9') => c as i32,
                ',' => 44,
                '-' => 45,
                '.' => 46,
                '/' => 47,
                ';' => 59,
                '=' => 61,
                '[' => 91,
                '\\' => 92,
                ']' => 93,
                '`' => 192,
                '\'' => 222,
                _ => -1,
            }
        }
        _ => -1,
    }
}

pub fn run() {
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
