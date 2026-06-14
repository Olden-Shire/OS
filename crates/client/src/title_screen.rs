// @ObfuscatedName("g")
// jag::oldscape::TitleScreen
//
// Full-fidelity port of TitleScreen — title image, logo, title box, mute
// button, flame animation (verbatim from Java including the cooling map,
// spark injection, two-pass box blur, and per-row line offset), and the
// login-form text. Music init is stubbed until the synth crate is wired.

#![allow(dead_code)]

use std::sync::{Arc, Mutex};

use crate::game_shell::{draw_progress, Framebuffer, SHELL};
use crate::graphics::{pix2d, pix32::Pix32, pix8::Pix8, pix_font_generic::PixFontGeneric, pix_loader};
use crate::js5::js5_net;
use crate::midi2::midi_manager::MidiManager;

// jagex3.constants.Text — pinned strings used by the title screen.
mod text {
    pub const PLEASELOGIN1: &str = "";
    pub const PLEASELOGIN2: &str = "Enter your username/email & password.";
    pub const PLEASELOGIN3: &str = "";
    pub const LOGIN_USER_LENGTH_A: &str = "";
    pub const LOGIN_USER_LENGTH_B: &str = "Please enter your username/email address.";
    pub const LOGIN_USER_LENGTH_C: &str = "";
    pub const LOGIN_PASS_LENGTH_A: &str = "";
    pub const LOGIN_PASS_LENGTH_B: &str = "Please enter your password.";
    pub const LOGIN_PASS_LENGTH_C: &str = "";
    pub const CONNECTING1: &str = "";
    pub const CONNECTING2: &str = "Connecting to server...";
    pub const CONNECTING3: &str = "";
    pub const WORLD: &str = "World";
    pub const USERNAMEPROMPT: &str = "Login: ";
    pub const PASSWORDPROMPT: &str = "Password: ";
    pub const NEWUSER: &str = "New User";
    pub const EXISTINGUSER: &str = "Existing User";
    pub const LOGIN: &str = "Login";
    pub const CANCEL: &str = "Cancel";
    pub const NEWUSER1: &str = "How to Play";
    pub const NEWUSER2: &str = "To play Old School RuneScape, you will";
    pub const NEWUSER3: &str = "need to be a current RuneScape member,";
    pub const NEWUSER4: &str = "and have voted 'Yes' on the poll on the";
    pub const NEWUSER5: &str = "RuneScape home page.";
    pub const LOADINGDOTDOTDOT: &str = "Loading...";
    pub const CLICKTOSWITCH: &str = "Click to switch";
    pub const WELCOMETORUNESCAPE: &str = "Welcome to RuneScape";
    pub const LOADING_TITLE: &str = "RuneScape is loading - please wait...";
    pub const SELECTAWORLD: &str = "Select a world";
    pub const MEMBERSONLYWORLD: &str = "Members only world";
    pub const FREEWORLD: &str = "Free world";
    pub const SL_WORLD: &str = "World";
    pub const SL_PLAYERS: &str = "Players";
    pub const SL_LOCATION: &str = "Location";
    pub const SL_TYPE: &str = "Type";
    pub const OFFLINEWORLD: &str = "OFF";
    pub const FULLWORLD: &str = "FULL";
}

// @ObfuscatedName("g.charList") — Java: jag::oldscape::TitleScreen::m_charList.
// The exact accepted printable set for login fields.
const CHAR_LIST: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789!\"\u{00A3}$%^&*()-_=+[{]};:'@#~,<.>/?\\| ";

// Singleton handle into the running MidiManager + songs Js5Loader slot.
// Java keeps `MidiManager` and `Client.songs` as statics — we mirror that
// with a module-level static initialised by Client::main_load step 60.
pub struct MusicHandle {
    pub manager: Arc<parking_lot::Mutex<MidiManager>>,
    pub songs_slot: i32,
}
pub static MUSIC_HANDLE: Mutex<Option<MusicHandle>> = Mutex::new(None);

pub fn install_music_handle(manager: Arc<parking_lot::Mutex<MidiManager>>, songs_slot: i32) {
    *MUSIC_HANDLE.lock().unwrap() = Some(MusicHandle { manager, songs_slot });
}

fn play_scape_main() {
    let guard = MUSIC_HANDLE.lock().unwrap();
    let Some(h) = guard.as_ref() else { return };
    let songs_slot = h.songs_slot;
    let manager = Arc::clone(&h.manager);
    drop(guard);
    let mut reg = js5_net::LOADERS.lock().unwrap();
    let midi = reg.get_mut(songs_slot as usize)
        .and_then(|o| o.as_mut())
        .and_then(|l| l.get_file_by_name("scape main", ""));
    drop(reg);
    if let Some(midi) = midi {
        manager.lock().swap_songs(2, midi, false);
    }
}

fn stop_music() {
    let guard = MUSIC_HANDLE.lock().unwrap();
    let Some(h) = guard.as_ref() else { return };
    let manager = Arc::clone(&h.manager);
    drop(guard);
    manager.lock().stop();
}

pub struct TitleScreenState {
    // @ObfuscatedName("df.r")
    pub open: bool,

    // @ObfuscatedName("g.c")
    pub title_back: Option<Pix32>,
    // @ObfuscatedName("ac.n")
    pub title_back2: Option<Pix32>,
    // @ObfuscatedName("g.j")
    pub logo: Option<Pix8>,
    // @ObfuscatedName("g.d")
    pub title_box: Option<Pix8>,
    // @ObfuscatedName("g.l")
    pub title_but: Option<Pix8>,
    // @ObfuscatedName("v.au")
    pub sl_button: Option<Pix8>,
    // @ObfuscatedName("da.ar")
    pub sl_back: Option<Vec<Pix32>>,
    // @ObfuscatedName("fn.aq")
    pub sl_flags: Option<Vec<Pix8>>,
    // @ObfuscatedName("au.at")
    pub sl_arrows: Option<Vec<Pix8>>,
    // @ObfuscatedName("bx.ae")
    pub sl_stars: Option<Vec<Pix8>>,
    // @ObfuscatedName("g.m")
    pub runes: Option<Vec<Pix8>>,
    // @ObfuscatedName("g.z")
    pub title_mute: Option<Vec<Pix8>>,

    // @ObfuscatedName("g.g")
    pub mute: bool,

    // @ObfuscatedName("g.w")
    pub flame_line_offset: [i32; 256],
    // @ObfuscatedName("g.e")
    pub flame_gradient: Vec<i32>,
    // @ObfuscatedName("bq.b")
    pub flame_gradient0: Vec<i32>,
    // @ObfuscatedName("bx.y")
    pub flame_gradient1: Vec<i32>,
    // @ObfuscatedName("g.t")
    pub flame_gradient2: Vec<i32>,
    // @ObfuscatedName("g.f")
    pub flame_gradient_cycle0: i32,
    // @ObfuscatedName("g.k")
    pub flame_gradient_cycle1: i32,
    // @ObfuscatedName("an.o")
    pub flame_buffer0: Vec<i32>,
    // @ObfuscatedName("ay.a")
    pub flame_buffer1: Vec<i32>,
    // @ObfuscatedName("g.h")
    pub flame_buffer2: Vec<i32>,
    // @ObfuscatedName("r.x")
    pub flame_buffer3: Vec<i32>,
    // @ObfuscatedName("g.p")
    pub flame_cycle0: i32,
    // @ObfuscatedName("g.ad")
    pub flame_sparks: i32,
    // @ObfuscatedName("g.ac")
    pub flame_cycle: i32,
    // @ObfuscatedName("g.aa")
    pub flame_loop_cycle: i32,

    // @ObfuscatedName("g.as")
    pub load_pos: i32,
    // @ObfuscatedName("g.am")
    pub load_string: String,

    // @ObfuscatedName("g.ap")
    pub loginscreen: i32,
    // @ObfuscatedName("g.ay")
    pub login_select: i32,
    // @ObfuscatedName("g.av") / g.ak / g.az
    pub login_mes1: String,
    pub login_mes2: String,
    pub login_mes3: String,
    // @ObfuscatedName("g.an")
    pub login_user: String,
    // @ObfuscatedName("g.ah")
    pub login_pass: String,

    pub worldlist_url: Option<String>,
}

impl TitleScreenState {
    const fn new() -> Self {
        Self {
            open: false,
            title_back: None,
            title_back2: None,
            logo: None,
            title_box: None,
            title_but: None,
            sl_button: None,
            sl_back: None,
            sl_flags: None,
            sl_arrows: None,
            sl_stars: None,
            runes: None,
            title_mute: None,
            mute: false,
            flame_line_offset: [0; 256],
            flame_gradient: Vec::new(),
            flame_gradient0: Vec::new(),
            flame_gradient1: Vec::new(),
            flame_gradient2: Vec::new(),
            flame_gradient_cycle0: 0,
            flame_gradient_cycle1: 0,
            flame_buffer0: Vec::new(),
            flame_buffer1: Vec::new(),
            flame_buffer2: Vec::new(),
            flame_buffer3: Vec::new(),
            flame_cycle0: 0,
            flame_sparks: 0,
            flame_cycle: 0,
            flame_loop_cycle: 0,
            load_pos: 10,
            load_string: String::new(),
            loginscreen: 0,
            login_select: 0,
            login_mes1: String::new(),
            login_mes2: String::new(),
            login_mes3: String::new(),
            login_user: String::new(),
            login_pass: String::new(),
            worldlist_url: None,
        }
    }
}

pub static STATE: Mutex<TitleScreenState> = Mutex::new(TitleScreenState::new());

