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
    // @ObfuscatedName("n.cm") — gamepack code (see key_code_map). Java's
    // queue holds EITHER a keyPressed entry (code, ch=0) OR a keyTyped
    // entry (code=-1, ch) — never both in one slot.
    pub code: i32,
    // @ObfuscatedName("ca.cc") — Java char (0 if non-printable)
    pub ch: char,
}

// @ObfuscatedName("az.cn") — ClientKeyboardListener.KEY_CODE_MAP: AWT
// VK_* → gamepack key code (-1 = unmapped). Includes setupKeyCodeMap's
// non-Microsoft-VM punctuation entries (we are never the MS VM). Codes
// with bit 0x80 set (numpad) are masked out of keyPressed but their
// & 0x7F value still clears keyHeld on release — verbatim Java quirk.
pub fn key_code_map(vk: i32) -> i32 {
    match vk {
        8 => 85,    // backspace
        9 => 80,    // tab
        10 => 84,   // enter
        12 => 91,   // clear
        16 => 81,   // shift
        17 => 82,   // ctrl
        18 => 86,   // alt
        27 => 13,   // escape
        32 => 83,   // space
        33 => 104,  // page up
        34 => 105,  // page down
        35 => 103,  // end
        36 => 102,  // home
        37 => 96,   // left
        38 => 98,   // up
        39 => 97,   // right
        40 => 99,   // down
        48 => 25,   // 0
        49..=57 => 16 + (vk - 49), // 1-9 → 16..24
        // setupKeyCodeMap (non-Microsoft branch)
        44 => 71,   // comma
        45 => 26,   // minus
        46 => 72,   // period
        47 => 73,   // slash
        59 => 57,   // semicolon
        61 => 27,   // equals
        91 => 42,   // open bracket
        92 => 74,   // backslash
        93 => 43,   // close bracket
        192 => 28,  // back quote
        222 => 58,  // quote
        520 => 59,  // AWT VK 520 (inverted exclamation on some layouts)
        // letters A-Z
        65 => 48, 66 => 68, 67 => 66, 68 => 50, 69 => 34, 70 => 51,
        71 => 52, 72 => 53, 73 => 39, 74 => 54, 75 => 55, 76 => 56,
        77 => 70, 78 => 69, 79 => 40, 80 => 41, 81 => 32, 82 => 35,
        83 => 49, 84 => 36, 85 => 38, 86 => 67, 87 => 33, 88 => 65,
        89 => 37, 90 => 64,
        // numpad 0-9 (codes carry bit 0x80 — see masking quirk above)
        96 => 228, 97 => 231, 98 => 227, 99 => 233, 100 => 224,
        101 => 219, 102 => 225, 103 => 230, 104 => 226, 105 => 232,
        106 => 89,  // numpad *
        107 => 87,  // numpad +
        109 => 88,  // numpad -
        110 => 229, // numpad .
        111 => 90,  // numpad /
        112..=123 => vk - 111, // F1-F12 → 1..12
        127 => 101, // delete
        155 => 100, // insert
        _ => -1,
    }
}

pub struct KeyboardState {
    pub queue: VecDeque<KeyEvt>,
    // Held-key bitmap mirroring `az.cu`. Java defers updates through a
    // ring drained by cycle() because AWT events land on another thread;
    // our events and mainloop share the winit thread, so direct writes
    // are tick-equivalent.
    pub key_held: [bool; 112],
    // @ObfuscatedName("az.cx") — idleTimer: ticks since the last key
    // event (cycle() increments, events zero it).
    pub idle_timer: i32,
}

impl KeyboardState {
    pub const fn new() -> Self {
        Self { queue: VecDeque::new(), key_held: [false; 112], idle_timer: 0 }
    }

    // ClientKeyboardListener.keyPressed — map the AWT VK, mask out
    // 0x80-flagged (numpad) codes, set held, and enqueue a code entry.
    // The Java queue is a 128-slot ring that silently drops when full.
    pub fn press(&mut self, vk: i32) {
        self.idle_timer = 0;
        let mut code = key_code_map(vk);
        if code & 0x80 != 0 {
            code = -1;
        }
        if code >= 0 {
            if (code as usize) < self.key_held.len() {
                self.key_held[code as usize] = true;
            }
            if self.queue.len() < 127 {
                self.queue.push_back(KeyEvt { code, ch: '\0' });
            }
        }
    }

    // ClientKeyboardListener.keyReleased — `KEY_CODE_MAP[vk] & 0xFFFFFF7F`:
    // clears bit 7 so numpad codes land back in the 112-entry bitmap;
    // unmapped (-1) stays negative and is skipped.
    pub fn release(&mut self, vk: i32) {
        self.idle_timer = 0;
        let code = ((key_code_map(vk) as u32) & 0xFFFF_FF7F) as i32;
        if (0..112).contains(&code) {
            self.key_held[code as usize] = false;
        }
    }

    // ClientKeyboardListener.keyTyped — Cp1252-encodable chars only,
    // queued as a char entry (code -1).
    pub fn typed(&mut self, ch: char) {
        if is_chat_char(ch) && self.queue.len() < 127 {
            self.queue.push_back(KeyEvt { code: -1, ch });
        }
    }

    // ClientKeyboardListener.focusLost — keyHeldReadPos = -1 makes the
    // next cycle() clear the whole held bitmap.
    pub fn focus_lost(&mut self) {
        self.key_held = [false; 112];
    }

    // ClientKeyboardListener.cycle — idleTimer accrual (the queue
    // read-cursor swap is subsumed by poll_key's direct pops).
    pub fn cycle(&mut self) {
        self.idle_timer += 1;
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
