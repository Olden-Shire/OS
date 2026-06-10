// jagex3.client.input — ClientMouseListener (@ObfuscatedName "av") +
// ClientKeyboardListener (@ObfuscatedName "az") drained into single
// global state structs that mainloop reads each tick.

#![allow(dead_code)]

pub mod mouse_tracking;
pub mod mouse_wheel;

use std::collections::VecDeque;
use std::sync::Mutex;

pub struct MouseState {
    // @ObfuscatedName("av.r") — ClientMouseListener.mouseClickButton
    pub mouse_click_button: i32,
    // @ObfuscatedName("av.d") — ClientMouseListener.mouseClickX
    pub mouse_click_x: i32,
    // @ObfuscatedName("av.l") — ClientMouseListener.mouseClickY
    pub mouse_click_y: i32,
    // @ObfuscatedName("av.m") — ClientMouseListener.mouseX
    pub mouse_x: i32,
    // @ObfuscatedName("av.c") — ClientMouseListener.mouseY
    pub mouse_y: i32,
    // @ObfuscatedName("av.n") — ClientMouseListener.mouseButton: the
    // CURRENTLY-HELD button (0 none, 1 left, 2 right), distinct from
    // the once-per-cycle click event above.
    pub mouse_button: i32,
    // @ObfuscatedName("av.j") — ClientMouseListener.mouseClickTime,
    // unix millis of the last press (feeds the op-161 time delta).
    pub mouse_click_time: i64,
    // custom — middle-button drag state for camera rotation. The Java
    // gamepack relies on the applet host's mouse wheel handler (no
    // analogue lives on ClientMouseListener) so these are entirely our
    // addition.
    pub middle_down: bool,
    pub last_middle_x: i32,
    pub last_middle_y: i32,
    pub scroll_delta: i32,
    pub drag_delta_x: i32,
    pub drag_delta_y: i32,
}

impl MouseState {
    pub const fn new() -> Self {
        Self {
            mouse_click_button: 0, mouse_click_x: 0, mouse_click_y: 0,
            mouse_x: 0, mouse_y: 0,
            mouse_button: 0,
            mouse_click_time: 0,
            middle_down: false,
            last_middle_x: 0, last_middle_y: 0,
            scroll_delta: 0,
            drag_delta_x: 0, drag_delta_y: 0,
        }
    }
    pub fn consume_click(&mut self) {
        self.mouse_click_button = 0;
    }

    // Pure click-state predicates. mouseClickButton is the once-per-
    // cycle click event (consumed by consume_click); the Rust port
    // doesn't track held-button state separately yet, so is_left_down
    // / is_right_down land with the input-event layer rewrite.
    pub fn is_left_click(&self) -> bool { self.mouse_click_button == 1 }
    pub fn is_right_click(&self) -> bool { self.mouse_click_button == 2 }
    pub fn take_scroll(&mut self) -> i32 {
        let d = self.scroll_delta;
        self.scroll_delta = 0;
        d
    }
    pub fn take_drag(&mut self) -> (i32, i32) {
        let d = (self.drag_delta_x, self.drag_delta_y);
        self.drag_delta_x = 0;
        self.drag_delta_y = 0;
        d
    }
}

pub static MOUSE: Mutex<MouseState> = Mutex::new(MouseState::new());

pub struct KeyEvt {
    // @ObfuscatedName("n.cm") — gamepack code (see KEY_CODE_MAP)
    pub code: i32,
    // @ObfuscatedName("ca.cc") — Java char (0 if non-printable)
    pub ch: char,
}

pub struct KeyboardState {
    pub queue: VecDeque<KeyEvt>,
    // Held-key bitmap mirroring `az.cu`.
    pub key_held: [bool; 112],
}

impl KeyboardState {
    pub const fn new() -> Self {
        Self { queue: VecDeque::new(), key_held: [false; 112] }
    }

    // @ObfuscatedName("eq.q(B)Z") — ClientKeyboardListener.pollKey
    pub fn poll_key(&mut self) -> Option<KeyEvt> {
        self.queue.pop_front()
    }

    // Bounds-checked accessor for the key_held bitmap. Java exposes
    // the 112-entry array directly; the gamepack reads
    // `ClientKeyboardListener.keyHeld[KEY_X]` with the implicit array
    // bounds check. Mirrors that safely.
    pub fn is_key_held(&self, code: i32) -> bool {
        if code < 0 || code >= 112 { return false; }
        self.key_held[code as usize]
    }
}

// Pure chat-input predicate. Mirrors Java's `keyTyped` filter at
// ClientKeyboardListener.java:203-217: only Cp1252-encodable
// printable chars are accepted into the chat input buffer.
pub fn is_chat_char(ch: char) -> bool {
    ch != '\0' && ch != '\u{FFFF}' && crate::jstring::can_encode_to_cp1252(ch)
}

pub static KEYBOARD: Mutex<KeyboardState> = Mutex::new(KeyboardState::new());

// Gamepack key codes referenced by TitleScreen / login form:
pub const KEY_BACKSPACE: i32 = 85;
pub const KEY_TAB: i32 = 80;
pub const KEY_ENTER: i32 = 84;
// Java's gamepack camera key codes (see Client.java orbitCameraYaw input).
pub const KEY_LEFT: i32 = 96;
pub const KEY_RIGHT: i32 = 97;
pub const KEY_UP: i32 = 98;
pub const KEY_DOWN: i32 = 99;