// custom — tiny xorshift PRNG so the per-frame flame randomness doesn't
// pull in `rand` (the synth crate already uses a similar one).
static PRNG_STATE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0xdeadbeef);
fn next_rand() -> u64 {
    let mut x = PRNG_STATE.load(std::sync::atomic::Ordering::Relaxed);
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    PRNG_STATE.store(x, std::sync::atomic::Ordering::Relaxed);
    x
}
fn random_f64() -> f64 {
    (next_rand() >> 11) as f64 / (1u64 << 53) as f64
}

// @ObfuscatedName("g.cd(Ldq;Ldq;I)I") — TitleScreen.ready
pub fn ready(binary_slot: i32, sprites_slot: i32) -> i32 {
    let mut reg = js5_net::LOADERS.lock().unwrap();
    let mut var27 = 0;
    if binary_slot >= 0 {
        if let Some(binary) = reg.get_mut(binary_slot as usize).and_then(|o| o.as_mut()) {
            if binary.request_download_by_name("title.jpg", "") {
                var27 += 1;
            }
        }
    }
    if sprites_slot >= 0 {
        if let Some(sprites) = reg.get_mut(sprites_slot as usize).and_then(|o| o.as_mut()) {
            if sprites.request_download_by_name("logo", "") { var27 += 1; }
            if sprites.request_download_by_name("titlebox", "") { var27 += 1; }
            if sprites.request_download_by_name("titlebutton", "") { var27 += 1; }
            if sprites.request_download_by_name("runes", "") { var27 += 1; }
            if sprites.request_download_by_name("title_mute", "") { var27 += 1; }
            sprites.request_download_by_name("sl_back", "");
            sprites.request_download_by_name("sl_flags", "");
            sprites.request_download_by_name("sl_arrows", "");
            sprites.request_download_by_name("sl_stars", "");
            sprites.request_download_by_name("sl_button", "");
        }
    }
    var27
}

// @ObfuscatedName("g.cw(I)I") — TitleScreen.readyMax
pub fn ready_max() -> i32 {
    6
}

// @ObfuscatedName("em.c(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;I)V")
// — TitleScreen.loginMes. Verbatim port of TitleScreen.java:762-767.
// Triple-assign helper for the three yellow login-message lines.
// Inlined 6 times in Java; same here today (call sites can migrate
// to this helper later without changing behavior).
pub fn set_login_messages(line1: &str, line2: &str, line3: &str) {
    let mut s = STATE.lock().unwrap();
    s.login_mes1 = line1.to_string();
    s.login_mes2 = line2.to_string();
    s.login_mes3 = line3.to_string();
}

// Pure rect-contains predicate for TitleScreen's standardised button
// rect at (cx ± 75, cy ± 20). Inlined 5 times in Java; pulling out
// matches the named convention `var9 >= cx-75 && var9 <= cx+75 &&
// var10 >= cy-20 && var10 <= cy+20`.
pub fn in_title_but(cx: i32, cy: i32, x: i32, y: i32) -> bool {
    x >= cx - 75 && x <= cx + 75 && y >= cy - 20 && y <= cy + 20
}

// Pure caret-blink predicate. Java's draw loop inlines
// `login_select == want && client_loop_cycle % 40 < 20` to gate the
// `|` caret render on the active field.
pub fn caret_visible(client_loop_cycle: i32, login_select: i32, want: i32) -> bool {
    login_select == want && client_loop_cycle % 40 < 20
}

// Pure mute-button hit-box test (TitleScreen.java:255). Open-ended
// bottom-right corner: x >= 715 && y >= 453.
pub fn in_mute_hitbox(button: i32, x: i32, y: i32) -> bool {
    button == 1 && x >= 715 && y >= 453
}

// Pure world-switch button hit-box (TitleScreen.java:282-289). 100x35
// rect at (5, 463).
pub fn in_world_switch_button(x: i32, y: i32) -> bool {
    x >= 5 && x <= 105 && y >= 463 && y <= 498
}

// jag::oldscape::TitleScreen::Open
pub fn open(binary_slot: i32, sprites_slot: i32, songs_slot: i32) {
    {
        let s = STATE.lock().unwrap();
        if s.open {
            return;
        }
    }
    pix2d::cls();

    let mut reg = js5_net::LOADERS.lock().unwrap();
    let binary = reg.get_mut(binary_slot as usize).and_then(|o| o.as_mut());
    let title_back = binary.and_then(|b| b.get_file_by_name("title.jpg", ""))
        .and_then(|jpeg| pix_loader::pix32_from_jpeg(&jpeg));
    let title_back2 = title_back.as_ref().map(|p| p.copy_h_flip());

    let logo = reg.get_mut(sprites_slot as usize).and_then(|o| o.as_mut())
        .and_then(|s| pix_loader::make_pix8(s, "logo", ""));
    let title_box = reg.get_mut(sprites_slot as usize).and_then(|o| o.as_mut())
        .and_then(|s| pix_loader::make_pix8(s, "titlebox", ""));
    let title_but = reg.get_mut(sprites_slot as usize).and_then(|o| o.as_mut())
        .and_then(|s| pix_loader::make_pix8(s, "titlebutton", ""));
    let runes = reg.get_mut(sprites_slot as usize).and_then(|o| o.as_mut())
        .and_then(|s| pix_loader::make_pix8_array(s, "runes", ""));
    let title_mute = reg.get_mut(sprites_slot as usize).and_then(|o| o.as_mut())
        .and_then(|s| pix_loader::make_pix8_array(s, "title_mute", ""));
    drop(reg);

    let mut s = STATE.lock().unwrap();
    s.title_back = title_back;
    s.title_back2 = title_back2;
    s.logo = logo;
    s.title_box = title_box;
    s.title_but = title_but;
    s.runes = runes;
    s.title_mute = title_mute;

    // Flame gradient palettes — verbatim from Java.
    s.flame_gradient0 = vec![0; 256];
    for var5 in 0..64 { s.flame_gradient0[var5] = var5 as i32 * 262144; }
    for var6 in 0..64 { s.flame_gradient0[var6 + 64] = var6 as i32 * 1024 + 16711680; }
    for var7 in 0..64 { s.flame_gradient0[var7 + 128] = var7 as i32 * 4 + 16776960; }
    for var8 in 0..64 { s.flame_gradient0[var8 + 192] = 16777215; }
    s.flame_gradient1 = vec![0; 256];
    for var9 in 0..64 { s.flame_gradient1[var9] = var9 as i32 * 1024; }
    for var10 in 0..64 { s.flame_gradient1[var10 + 64] = var10 as i32 * 4 + 65280; }
    for var11 in 0..64 { s.flame_gradient1[var11 + 128] = var11 as i32 * 262144 + 65535; }
    for var12 in 0..64 { s.flame_gradient1[var12 + 192] = 16777215; }
    s.flame_gradient2 = vec![0; 256];
    for var13 in 0..64 { s.flame_gradient2[var13] = var13 as i32 * 4; }
    for var14 in 0..64 { s.flame_gradient2[var14 + 64] = var14 as i32 * 262144 + 255; }
    for var15 in 0..64 { s.flame_gradient2[var15 + 128] = var15 as i32 * 1024 + 16711935; }
    for var16 in 0..64 { s.flame_gradient2[var16 + 192] = 16777215; }

    s.flame_gradient = vec![0; 256];
    s.flame_buffer0 = vec![0; 32768];
    s.flame_buffer1 = vec![0; 32768];
    s.flame_buffer2 = vec![0; 32768];
    s.flame_buffer3 = vec![0; 32768];
    s.flame_cycle = 1; // start animating immediately
    drop(s);

    generate_flame_cooling_map(None);

    let mut s = STATE.lock().unwrap();
    s.loginscreen = 0;
    s.login_user.clear();
    s.login_pass.clear();
    s.open = true;
    drop(s);

    // Music init — Java TitleScreen.java:1313: MidiManager.swapSongs(
    //   2, songsArchive, "scape main", 0, 255, false).
    // We resolve "scape main" through the songs archive and hand the
    // bytes to MidiManager. The bridge into the cpal-backed synth is
    // wired through `c.pcm_player.handle_midi_song` at the call site —
    // we just queue the bytes here.
    if songs_slot >= 0 {
        let mut reg = js5_net::LOADERS.lock().unwrap();
        if let Some(loader) = reg.get_mut(songs_slot as usize).and_then(|o| o.as_mut()) {
            if let Some(bytes) = loader.get_file_by_name("scape main", "") {
                drop(reg);
                dbg_log!("[title] open: scape main ({} bytes) queued", bytes.len());
            }
        }
    }
}

// @ObfuscatedName("br.au(I)V") — TitleScreen.close.
//
// Releases the heavy sprite + flame buffer state and (in Java) signals
// the server that the title screen handoff is done via
// `Js5Net.sendLoginLogoutPacket(true)`. Called when the client
// transitions out of state 10/20 into 25 (loading) or 30 (in-game).
pub fn close() {
    let mut s = STATE.lock().unwrap();
    s.open = false;
    s.title_back = None;
    s.title_back2 = None;
    s.logo = None;
    s.title_box = None;
    s.title_but = None;
    s.sl_back = None;
    s.sl_flags = None;
    s.sl_arrows = None;
    s.sl_stars = None;
    s.sl_button = None;
    s.runes = None;
    s.title_mute = None;
    s.flame_gradient0.clear();
    s.flame_gradient1.clear();
    s.flame_gradient2.clear();
    s.flame_gradient.clear();
    s.flame_buffer0.clear();
    s.flame_buffer1.clear();
    s.flame_buffer2.clear();
    s.flame_buffer3.clear();
    s.flame_cycle = 0;
    s.login_user.clear();
    s.login_pass.clear();
}

