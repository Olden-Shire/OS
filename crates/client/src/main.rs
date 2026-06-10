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

use std::num::NonZeroU32;
use std::time::{Duration, Instant};

use softbuffer::{Context, Surface};
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::keyboard::{Key, NamedKey, PhysicalKey};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

mod applet;
mod client;
mod client_build;
mod config;
mod dash3d;
mod datastruct;
mod game_canvas;
mod game_shell;
mod graphics;
mod input;
mod interface_render;
mod io;
mod client_inv_cache;
mod friend;
mod js5;
mod jag_exception;
mod javconfig;
mod jstring;
mod login;
mod midi2;
mod minimap;
mod namespace;
mod obfuscation;
mod overlays;
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
mod world_entry;

use client::Client;
use game_shell::{Framebuffer, GameShellLifecycle, SHELL};

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
    surface: Option<Surface<std::rc::Rc<Window>, std::rc::Rc<Window>>>,
    _context: Option<Context<std::rc::Rc<Window>>>,
    next_tick: Instant,
    inited: bool,
}

impl App {
    fn new() -> Self {
        let mut client = Client::new();
        // app.startApplication(765, 503, 1);
        client.start_application(WIDTH, HEIGHT, REVISION);
        Self {
            client,
            window: None,
            surface: None,
            _context: None,
            next_tick: Instant::now(),
            inited: false,
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
            .with_resizable(false);
        let window = std::rc::Rc::new(event_loop.create_window(attrs).expect("create_window"));
        let context = Context::new(window.clone()).expect("softbuffer context");
        let surface = Surface::new(&context, window.clone()).expect("softbuffer surface");
        self.window = Some(window);
        self._context = Some(context);
        self.surface = Some(surface);

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
                // @ObfuscatedName("dj.g(I)V") addcanvas — resizes the AWT canvas to (sWid, sHei).
                let mut shell = SHELL.lock().unwrap();
                shell.s_wid = size.width.max(1);
                shell.s_hei = size.height.max(1);
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
                let mut m = crate::input::MOUSE.lock().unwrap();
                let new_x = position.x as i32;
                let new_y = position.y as i32;
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
                            m.mouse_click_time = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
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
            self.client.mainloop();
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
        let Some(surface) = self.surface.as_mut() else { return };
        let Some(window) = self.window.as_ref() else { return };
        let size = window.inner_size();
        let (Some(w_nz), Some(h_nz)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
        else {
            return;
        };
        if surface.resize(w_nz, h_nz).is_err() {
            return;
        }
        let Ok(mut buffer) = surface.buffer_mut() else {
            return;
        };

        {
            let mut shell = SHELL.lock().unwrap();
            shell.s_wid = size.width;
            shell.s_hei = size.height;
        }

        let mut fb = Framebuffer::new(&mut buffer, size.width as i32, size.height as i32);
        self.client.mainredraw(&mut fb);

        let _ = buffer.present();
    }
}

fn main() {
    let event_loop = EventLoop::new().expect("event loop");
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = App::new();
    event_loop.run_app(&mut app).expect("run_app");
}