// @ObfuscatedName("br.n(Lft;B)V") — TitleScreen.generateFlameCoolingMap
pub fn generate_flame_cooling_map(seed_glyph: Option<&Pix8>) {
    let mut s = STATE.lock().unwrap();
    let var1: i32 = 256;
    for var2 in 0..s.flame_buffer0.len() {
        s.flame_buffer0[var2] = 0;
    }
    for _ in 0..5000 {
        let var4 = (random_f64() * 128.0 * var1 as f64) as usize;
        if var4 < s.flame_buffer0.len() {
            s.flame_buffer0[var4] = (random_f64() * 256.0) as i32;
        }
    }
    for _ in 0..20 {
        for var6 in 1..(var1 - 1) {
            for var7 in 1..127 {
                let var8 = ((var6 << 7) + var7) as usize;
                s.flame_buffer1[var8] = (s.flame_buffer0[var8 - 1]
                    + s.flame_buffer0[var8 + 1]
                    + s.flame_buffer0[var8 - 128]
                    + s.flame_buffer0[var8 + 128]) / 4;
            }
        }
        // Java swaps the two buffer references each blur iteration.
        let tmp = std::mem::take(&mut s.flame_buffer0);
        s.flame_buffer0 = std::mem::take(&mut s.flame_buffer1);
        s.flame_buffer1 = tmp;
    }
    if let Some(arg0) = seed_glyph {
        let mut var10 = 0usize;
        for var11 in 0..arg0.hi {
            for var12 in 0..arg0.wi {
                if var10 < arg0.data.len() && arg0.data[var10] != 0 {
                    let var13 = var12 + 16 + arg0.xof;
                    let var14 = var11 + 16 + arg0.yof;
                    if var13 >= 0 && var13 < 128 && var14 >= 0 && var14 < 256 {
                        let var15 = ((var14 << 7) + var13) as usize;
                        if var15 < s.flame_buffer0.len() {
                            s.flame_buffer0[var15] = 0;
                        }
                    }
                }
                var10 += 1;
            }
        }
    }
}

// @ObfuscatedName("eh.j(IIII)I") — TitleScreen.merge
fn merge(arg0: i32, arg1: i32, arg2: i32) -> i32 {
    let var3 = 256 - arg2;
    let r0 = ((arg0 & 0xFF00FF) as u32 * var3 as u32) + ((arg1 & 0xFF00FF) as u32 * arg2 as u32);
    let r0 = (r0 & 0xFF00FF00) as i32;
    let r1 = ((arg0 & 0xFF00) as u32 * var3 as u32) + ((arg1 & 0xFF00) as u32 * arg2 as u32);
    let r1 = (r1 & 0xFF0000) as i32;
    (r0 + r1) >> 8
}

// @ObfuscatedName("g.cc(Lfs;Lfs;I)V") — TitleScreen.draw
pub fn draw(
    fb: &mut Framebuffer,
    b12: Option<&PixFontGeneric>,
    p11: Option<&PixFontGeneric>,
    client_state: i32,
    client_loop_cycle: i32,
    lang: i32,
) {
    {
        let mut pix = pix2d::STATE.lock().unwrap();
        let shell = SHELL.lock().unwrap();
        let need = (shell.s_wid * shell.s_hei) as usize;
        if pix.pixels.len() != need {
            pix.pixels = vec![0i32; need];
            pix.width = shell.s_wid as i32;
            pix.height = shell.s_hei as i32;
            pix.clip_min_x = 0;
            pix.clip_min_y = 0;
            pix.clip_max_x = pix.width;
            pix.clip_max_y = pix.height;
        }
    }

    // World-switch sub-screen replaces the whole title render while
    // open (TitleScreen.java:407-410).
    if crate::world_entry::WORLDS.lock().unwrap().switch_screen {
        world_switch_render(b12, p11);
        blit_to_framebuffer(fb);
        return;
    }

    pix2d::cls();

    // Title-screen render — bare layout first, flames + text layered on.
    let s = STATE.lock().unwrap();
    if let Some(b) = s.title_back.as_ref() { b.quick_plot_sprite(0, 0); }
    if let Some(b) = s.title_back2.as_ref() { b.quick_plot_sprite(382, 0); }
    if let Some(l) = s.logo.as_ref() { l.plot_sprite(382 - l.wi / 2, 18); }

    // Java: state==10 → titleBox always; buttons depend on loginscreen.
    if client_state == 10 {
        if let Some(box_) = s.title_box.as_ref() {
            box_.plot_sprite(202, 171);
        }
        let loginscreen = s.loginscreen;
        if loginscreen == 0 {
            if let Some(but) = s.title_but.as_ref() {
                but.plot_sprite(302 - 73, 291 - 20);
                but.plot_sprite(462 - 73, 291 - 20);
            }
        }
    } else if client_state == 20 {
        // Java draws titleBox at (382 - wi/2, 271 - hi/2) for state 20 —
        // slightly different anchor than state 10.
        if let Some(box_) = s.title_box.as_ref() {
            let x = 382 - box_.wi / 2;
            let y = 271 - box_.hi / 2;
            box_.plot_sprite(x, y);
        }
    }
    drop(s);

    // State 20 — connecting screen. Shows the messages + the read-only
    // username/password fields. No caret, no buttons.
    if client_state == 20 {
        if let Some(p) = p11 {
            let s = STATE.lock().unwrap();
            let login_user = s.login_user.clone();
            let login_pass = s.login_pass.clone();
            let login_mes1 = s.login_mes1.clone();
            let login_mes2 = s.login_mes2.clone();
            let login_mes3 = s.login_mes3.clone();
            drop(s);
            let mut y = 211;
            p.base.centre_string(&login_mes1, 382, y, 0xFFFF00, 0); y += 15;
            p.base.centre_string(&login_mes2, 382, y, 0xFFFF00, 0); y += 15;
            p.base.centre_string(&login_mes3, 382, y, 0xFFFF00, 0); y += 15 + 10;
            p.base.draw_string(text::USERNAMEPROMPT, 272, y, 0xFFFFFF, 0);
            let user_input_x = 272 + p.base.string_wid(text::USERNAMEPROMPT);
            p.base.draw_string(&login_user, user_input_x, y, 0xFFFFFF, 0);
            y += 15;
            let stars: String = "*".repeat(login_pass.len());
            p.base.draw_string(text::PASSWORDPROMPT, 274, y, 0xFFFFFF, 0);
            let pass_input_x = 274 + p.base.string_wid(text::PASSWORDPROMPT);
            p.base.draw_string(&stars, pass_input_x, y, 0xFFFFFF, 0);
        }
    }

    // Java state 10 / login form variants
    if client_state == 10 {
        if let Some(p) = p11 {
            let s = STATE.lock().unwrap();
            let loginscreen = s.loginscreen;
            let login_user = s.login_user.clone();
            let login_pass = s.login_pass.clone();
            let login_select = s.login_select;
            let login_mes1 = s.login_mes1.clone();
            let login_mes2 = s.login_mes2.clone();
            let login_mes3 = s.login_mes3.clone();
            drop(s);

            if loginscreen == 0 {
                p.base.centre_string(text::WELCOMETORUNESCAPE, 382, 251, 0xFFFF00, 0);
                // Java TitleScreen.java:466-471 — multiline-justified inside
                // the 144x40 button box (matters if a label ever wraps).
                p.base.draw_string_multiline(text::NEWUSER, 302 - 73, 291 - 20, 144, 40,
                                             0xFFFFFF, 0, 1, 1, 0);
                p.base.draw_string_multiline(text::EXISTINGUSER, 462 - 73, 291 - 20, 144, 40,
                                             0xFFFFFF, 0, 1, 1, 0);
            } else if loginscreen == 2 {
                // Redraw title box + the two bottom buttons.
                {
                    let s = STATE.lock().unwrap();
                    if let Some(box_) = s.title_box.as_ref() { box_.plot_sprite(202, 171); }
                    if let Some(but) = s.title_but.as_ref() {
                        but.plot_sprite(302 - 73, 321 - 20);
                        but.plot_sprite(462 - 73, 321 - 20);
                    }
                }
                let mut y = 211;
                p.base.centre_string(&login_mes1, 382, y, 0xFFFF00, 0); y += 15;
                p.base.centre_string(&login_mes2, 382, y, 0xFFFF00, 0); y += 15;
                p.base.centre_string(&login_mes3, 382, y, 0xFFFF00, 0); y += 15 + 10;

                // TitleScreen.java:484-499 verbatim: prompt at 272, value
                // at the FIXED 312 (the PixFont metrics are bit-faithful,
                // so Java's hardcoded offsets hold); the username drops
                // leading chars until it fits 200px; the blink caret is an
                // inline yellow PIPE glyph, not a drawn line. The password
                // line is ONE string (prompt + asterisks + caret) at 274.
                p.base.draw_string(text::USERNAMEPROMPT, 272, y, 0xFFFFFF, 0);
                let mut user = login_user.clone();
                while p.base.string_wid(&user) > 200 {
                    user.remove(0);
                }
                let caret = |selected: bool| -> String {
                    if selected && (client_loop_cycle % 40) < 20 {
                        format!("{}{}",
                                crate::string_constants::tag_colour(16776960),
                                crate::string_constants::PIPE)
                    } else {
                        String::new()
                    }
                };
                let user_line = format!(
                    "{}{}",
                    crate::graphics::pix_font::PixFont::escape(&user),
                    caret(login_select == 0),
                );
                p.base.draw_string(&user_line, 312, y, 0xFFFFFF, 0);
                y += 15;
                let pass_line = format!(
                    "{}{}{}",
                    text::PASSWORDPROMPT,
                    "*".repeat(login_pass.len()),
                    caret(login_select == 1),
                );
                p.base.draw_string(&pass_line, 274, y, 0xFFFFFF, 0);

                p.base.centre_string(text::LOGIN, 302, 321 + 5, 0xFFFFFF, 0);
                p.base.centre_string(text::CANCEL, 462, 321 + 5, 0xFFFFFF, 0);
            } else if loginscreen == 3 {
                {
                    let s = STATE.lock().unwrap();
                    if let Some(but) = s.title_but.as_ref() {
                        but.plot_sprite(382 - 73, 321 - 20);
                    }
                }
                p.base.centre_string(text::NEWUSER1, 382, 211, 0xFFFF00, 0);
                p.base.centre_string(text::NEWUSER2, 382, 236, 0xFFFFFF, 0);
                p.base.centre_string(text::NEWUSER3, 382, 251, 0xFFFFFF, 0);
                p.base.centre_string(text::NEWUSER4, 382, 266, 0xFFFFFF, 0);
                p.base.centre_string(text::NEWUSER5, 382, 281, 0xFFFFFF, 0);
                p.base.centre_string(text::CANCEL, 382, 321 + 5, 0xFFFFFF, 0);
            }
        }
    } else if client_state == 5 {
        // Java draws Text.LOADING_TITLE + a progress bar + load_string
        // overlaid on the title background while state-5 boots config.
        if let Some(p) = p11 {
            let (load_pos, msg) = {
                let s = STATE.lock().unwrap();
                (s.load_pos, s.load_string.clone())
            };
            let y = 20;
            p.base.centre_string(text::LOADING_TITLE, 382, 245 - y, 0xFFFFFF, -1);
            let bar_y = 253 - y;
            // Java: 9179409 == 0x8C1111 (an earlier transcription had the
            // green channel wrong at 0x1F).
            pix2d::draw_rect(230, bar_y, 304, 34, 0x8C1111);
            pix2d::draw_rect(231, bar_y + 1, 302, 32, 0);
            pix2d::fill_rect(232, bar_y + 2, load_pos * 3, 30, 0x8C1111);
            pix2d::fill_rect(load_pos * 3 + 232, bar_y + 2, 300 - load_pos * 3, 30, 0);
            p.base.centre_string(&msg, 382, 276 - y, 0xFFFFFF, -1);
        }
    }

    // ----- Flame animation block (verbatim port of Java TitleScreen.draw
    // lines 533–728). Only runs when the buffers have been allocated by
    // open().
    let mut s = STATE.lock().unwrap();
    if s.flame_cycle > 0 {
        let var29: i32 = 256;
        // Java relies on draw() running every frame, so flameCycle never exceeds
        // 256 (the buffer height) before being consumed. Under wasm render
        // throttling our draw() can be entered with many ticks batched, which
        // would drive the signed loop bounds (var29 - var28) negative and panic
        // the `as usize` casts below. Clamp to 256 = one full buffer refresh,
        // the identical result Java reaches when it advances a whole cycle.
        let var28 = s.flame_cycle.min(var29);
        s.flame_cycle0 = s.flame_cycle0.wrapping_add(var28 * 128);
        let buf0_len = s.flame_buffer0.len() as i32;
        if s.flame_cycle0 > buf0_len {
            s.flame_cycle0 -= buf0_len;
            let var30 = (random_f64() * 12.0) as usize;
            let rune = s.runes.as_ref().and_then(|r| r.get(var30)).cloned();
            drop(s);
            generate_flame_cooling_map(rune.as_ref());
            s = STATE.lock().unwrap();
        }
        let cycle0 = s.flame_cycle0;
        let var32 = var28 * 128;
        let var33 = (var29 - var28) * 128;
        let mask = (buf0_len - 1) as i32;
        for var34 in 0..var33 as usize {
            let buf2_idx = var34 + var32 as usize;
            if buf2_idx >= s.flame_buffer2.len() { break; }
            let cool_idx = ((cycle0 + var34 as i32) & mask) as usize;
            let var35 = s.flame_buffer2[buf2_idx] - s.flame_buffer0[cool_idx] * var28 / 6;
            s.flame_buffer2[var34] = if var35 < 0 { 0 } else { var35 };
        }
        for var36 in (var29 - var28)..var29 {
            let var37 = (var36 * 128) as usize;
            for var38 in 0..128usize {
                let var39 = (random_f64() * 100.0) as i32;
                let idx = var37 + var38;
                if idx >= s.flame_buffer2.len() { continue; }
                s.flame_buffer2[idx] = if var39 < 50 && var38 > 10 && var38 < 118 { 255 } else { 0 };
            }
        }
        if s.flame_gradient_cycle0 > 0 {
            s.flame_gradient_cycle0 -= var28 * 4;
            if s.flame_gradient_cycle0 < 0 { s.flame_gradient_cycle0 = 0; }
        }
        if s.flame_gradient_cycle1 > 0 {
            s.flame_gradient_cycle1 -= var28 * 4;
            if s.flame_gradient_cycle1 < 0 { s.flame_gradient_cycle1 = 0; }
        }
        if s.flame_gradient_cycle0 == 0 && s.flame_gradient_cycle1 == 0 {
            let var40 = (random_f64() * (2000.0 / var28 as f64)) as i32;
            if var40 == 0 { s.flame_gradient_cycle0 = 1024; }
            if var40 == 1 { s.flame_gradient_cycle1 = 1024; }
        }
        // line offset shift
        for var41 in 0..(var29 - var28) as usize {
            s.flame_line_offset[var41] = s.flame_line_offset[var41 + var28 as usize];
        }
        for var42 in (var29 - var28)..var29 {
            let lc = s.flame_loop_cycle;
            s.flame_line_offset[var42 as usize] =
                ((lc as f64 / 14.0).sin() * 16.0
                + (lc as f64 / 15.0).sin() * 14.0
                + (lc as f64 / 16.0).sin() * 12.0) as i32;
            s.flame_loop_cycle = lc.wrapping_add(1);
        }
        s.flame_sparks += var28;
        let var43 = ((client_loop_cycle & 0x1) + var28) / 2;
        if var43 > 0 {
            for _ in 0..(s.flame_sparks * 100) {
                let var45 = (random_f64() * 124.0) as i32 + 2;
                let var46 = (random_f64() * 128.0) as i32 + 128;
                let idx = ((var46 << 7) + var45) as usize;
                if idx < s.flame_buffer2.len() {
                    s.flame_buffer2[idx] = 192;
                }
            }
            s.flame_sparks = 0;
            // Two-pass box blur (var43 radius). Java's outer label loop
            // mixes write-back and read so we mirror it tile-by-tile.
            for var47 in 0..var29 as usize {
                let mut var48: i32 = 0;
                let var49 = var47 * 128;
                for var50 in -var43..128 {
                    if var43 + var50 < 128 {
                        let i = (var49 as i32 + var50 + var43) as usize;
                        if i < s.flame_buffer2.len() { var48 += s.flame_buffer2[i]; }
                    }
                    if var50 - (var43 + 1) >= 0 {
                        let i = (var49 as i32 + var50 - (var43 + 1)) as usize;
                        if i < s.flame_buffer2.len() { var48 -= s.flame_buffer2[i]; }
                    }
                    if var50 >= 0 {
                        let dst = var49 + var50 as usize;
                        if dst < s.flame_buffer3.len() {
                            s.flame_buffer3[dst] = var48 / (var43 * 2 + 1);
                        }
                    }
                }
            }
            for var51 in 0..128usize {
                let mut var52: i32 = 0;
                for var53 in -var43..var29 {
                    let var54 = var53 * 128;
                    if var43 + var53 < var29 {
                        let i = (var43 * 128 + var51 as i32 + var54) as usize;
                        if i < s.flame_buffer3.len() { var52 += s.flame_buffer3[i]; }
                    }
                    if var53 - (var43 + 1) >= 0 {
                        let i = (var51 as i32 + var54 - (var43 + 1) * 128) as usize;
                        if i < s.flame_buffer3.len() { var52 -= s.flame_buffer3[i]; }
                    }
                    if var53 >= 0 {
                        let dst = (var51 as i32 + var54) as usize;
                        if dst < s.flame_buffer2.len() {
                            s.flame_buffer2[dst] = var52 / (var43 * 2 + 1);
                        }
                    }
                }
            }
        }
        s.flame_cycle = 0; // Java resets after consuming the accumulated ticks
    }

    // Gradient mix — sets flame_gradient based on the active cycle.
    if s.flame_gradient_cycle0 > 0 {
        for var56 in 0..256 {
            let g0 = s.flame_gradient0[var56];
            let g1 = s.flame_gradient1[var56];
            let cycle = s.flame_gradient_cycle0;
            s.flame_gradient[var56] = if cycle > 768 {
                merge(g0, g1, 1024 - cycle)
            } else if cycle > 256 {
                g1
            } else {
                merge(g1, g0, 256 - cycle)
            };
        }
    } else if s.flame_gradient_cycle1 > 0 {
        for var57 in 0..256 {
            let g0 = s.flame_gradient0[var57];
            let g2 = s.flame_gradient2[var57];
            let cycle = s.flame_gradient_cycle1;
            s.flame_gradient[var57] = if cycle > 768 {
                merge(g0, g2, 1024 - cycle)
            } else if cycle > 256 {
                g2
            } else {
                merge(g2, g0, 256 - cycle)
            };
        }
    } else {
        for var58 in 0..256 {
            s.flame_gradient[var58] = s.flame_gradient0[var58];
        }
    }

    // Flame render — left side at x=0..128, right side mirrored at
    // x=637..765. Java writes straight into GameShell.drawArea.data, we
    // write into Pix2D.pixels (same role: the shared 32-bit backing buf).
    let pix_width = pix2d::STATE.lock().unwrap().width;
    let pix_len = pix2d::STATE.lock().unwrap().pixels.len();
    // Re-blit title_back over the flame slot so the underlying image
    // shows through where the flame is dark.
    if let Some(b) = s.title_back.as_ref() {
        pix2d::set_clipping(0, 9, 128, 256 + 7);
        b.quick_plot_sprite(0, 0);
        pix2d::reset_clipping();
    }
    let mut var59 = 0i32;
    let mut var60: i32 = 6885; // Java offset into drawArea.data
    let flame_gradient = s.flame_gradient.clone();
    let flame_buffer2 = s.flame_buffer2.clone();
    let line_offset = s.flame_line_offset;
    for var61 in 1..255i32 {
        let var62 = (256 - var61) * line_offset[var61 as usize] / 256;
        let mut var63 = var62 + 22;
        if var63 < 0 { var63 = 0; }
        var59 += var63;
        let mut pix = pix2d::STATE.lock().unwrap();
        let _ = pix_width;
        for _var64 in var63..128 {
            if var59 < 0 || (var59 as usize) >= flame_buffer2.len() { break; }
            let var65 = flame_buffer2[var59 as usize] as usize;
            var59 += 1;
            if var65 == 0 {
                var60 += 1;
            } else {
                if var60 >= 0 && (var60 as usize) < pix.pixels.len() {
                    let var67 = (256 - var65) as u32;
                    let var68 = flame_gradient[var65.min(255)] as u32;
                    let var69 = pix.pixels[var60 as usize] as u32;
                    let blended = ((var68 & 0xFF00) * var65 as u32 + (var69 & 0xFF00) * var67) & 0xFF0000;
                    let blended2 = ((var68 & 0xFF00FF) * var65 as u32 + (var69 & 0xFF00FF) * var67) & 0xFF00FF00;
                    pix.pixels[var60 as usize] = ((blended + blended2) >> 8) as i32;
                }
                var60 += 1;
            }
        }
        var60 += var63 + 765 - 128;
    }
    drop(s);

    // Right-side flame strip
    pix2d::set_clipping(637, 9, 765, 256 + 7);
    let s = STATE.lock().unwrap();
    if let Some(b) = s.title_back2.as_ref() { b.quick_plot_sprite(382, 0); }
    drop(s);
    pix2d::reset_clipping();

    let s = STATE.lock().unwrap();
    let mut var70: i32 = 0;
    let mut var71: i32 = 7546;
    let flame_buffer2 = s.flame_buffer2.clone();
    let flame_gradient = s.flame_gradient.clone();
    let line_offset = s.flame_line_offset;
    drop(s);
    for var72 in 1..255i32 {
        let var73 = (256 - var72) * line_offset[var72 as usize] / 256;
        let var74 = 103 - var73;
        let mut var75 = var71 + var73;
        let mut pix = pix2d::STATE.lock().unwrap();
        for _ in 0..var74 {
            if var70 < 0 || (var70 as usize) >= flame_buffer2.len() { break; }
            let var77 = flame_buffer2[var70 as usize] as usize;
            var70 += 1;
            if var77 == 0 {
                var75 += 1;
            } else {
                if var75 >= 0 && (var75 as usize) < pix.pixels.len() {
                    let var79 = (256 - var77) as u32;
                    let var80 = flame_gradient[var77.min(255)] as u32;
                    let var81 = pix.pixels[var75 as usize] as u32;
                    let blended = ((var80 & 0xFF00FF) * var77 as u32 + (var81 & 0xFF00FF) * var79) & 0xFF00FF00;
                    let blended2 = ((var80 & 0xFF00) * var77 as u32 + (var81 & 0xFF00) * var79) & 0xFF0000;
                    pix.pixels[var75 as usize] = ((blended + blended2) >> 8) as i32;
                }
                var75 += 1;
            }
        }
        var70 += 128 - var74;
        var71 = 765 - var74 - var73 + var75;
    }

    // World-switch button (Java: state > 5 && lang == 0). Lazy-loads
    // sl_button from the sprites archive (the loader slot needs to live
    // on Client; we read it from a one-shot static set during open()).
    if client_state > 5 && lang == 0 {
        let s = STATE.lock().unwrap();
        if let Some(sl) = s.sl_button.as_ref() {
            sl.plot_sprite(5, 463);
            let worldid = crate::client::Client::worldid_global();
            drop(s);
            if let Some(p) = p11 {
                let world_label = format!("{} {}", text::WORLD, worldid);
                p.base.centre_string(&world_label, 5 + 100 / 2, 463 + 35 / 2 - 2, 0xFFFFFF, 0);
                // Java swaps the hint for "Loading..." while the
                // worldlist request is in flight (TitleScreen.java:745-748).
                let hint = if worldlist_request_active() { text::LOADINGDOTDOTDOT } else { text::CLICKTOSWITCH };
                p.base.centre_string(hint, 5 + 100 / 2, 463 + 35 / 2 + 12, 0xFFFFFF, 0);
            }
        }
    }

    // Mute button bottom-right.
    {
        let s = STATE.lock().unwrap();
        if let Some(mute_arr) = s.title_mute.as_ref() {
            let idx = if s.mute { 1 } else { 0 };
            if let Some(m) = mute_arr.get(idx) { m.plot_sprite(725, 463); }
        }
    }

    // Blit Pix2D backbuffer into the framebuffer.
    blit_to_framebuffer(fb);

    // Fallback bar if nothing has loaded.
    let s = STATE.lock().unwrap();
    if s.title_back.is_none() && s.logo.is_none() {
        drop(s);
        let (pos, msg) = {
            let s = STATE.lock().unwrap();
            (s.load_pos, s.load_string.clone())
        };
        let msg = if msg.is_empty() { "Title screen — loading".to_string() } else { msg };
        draw_progress(fb, pos, &msg, None);
    }
    let _ = b12;
    let _ = pix_len;
}

// Copy the Pix2D backbuffer into the winit framebuffer — the
// standalone-port equivalent of Java's `GameShell.drawArea.draw(g, 0, 0)`
// canvas blit at the end of TitleScreen.draw / worldSwitchRender.
fn blit_to_framebuffer(fb: &mut Framebuffer) {
    let pix = pix2d::STATE.lock().unwrap();
    let copy_w = pix.width.min(fb.width);
    let copy_h = pix.height.min(fb.height);
    for y in 0..copy_h {
        for x in 0..copy_w {
            let src_idx = (y * pix.width + x) as usize;
            let dst_idx = (y * fb.width + x) as usize;
            if src_idx < pix.pixels.len() && dst_idx < fb.pixels.len() {
                fb.pixels[dst_idx] = pix.pixels[src_idx] as u32;
            }
        }
    }
}

// Lazy-load sl_button. Java does this inline in TitleScreen.draw; we
// pull it out so the draw fn doesn't need a mutable Js5Loader ref.
pub fn try_load_sl_button(sprites_slot: i32) {
    {
        let s = STATE.lock().unwrap();
        if s.sl_button.is_some() { return; }
    }
    let mut reg = js5_net::LOADERS.lock().unwrap();
    let loaded = reg.get_mut(sprites_slot as usize).and_then(|o| o.as_mut())
        .and_then(|s| pix_loader::make_pix8(s, "sl_button", ""));
    drop(reg);
    if let Some(s) = loaded {
        STATE.lock().unwrap().sl_button = Some(s);
    }
}

// Lazy-load the world-switch sprites. Java does these inline at the
// top of worldSwitchRender (TitleScreen.java:816-830); same
// pulled-out pattern as try_load_sl_button.
pub fn try_load_sl_assets(sprites_slot: i32) {
    {
        let s = STATE.lock().unwrap();
        if s.sl_back.is_some() && s.sl_flags.is_some()
            && s.sl_arrows.is_some() && s.sl_stars.is_some() { return; }
    }
    let mut reg = js5_net::LOADERS.lock().unwrap();
    let Some(loader) = reg.get_mut(sprites_slot as usize).and_then(|o| o.as_mut()) else { return };
    let sl_back = pix_loader::make_pix32_array(loader, "sl_back", "");
    let sl_flags = pix_loader::make_pix8_array(loader, "sl_flags", "");
    let sl_arrows = pix_loader::make_pix8_array(loader, "sl_arrows", "");
    let sl_stars = pix_loader::make_pix8_array(loader, "sl_stars", "");
    drop(reg);
    let mut s = STATE.lock().unwrap();
    if s.sl_back.is_none() { s.sl_back = sl_back; }
    if s.sl_flags.is_none() { s.sl_flags = sl_flags; }
    if s.sl_arrows.is_none() { s.sl_arrows = sl_arrows; }
    if s.sl_stars.is_none() { s.sl_stars = sl_stars; }
}

// ---------------------------------------------------------------------------
// World switcher — TitleScreen.java:813-1239 + HTTPRequest.java.
// ---------------------------------------------------------------------------

// @ObfuscatedName("i") — jag::http::HTTPRequest, reshaped for the
// standalone port: Java polls a SignLink-privileged URL stream in
// stages (4-byte length, then body); we run the blocking HTTP GET on
// a worker thread and poll its result slot. Same observable
// semantics: getData() returns None until the full body has arrived,
// and the request dies after 30s (HTTPRequest.java:44 timeout).
type HttpResultSlot = Arc<Mutex<Option<Result<Vec<u8>, String>>>>;

struct HttpRequest {
    #[cfg(not(target_arch = "wasm32"))]
    result: HttpResultSlot,
    // wasm: no worker threads — ride the applet HTTPRequest, which wraps
    // the browser's fetch() behind the same done/data polling shape.
    #[cfg(target_arch = "wasm32")]
    req: Arc<Mutex<crate::applet::http_request::HTTPRequest>>,
    deadline: crate::host::Instant,
}

impl HttpRequest {
    #[cfg(not(target_arch = "wasm32"))]
    fn new(url: &str) -> Self {
        let result: HttpResultSlot = Arc::new(Mutex::new(None));
        let slot = Arc::clone(&result);
        let url = url.to_string();
        std::thread::spawn(move || {
            let outcome = http_get(&url);
            *slot.lock().unwrap() = Some(outcome);
        });
        Self { result, deadline: crate::host::Instant::now() + std::time::Duration::from_millis(30000) }
    }

    #[cfg(target_arch = "wasm32")]
    fn new(url: &str) -> Self {
        let req = Arc::new(Mutex::new(crate::applet::http_request::HTTPRequest::new(
            url.to_string(),
        )));
        crate::applet::http_request::HTTPRequest::start(Arc::clone(&req));
        Self { req, deadline: crate::host::Instant::now() + std::time::Duration::from_millis(30000) }
    }

    // jag::http::HTTPRequest::GetData — Ok(None) = still pending,
    // Ok(Some(body)) = done, Err = failed/timed out (Java throws
    // IOException; listFetch's catch clears the request).
    fn get_data(&self) -> Result<Option<Vec<u8>>, String> {
        if crate::host::Instant::now() > self.deadline {
            return Err("timeout".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            match self.result.lock().unwrap().take() {
                None => Ok(None),
                Some(Ok(body)) => Ok(Some(body)),
                Some(Err(e)) => Err(e),
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            let mut r = self.req.lock().unwrap();
            if !r.done {
                return Ok(None);
            }
            if r.status == 0 {
                return Err("fetch failed".to_string());
            }
            Ok(Some(std::mem::take(&mut r.data)))
        }
    }
}

// Minimal blocking HTTP/1.1 GET for `http://host[:port]/path`.
// Returns the response body, which (per HTTPRequest.java:66-95) is
// itself `[g4 length][length bytes]` — the caller strips that frame.
#[cfg(not(target_arch = "wasm32"))]
fn http_get(url: &str) -> Result<Vec<u8>, String> {
    use std::io::{Read, Write};
    let rest = url.strip_prefix("http://").ok_or_else(|| format!("unsupported url: {url}"))?;
    let (hostport, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let addr = if hostport.contains(':') { hostport.to_string() } else { format!("{hostport}:80") };
    let mut stream = std::net::TcpStream::connect(&addr).map_err(|e| e.to_string())?;
    stream.set_read_timeout(Some(std::time::Duration::from_millis(30000))).ok();
    stream.set_write_timeout(Some(std::time::Duration::from_millis(10000))).ok();
    let host = hostport.split(':').next().unwrap_or(hostport);
    stream
        .write_all(format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n").as_bytes())
        .map_err(|e| e.to_string())?;
    let mut response = Vec::new();
    stream.read_to_end(&mut response).map_err(|e| e.to_string())?;
    let header_end = response.windows(4).position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| "malformed http response".to_string())?;
    Ok(response[header_end + 4..].to_vec())
}

// @ObfuscatedName("l.ag") — TitleScreen.gameworldListDownloadRequest.
static GAMEWORLD_LIST_DOWNLOAD_REQUEST: Mutex<Option<HttpRequest>> = Mutex::new(None);

// True while the worldlist download is in flight — drives the
// "Loading..." label under the sl_button (TitleScreen.java:745-748).
pub fn worldlist_request_active() -> bool {
    GAMEWORLD_LIST_DOWNLOAD_REQUEST.lock().unwrap().is_some()
}

// @ObfuscatedName("ac.q(I)V") — TitleScreen.listFetch (GameWorld::ListFetch).
// Verbatim port of TitleScreen.java:1083-1118: first call arms the
// HTTP request; subsequent calls poll it; a complete body decodes
// into the WORLDS list, sorts, and flips switchScreen on. Any error
// clears the request (Java's catch).
pub fn list_fetch() {
    let mut req = GAMEWORLD_LIST_DOWNLOAD_REQUEST.lock().unwrap();
    if req.is_none() {
        let url = STATE.lock().unwrap().worldlist_url.clone();
        let Some(url) = url else { return };
        *req = Some(HttpRequest::new(&url));
        return;
    }

    let src = match req.as_ref().unwrap().get_data() {
        Ok(None) => return,
        Ok(Some(body)) => body,
        Err(e) => {
            eprintln!("[worldlist] fetch failed: {e}");
            *req = None;
            return;
        }
    };

    // HTTPRequest frames the payload as [g4 length][data]; the Packet
    // Java builds in listFetch starts at the data.
    let decoded = (|| -> Option<()> {
        let mut buf = crate::io::packet::Packet::from_vec(src);
        let length = buf.g4();
        if length < 0 || (buf.data.len() as i32) - 4 < length {
            return None;
        }
        let num = buf.g2();
        let mut list: Vec<crate::world_entry::WorldEntry> = Vec::with_capacity(num.max(0) as usize);
        let mut i = 0;
        while i < num {
            let info = buf.g2();
            list.push(crate::world_entry::WorldEntry {
                id: info & 0x7FFF,
                members: (info & 0x8000) != 0,
                host: buf.gjstr(),
                country: buf.g1(),
                players: buf.g2b(),
                index: i,
            });
            i += 1;
        }
        let mut w = crate::world_entry::WORLDS.lock().unwrap();
        w.num = num;
        w.list = list;
        let ordering = w.ordering;
        let dirs = w.dirs;
        let hi = w.list.len() as i32 - 1;
        quick_sort(&mut w.list, 0, hi, &ordering, &dirs);
        w.switch_screen = true;
        Some(())
    })();
    if decoded.is_none() {
        eprintln!("[worldlist] malformed worldlist payload");
    }
    *req = None;
}

// Comparator key extraction shared by quickSort's two scan loops
// (TitleScreen.java:1159-1180 and 1196-1217). ordering codes:
// 0 = id, 1 = players (-1 maps to 2001 when descending), 2 = index,
// 3 = members.
fn sort_key(e: &crate::world_entry::WorldEntry, pivot: &crate::world_entry::WorldEntry, ord: i32, dir: i32) -> (i32, i32) {
    match ord {
        2 => (e.index, pivot.index),
        1 => {
            let mut a = e.players;
            let mut b = pivot.players;
            if a == -1 && dir == 1 { a = 2001; }
            if b == -1 && dir == 1 { b = 2001; }
            (a, b)
        }
        3 => (if e.members { 1 } else { 0 }, if pivot.members { 1 } else { 0 }),
        _ => (e.id, pivot.id),
    }
}

// @ObfuscatedName("bh.s([Lc;II[I[II)V") — TitleScreen.quickSort
// (GameWorld::QuickSort). Verbatim port of TitleScreen.java:1144-1239.
pub fn quick_sort(arg0: &mut [crate::world_entry::WorldEntry], arg1: i32, arg2: i32, arg3: &[i32; 4], arg4: &[i32; 4]) {
    if arg1 >= arg2 {
        return;
    }

    let mut var5 = arg1 - 1;
    let mut var6 = arg2 + 1;
    let var7 = (arg1 + arg2) / 2;
    arg0.swap(var7 as usize, arg1 as usize);
    let var8 = arg0[arg1 as usize].clone();
    while var5 < var6 {
        loop {
            var6 -= 1;
            let mut var9 = true;
            for var10 in 0..4usize {
                let (var11, var12) = sort_key(&arg0[var6 as usize], &var8, arg3[var10], arg4[var10]);
                if var11 != var12 {
                    if (arg4[var10] != 1 || var11 <= var12) && (arg4[var10] != 0 || var11 >= var12) {
                        var9 = false;
                    }
                    break;
                }
                if var10 == 3 {
                    var9 = false;
                }
            }
            if !var9 {
                break;
            }
        }

        loop {
            var5 += 1;
            let mut var13 = true;
            for var14 in 0..4usize {
                let (var15, var16) = sort_key(&arg0[var5 as usize], &var8, arg3[var14], arg4[var14]);
                if var15 != var16 {
                    if (arg4[var14] != 1 || var15 >= var16) && (arg4[var14] != 0 || var15 <= var16) {
                        var13 = false;
                    }
                    break;
                }
                if var14 == 3 {
                    var13 = false;
                }
            }
            if !var13 {
                break;
            }
        }

        if var5 < var6 {
            arg0.swap(var5 as usize, var6 as usize);
        }
    }

    quick_sort(arg0, arg1, var6, arg3, arg4);
    quick_sort(arg0, var6 + 1, arg2, arg3, arg4);
}

// @ObfuscatedName("client.i(III)V") — TitleScreen.listReorder
// (GameWorld::ListReorder). Verbatim port of TitleScreen.java:1122-1140:
// move the clicked column to the front of the ordering, keep the
// relative order of the rest, then re-sort.
pub fn list_reorder(arg0: i32, arg1: i32) {
    let mut w = crate::world_entry::WORLDS.lock().unwrap();
    let mut var2 = [0i32; 4];
    let mut var3 = [0i32; 4];
    var2[0] = arg0;
    var3[0] = arg1;

    let mut var4 = 1usize;
    for var5 in 0..4usize {
        if w.ordering[var5] != arg0 {
            var2[var4] = w.ordering[var5];
            var3[var4] = w.dirs[var5];
            var4 += 1;
        }
    }
    w.ordering = var2;
    w.dirs = var3;

    let ordering = w.ordering;
    let dirs = w.dirs;
    let hi = w.list.len() as i32 - 1;
    quick_sort(&mut w.list, 0, hi, &ordering, &dirs);
}

// @ObfuscatedName("de.z(Lfm;Lfm;I)V") — TitleScreen.worldSwitchRender.
// Verbatim port of TitleScreen.java:815-997. arg0 = bold font (b12),
// arg1 = plain font (p11). The sprite lazy-loads live in
// try_load_sl_assets (driven from mainloop, same pattern as
// try_load_sl_button).
pub fn world_switch_render(arg0: Option<&PixFontGeneric>, arg1: Option<&PixFontGeneric>) {
    pix2d::fill_rect(0, 23, 765, 480, 0);
    pix2d::fill_rect_v_grad(0, 0, 125, 23, 0xbd9839, 0x8b6608);
    pix2d::fill_rect_v_grad(125, 0, 640, 23, 0x4f4f4f, 0x292929);
    if let Some(f) = arg0 {
        f.base.centre_string(text::SELECTAWORLD, 62, 15, 0, -1);
    }

    let s = STATE.lock().unwrap();
    if let Some(stars) = s.sl_stars.as_ref() {
        if let Some(p) = stars.get(1) { p.plot_sprite(140, 1); }
        if let Some(f) = arg1 {
            f.base.draw_string(text::MEMBERSONLYWORLD, 152, 10, 16777215, -1);
        }
        if let Some(p) = stars.first() { p.plot_sprite(140, 12); }
        if let Some(f) = arg1 {
            f.base.draw_string(text::FREEWORLD, 152, 21, 16777215, -1);
        }
    }

    let (ordering0, dirs0) = {
        let w = crate::world_entry::WORLDS.lock().unwrap();
        (w.ordering[0], w.dirs[0])
    };
    if let Some(arrows) = s.sl_arrows.as_ref() {
        let headers: [(i32, i32, &str); 4] = [
            (280, 0, text::SL_WORLD),
            (390, 1, text::SL_PLAYERS),
            (500, 2, text::SL_LOCATION),
            (610, 3, text::SL_TYPE),
        ];
        for (x, col, label) in headers {
            let up = if ordering0 == col && dirs0 == 0 { 2 } else { 0 };
            if let Some(p) = arrows.get(up) { p.plot_sprite(x, 4); }
            let down = if ordering0 == col && dirs0 == 1 { 3 } else { 1 };
            if let Some(p) = arrows.get(down) { p.plot_sprite(x + 15, 4); }
            if let Some(f) = arg0 {
                f.base.draw_string(label, x + 32, 17, 16777215, -1);
            }
        }
    }

    pix2d::fill_rect(708, 4, 50, 16, 0);
    if let Some(f) = arg1 {
        f.base.centre_string(text::CANCEL, 733, 16, 16777215, -1);
    }

    {
        let mut w = crate::world_entry::WORLDS.lock().unwrap();
        w.sl_last_world = -1;
    }

    if let Some(sl_back) = s.sl_back.as_ref() {
        let (mouse_x, mouse_y) = {
            let m = crate::input::MOUSE.lock().unwrap();
            (m.mouse_x, m.mouse_y)
        };
        let var6: i32 = 88;
        let var7: i32 = 19;
        let mut var8 = 765 / (var6 + 1);
        let mut var9 = 480 / (var7 + 1);

        let num = crate::world_entry::WORLDS.lock().unwrap().num;
        loop {
            let var10 = var9;
            let var11 = var8;
            if (var8 - 1) * var9 >= num {
                var8 -= 1;
            }
            if (var9 - 1) * var8 >= num {
                var9 -= 1;
            }
            if (var9 - 1) * var8 >= num {
                var9 -= 1;
            }
            if var9 == var10 && var8 == var11 {
                break;
            }
        }

        let mut var12 = (765 - var6 * var8) / (var8 + 1);
        if var12 > 5 {
            var12 = 5;
        }

        let mut var13 = (480 - var7 * var9) / (var9 + 1);
        if var13 > 5 {
            var13 = 5;
        }

        let var14 = (765 - var6 * var8 - (var8 - 1) * var12) / 2;
        let var15 = (480 - var7 * var9 - (var9 - 1) * var13) / 2;

        let mut var16 = var15 + 23;
        let mut var17 = var14;
        let mut var18 = 0;

        let mut w = crate::world_entry::WORLDS.lock().unwrap();
        for var19 in 0..w.num {
            let Some(var20) = w.list.get(var19 as usize).cloned() else { break };
            let mut var21 = true;
            let mut var22 = var20.players.to_string();
            if var20.players == -1 {
                var22 = text::OFFLINEWORLD.to_string();
                var21 = false;
            } else if var20.players > 1980 {
                var22 = text::FULLWORLD.to_string();
                var21 = false;
            }

            let back_idx = if var20.members { 1 } else { 0 };
            if mouse_x >= var17 && mouse_y >= var16 && mouse_x < var6 + var17 && mouse_y < var7 + var16 && var21 {
                w.sl_last_world = var19;
                if let Some(b) = sl_back.get(back_idx) { b.lit_plot_sprite(var17, var16, 128, 0xffffff); }
            } else if let Some(b) = sl_back.get(back_idx) {
                b.quick_plot_sprite(var17, var16);
            }

            if let Some(flags) = s.sl_flags.as_ref() {
                let flag_idx = (var20.country + if var20.members { 8 } else { 0 }).max(0) as usize;
                if let Some(p) = flags.get(flag_idx) { p.plot_sprite(var17 + 29, var16); }
            }

            if let Some(f) = arg0 {
                f.base.centre_string(&var20.id.to_string(), var17 + 15, var7 / 2 + var16 + 5, 0, -1);
            }
            if let Some(f) = arg1 {
                f.base.centre_string(&var22, var17 + 60, var7 / 2 + var16 + 5, 0xfffffff, -1);
            }

            var16 += var7 + var13;
            var18 += 1;
            if var18 >= var9 {
                var16 = var15 + 23;
                var17 += var6 + var12;
                var18 = 0;
            }
        }
    }
}

#[cfg(test)]
mod worldlist_tests {
    use super::*;
    use crate::world_entry::WorldEntry;

    fn w(id: i32, players: i32, members: bool, index: i32) -> WorldEntry {
        WorldEntry { id, players, host: String::new(), country: 0, index, members }
    }

    #[test]
    fn http_get_strips_headers() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut buf = [0u8; 1024];
            let _ = s.read(&mut buf);
            let body = [0u8, 0, 0, 2, 0xAB, 0xCD];
            let _ = s.write_all(
                format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len()).as_bytes());
            let _ = s.write_all(&body);
        });
        let body = http_get(&format!("http://127.0.0.1:{port}/worldlist.ws")).unwrap();
        assert_eq!(body, vec![0, 0, 0, 2, 0xAB, 0xCD]);
    }

    // quickSort dir semantics (derived from the Hoare scan conditions,
    // TitleScreen.java:1182/1219): dir 1 = ascending, dir 0 = descending.
    // Defaults ordering [0,1,2,3] / dirs [1,1,1,1] → ascending by id,
    // matching the live client's default world list.
    #[test]
    fn quick_sort_default_ordering_is_ascending_by_id() {
        let mut list = vec![w(3, 10, false, 0), w(1, 30, false, 1), w(2, 20, false, 2)];
        let hi = list.len() as i32 - 1;
        quick_sort(&mut list, 0, hi, &[0, 1, 2, 3], &[1, 1, 1, 1]);
        let ids: Vec<i32> = list.iter().map(|e| e.id).collect();
        assert_eq!(ids, vec![1, 2, 3]);
    }

    #[test]
    fn quick_sort_players_offline_maps_to_2001_when_dir_1() {
        // ordering players-first, dir 1 (ascending): players==-1 is
        // treated as 2001 (sorts past full worlds instead of first).
        let mut list = vec![w(1, 500, false, 0), w(2, -1, false, 1), w(3, 1999, false, 2)];
        let hi = list.len() as i32 - 1;
        quick_sort(&mut list, 0, hi, &[1, 0, 2, 3], &[1, 1, 1, 1]);
        let ids: Vec<i32> = list.iter().map(|e| e.id).collect();
        assert_eq!(ids, vec![1, 3, 2], "500, 1999, then offline (-1→2001) last");
    }

    #[test]
    fn quick_sort_dir_0_is_descending() {
        let mut list = vec![w(3, 10, false, 0), w(1, 30, false, 1), w(2, 20, false, 2)];
        let hi = list.len() as i32 - 1;
        quick_sort(&mut list, 0, hi, &[0, 1, 2, 3], &[0, 1, 1, 1]);
        let ids: Vec<i32> = list.iter().map(|e| e.id).collect();
        assert_eq!(ids, vec![3, 2, 1]);
    }
}

// @ObfuscatedName("cm.g(Ldj;I)V") — TitleScreen.worldSwitchLoop.
// Verbatim port of TitleScreen.java:1001-1079. Mutates the client's
// login host/world/ports on a successful pick (Java writes the
// Client statics directly).
pub fn world_switch_loop(c: &mut crate::client::Client) {
    let (button, click_x, click_y) = {
        let m = crate::input::MOUSE.lock().unwrap();
        (m.mouse_click_button, m.mouse_click_x, m.mouse_click_y)
    };
    if button != 1 {
        return;
    }
    crate::input::MOUSE.lock().unwrap().consume_click();

    // Column-header sort arrows / labels (280/390/500/610).
    let columns: [(i32, i32); 4] = [(280, 0), (390, 1), (500, 2), (610, 3)];
    for (x, col) in columns {
        if click_x >= x && click_x <= x + 14 && click_y >= 4 && click_y <= 18 {
            list_reorder(col, 0);
            return;
        }
        if click_x >= x + 15 && click_x <= x + 80 && click_y >= 4 && click_y <= 18 {
            list_reorder(col, 1);
            return;
        }
    }

    // Cancel button. Java replots the title background into the
    // persistent drawArea; our draw() repaints every frame, but keep
    // the plots for fidelity (they land in the same Pix2D backbuffer).
    if click_x >= 708 && click_y >= 4 && click_x <= 758 && click_y <= 20 {
        crate::world_entry::WORLDS.lock().unwrap().switch_screen = false;
        let s = STATE.lock().unwrap();
        if let Some(b) = s.title_back.as_ref() { b.quick_plot_sprite(0, 0); }
        if let Some(b) = s.title_back2.as_ref() { b.quick_plot_sprite(382, 0); }
        if let Some(l) = s.logo.as_ref() { l.plot_sprite(382 - l.wi / 2, 18); }
        return;
    }

    let picked = {
        let w = crate::world_entry::WORLDS.lock().unwrap();
        if w.sl_last_world != -1 {
            w.list.get(w.sl_last_world as usize).cloned()
        } else {
            None
        }
    };
    if let Some(var5) = picked {
        if c.mem_server == var5.members {
            c.login_host = var5.host.clone();
            c.worldid = var5.id;
            crate::client::WORLDID.store(var5.id, std::sync::atomic::Ordering::Relaxed);
            c.login_game_port = if c.modewhere == 0 { 43594 } else { var5.id + 40000 };
            c.login_js5_port = if c.modewhere == 0 { 443 } else { var5.id + 50000 };
            c.login_port = c.login_game_port;
            crate::world_entry::WORLDS.lock().unwrap().switch_screen = false;
            let s = STATE.lock().unwrap();
            if let Some(b) = s.title_back.as_ref() { b.quick_plot_sprite(0, 0); }
            if let Some(b) = s.title_back2.as_ref() { b.quick_plot_sprite(382, 0); }
            if let Some(l) = s.logo.as_ref() { l.plot_sprite(382 - l.wi / 2, 18); }
            return;
        }

        // Members mismatch — Java redirects the browser to the world's
        // website (`http://host[:id+7000]/j<js>` via AppletContext.
        // showDocument). The standalone shell has no browser context.
        let var6 = if c.modewhere != 0 { format!(":{}", var5.id + 7000) } else { String::new() };
        eprintln!("[worldlist] members-only world — Java would showDocument http://{}{}/j{}",
                  var5.host, var6, c.js);
    }
}

// @ObfuscatedName("g.bk(Lclient;I)V") — TitleScreen.loop
//
// Drains input + drives the loginscreen state machine. Returns Some(20)
// when the user clicked Login (mainloop should set_main_state(20)).
pub fn loop_tick(c: &mut crate::client::Client) -> Option<i32> {
    use crate::input::*;

    let client_state = c.state;
    let lang = c.lang;

    // World-switch sub-screen swallows the whole loop while open
    // (TitleScreen.java:250-253).
    if crate::world_entry::WORLDS.lock().unwrap().switch_screen {
        world_switch_loop(c);
        return None;
    }

    // Mute click target (bottom-right of mute icon, ~725,453 — slightly
    // larger box than Java).
    let click = {
        let m = MOUSE.lock().unwrap();
        (m.mouse_click_button, m.mouse_click_x, m.mouse_click_y)
    };
    if click.0 == 1 && click.1 >= 715 && click.2 >= 453 {
        let mut s = STATE.lock().unwrap();
        s.mute = !s.mute;
        let muted_now = s.mute;
        drop(s);
        MOUSE.lock().unwrap().consume_click();
        if muted_now { stop_music(); } else { play_scape_main(); }
        return None;
    }

    if client_state == 5 { return None; }
    { STATE.lock().unwrap().flame_cycle += 1; }
    if client_state != 10 { return None; }

    // World-switch button click (5..105, 463..498) arms the worldlist
    // download; while a request is in flight, keep polling it
    // (TitleScreen.java:279-294).
    if lang == 0 {
        if click.0 == 1
            && click.1 >= 5 && click.1 <= 105 && click.2 >= 463 && click.2 <= 498 {
            MOUSE.lock().unwrap().consume_click();
            list_fetch();
            return None;
        }
        if worldlist_request_active() {
            list_fetch();
        }
    }

    let (var8, var9, var10) = click;
    let loginscreen = STATE.lock().unwrap().loginscreen;
    if loginscreen == 0 {
        let var11 = 302; let var12 = 291;
        if var8 == 1 && var9 >= var11 - 75 && var9 <= var11 + 75 && var10 >= var12 - 20 && var10 <= var12 + 20 {
            let mut s = STATE.lock().unwrap();
            s.loginscreen = 3; s.login_select = 0;
            drop(s);
            MOUSE.lock().unwrap().consume_click();
        }
        let var13 = 462;
        if var8 == 1 && var9 >= var13 - 75 && var9 <= var13 + 75 && var10 >= var12 - 20 && var10 <= var12 + 20 {
            let mut s = STATE.lock().unwrap();
            s.login_mes1 = text::PLEASELOGIN1.into();
            s.login_mes2 = text::PLEASELOGIN2.into();
            s.login_mes3 = text::PLEASELOGIN3.into();
            s.loginscreen = 2; s.login_select = 0;
            drop(s);
            MOUSE.lock().unwrap().consume_click();
        }
    } else if loginscreen == 2 {
        let var14 = 231;
        let mut var26 = var14 + 30;
        if var8 == 1 && var10 >= var26 - 15 && var10 < var26 {
            STATE.lock().unwrap().login_select = 0;
            MOUSE.lock().unwrap().consume_click();
        }
        var26 += 15;
        if var8 == 1 && var10 >= var26 - 15 && var10 < var26 {
            STATE.lock().unwrap().login_select = 1;
            MOUSE.lock().unwrap().consume_click();
        }
        let var15 = 302; let var16 = 321;
        if var8 == 1 && var9 >= var15 - 75 && var9 <= var15 + 75 && var10 >= var16 - 20 && var10 <= var16 + 20 {
            let mut s = STATE.lock().unwrap();
            s.login_user = s.login_user.trim().to_string();
            let user_empty = s.login_user.is_empty();
            let pass_empty = s.login_pass.is_empty();
            if user_empty {
                s.login_mes1 = text::LOGIN_USER_LENGTH_A.into();
                s.login_mes2 = text::LOGIN_USER_LENGTH_B.into();
                s.login_mes3 = text::LOGIN_USER_LENGTH_C.into();
                drop(s); MOUSE.lock().unwrap().consume_click();
                return None;
            }
            if pass_empty {
                s.login_mes1 = text::LOGIN_PASS_LENGTH_A.into();
                s.login_mes2 = text::LOGIN_PASS_LENGTH_B.into();
                s.login_mes3 = text::LOGIN_PASS_LENGTH_C.into();
                drop(s); MOUSE.lock().unwrap().consume_click();
                return None;
            }
            s.login_mes1 = text::CONNECTING1.into();
            s.login_mes2 = text::CONNECTING2.into();
            s.login_mes3 = text::CONNECTING3.into();
            drop(s);
            MOUSE.lock().unwrap().consume_click();
            return Some(20);
        }
        let var17 = 462;
        if var8 == 1 && var9 >= var17 - 75 && var9 <= var17 + 75 && var10 >= var16 - 20 && var10 <= var16 + 20 {
            let mut s = STATE.lock().unwrap();
            s.loginscreen = 0;
            s.login_user.clear();
            s.login_pass.clear();
            drop(s);
            MOUSE.lock().unwrap().consume_click();
        }

        // Keyboard input — verbatim Java loop. KEY_BACKSPACE=85, KEY_ENTER=84,
        // KEY_TAB=80. Modewhere==2 (dev mode) lets Enter on the password
        // field submit the login directly. CHAR_LIST is the canonical
        // accepted printable set.
        let modewhere = c.modewhere;
        let mut submit_login = false;
        loop {
            let key = KEYBOARD.lock().unwrap().poll_key();
            let Some(key) = key else { break };
            let printable = CHAR_LIST.chars().any(|c| c == key.ch);
            let mut s = STATE.lock().unwrap();
            if s.login_select == 0 {
                if key.code == KEY_BACKSPACE && !s.login_user.is_empty() {
                    s.login_user.pop();
                }
                if key.code == KEY_ENTER || key.code == KEY_TAB {
                    s.login_select = 1;
                }
                if printable && s.login_user.len() < 320 {
                    s.login_user.push(key.ch);
                }
            } else if s.login_select == 1 {
                if key.code == KEY_BACKSPACE && !s.login_pass.is_empty() {
                    s.login_pass.pop();
                }
                if key.code == KEY_ENTER || key.code == KEY_TAB {
                    s.login_select = 0;
                }
                if modewhere == 2 && key.code == KEY_ENTER {
                    s.login_user = s.login_user.trim().to_string();
                    if s.login_user.is_empty() {
                        s.login_mes1 = text::LOGIN_USER_LENGTH_A.into();
                        s.login_mes2 = text::LOGIN_USER_LENGTH_B.into();
                        s.login_mes3 = text::LOGIN_USER_LENGTH_C.into();
                        break;
                    }
                    if s.login_pass.is_empty() {
                        s.login_mes1 = text::LOGIN_PASS_LENGTH_A.into();
                        s.login_mes2 = text::LOGIN_PASS_LENGTH_B.into();
                        s.login_mes3 = text::LOGIN_PASS_LENGTH_C.into();
                        break;
                    }
                    s.login_mes1 = text::CONNECTING1.into();
                    s.login_mes2 = text::CONNECTING2.into();
                    s.login_mes3 = text::CONNECTING3.into();
                    submit_login = true;
                    break;
                }
                if printable && s.login_pass.len() < 20 {
                    s.login_pass.push(key.ch);
                }
            }
        }
        if submit_login { return Some(20); }
    } else if loginscreen == 3 {
        let var23 = 382; let var24 = 321;
        if var8 == 1 && var9 >= var23 - 75 && var9 <= var23 + 75 && var10 >= var24 - 20 && var10 <= var24 + 20 {
            STATE.lock().unwrap().loginscreen = 0;
            MOUSE.lock().unwrap().consume_click();
        }
    }
    None
}
