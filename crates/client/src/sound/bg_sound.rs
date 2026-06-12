// @ObfuscatedName("de") — jag::oldscape::sound::BgSound.
//
// Positional ambient sound emitter. ClientBuild.addLoc registers one
// per loc with LocType.bgsound_sound != -1; doMix fades them in/out
// based on local-player distance from the bbox.
//
// Verbatim port of BgSound.java with Java's field layout (world-space
// coords × 128, not tile-space). The actual WaveStream playback uses
// stub plumbing — Client.mixer/jagFX wiring lands when the JagFX
// loader is real, but the addSound / recalcSound / doMix control flow
// is the full Java port.

#![allow(dead_code)]

use std::sync::Mutex;

#[derive(Debug, Clone)]
pub struct BgSound {
    // @ObfuscatedName("de.c")
    pub level: i32,
    // @ObfuscatedName("de.n")
    pub min_x: i32,
    // @ObfuscatedName("de.j")
    pub min_z: i32,
    // @ObfuscatedName("de.z")
    pub max_x: i32,
    // @ObfuscatedName("de.g")
    pub max_z: i32,
    // @ObfuscatedName("de.q")
    pub range: i32,
    // @ObfuscatedName("de.i")
    pub sound: i32,
    // @ObfuscatedName("de.s")
    pub mindelay: i32,
    // @ObfuscatedName("de.u") — continuous WaveStream voice id (-1 = none).
    pub continuous_stream: i32,
    // @ObfuscatedName("de.v")
    pub maxdelay: i32,
    // @ObfuscatedName("de.w")
    pub random: Option<Vec<i32>>,
    // @ObfuscatedName("de.e")
    pub random_sound_timer: i32,
    // @ObfuscatedName("de.b") — random one-shot voice id (-1 = none).
    pub random_stream: i32,
    // @ObfuscatedName("de.y") — multiloc dispatch source LocType id; -1
    // if the emitter doesn't change with multivar.
    pub multiloc_id: i32,
}

impl BgSound {
    pub fn new() -> Self {
        Self {
            level: 0,
            min_x: 0, min_z: 0, max_x: 0, max_z: 0,
            range: 0,
            sound: -1,
            mindelay: 0, maxdelay: 0,
            continuous_stream: -1, random_stream: -1,
            random: None,
            random_sound_timer: 0,
            multiloc_id: -1,
        }
    }
}

impl Default for BgSound { fn default() -> Self { Self::new() } }

// @ObfuscatedName("de.m") — BgSound.soundlist. Java uses LinkList with
// Linkable base class; we mirror with a Vec since iteration order is
// stable and removal is rare (only via reset()).
pub static SOUNDLIST: Mutex<Vec<BgSound>> = Mutex::new(Vec::new());

// @ObfuscatedName("az.c(B)V") — BgSound.recalculateMultilocs.
// Verbatim port of BgSound.java:60-67. Called when a multivar-bit or
// multivar changes server-side; iterates every emitter and lets each
// re-pick its active LocType variant.
pub fn recalculate_multilocs() {
    use crate::config::loc_type;
    let mut list = SOUNDLIST.lock().unwrap();
    for bg in list.iter_mut() {
        if bg.multiloc_id == -1 { continue; }
        if let Some(parent) = loc_type::list(bg.multiloc_id) {
            recalc_sound_for(bg, &parent);
        }
    }
}

// @ObfuscatedName("de.n(I)V") — BgSound.recalcSound. Verbatim port of
// BgSound.java:85-108. Pulled out as a free function so
// recalculate_multilocs can call it without re-borrowing SOUNDLIST.
//
// Note: `recalcSound` takes the multiloc's parent LocType (Java stores
// it on `this.multiloc`), looks up the active variant via
// `getMultiLoc()`, and copies the bgsound_* fields off it.
pub fn recalc_sound_for(bg: &mut BgSound, multiloc_parent: &crate::config::loc_type::LocType) {
    let prev_sound = bg.sound;
    let active = multiloc_parent.get_multi_loc();
    match active {
        None => {
            bg.sound = -1;
            bg.range = 0;
            bg.mindelay = 0;
            bg.maxdelay = 0;
            bg.random = None;
        }
        Some(loc) => {
            bg.sound = loc.bgsound_sound;
            bg.range = loc.bgsound_range * 128;
            bg.mindelay = loc.bgsound_mindelay;
            bg.maxdelay = loc.bgsound_maxdelay;
            bg.random = loc.bgsound_random.clone();
        }
    }
    if bg.sound != prev_sound && bg.continuous_stream != -1 {
        stop_stream(bg.continuous_stream);
        bg.continuous_stream = -1;
    }
}

// @ObfuscatedName("bs.j(IIILey;IB)V") — BgSound.addSound. Verbatim
// port of BgSound.java:111-139. Called from ClientBuild.addLoc.
//
// Java args: (tile_x, tile_z, level, LocType, rotation). The
// width/length swap on rotations 1/3 mirrors Java's `(rotation == 1 ||
// rotation == 3) → swap(width, length)`.
pub fn add_sound(level: i32, tile_x: i32, tile_z: i32, loc: &crate::config::loc_type::LocType, rotation: i32) {
    let mut bg = BgSound::new();
    bg.level = level;
    bg.min_x = tile_x * 128;
    bg.min_z = tile_z * 128;
    let (w, l) = if rotation == 1 || rotation == 3 {
        (loc.length, loc.width)
    } else {
        (loc.width, loc.length)
    };
    bg.max_x = (tile_x + w) * 128;
    bg.max_z = (tile_z + l) * 128;
    bg.sound = loc.bgsound_sound;
    bg.range = loc.bgsound_range * 128;
    bg.mindelay = loc.bgsound_mindelay;
    bg.maxdelay = loc.bgsound_maxdelay;
    bg.random = loc.bgsound_random.clone();
    if loc.multiloc.is_some() {
        bg.multiloc_id = loc.id;
        recalc_sound_for(&mut bg, loc);
    }
    if let Some(ref randoms) = bg.random {
        let _ = randoms; // captured for the timer init below.
        bg.random_sound_timer = bg.mindelay + ((rand_unit() * (bg.maxdelay - bg.mindelay) as f64) as i32);
    }
    SOUNDLIST.lock().unwrap().push(bg);
}

// @ObfuscatedName("ex.z(IIIII)V") — BgSound.doMix. Verbatim port of
// BgSound.java:142-206. Per-tick driver: walks every emitter, fades
// volume by player distance, starts/stops continuous streams, fires
// off random one-shots at random_sound_timer intervals.
//
// `tick_ms` is the elapsed ms since the previous doMix call (Java's
// arg3); we decrement random_sound_timer by that each tick.
pub fn do_mix(player_level: i32, player_x: i32, player_z: i32, tick_ms: i32) {
    let ambient = ambient_volume();
    let mut list = SOUNDLIST.lock().unwrap();
    for bg in list.iter_mut() {
        if bg.sound == -1 && bg.random.is_none() { continue; }

        let mut dist = 0i32;
        if player_x > bg.max_x { dist += player_x - bg.max_x; }
        else if player_x < bg.min_x { dist += bg.min_x - player_x; }
        if player_z > bg.max_z { dist += player_z - bg.max_z; }
        else if player_z < bg.min_z { dist += bg.min_z - player_z; }

        if dist - 64 > bg.range || ambient == 0 || bg.level != player_level {
            if bg.continuous_stream != -1 { stop_stream(bg.continuous_stream); bg.continuous_stream = -1; }
            if bg.random_stream != -1 { stop_stream(bg.random_stream); bg.random_stream = -1; }
        } else {
            let d = (dist - 64).max(0);
            let vol = ambient * (bg.range - d) / bg.range.max(1);
            if bg.continuous_stream != -1 {
                apply_volume(bg.continuous_stream, vol);
            } else if bg.sound >= 0 {
                bg.continuous_stream = play_jag_fx(bg.sound, vol, -1);
            }
            if bg.random_stream != -1 {
                apply_volume(bg.random_stream, vol);
                if !stream_is_linked(bg.random_stream) {
                    bg.random_stream = -1;
                }
            } else if let Some(randoms) = bg.random.clone() {
                bg.random_sound_timer -= tick_ms;
                if bg.random_sound_timer <= 0 && !randoms.is_empty() {
                    let pick = (rand_unit() * randoms.len() as f64) as usize;
                    let pick = pick.min(randoms.len() - 1);
                    bg.random_stream = play_jag_fx(randoms[pick], vol, 0);
                    bg.random_sound_timer = bg.mindelay
                        + ((rand_unit() * (bg.maxdelay - bg.mindelay) as f64) as i32);
                }
            }
        }
    }
}

// @ObfuscatedName(none — Java's `reset`) — BgSound.reset. Verbatim port
// of BgSound.java:70-82. Stops every active stream and clears the
// registry. Called on scene rebuild.
pub fn reset() {
    let mut list = SOUNDLIST.lock().unwrap();
    for bg in list.iter_mut() {
        if bg.continuous_stream != -1 { stop_stream(bg.continuous_stream); bg.continuous_stream = -1; }
        if bg.random_stream != -1 { stop_stream(bg.random_stream); bg.random_stream = -1; }
    }
    list.clear();
}

pub fn clear() { reset(); }

// Stubs for the audio plumbing — Java goes through
// `Client.mixer.stopStream(WaveStream)`, `Client.mixer.playStream(...)`,
// `WaveStream.applyVolume(int)`. We don't have those wired up yet, so
// the helpers below are no-ops returning a sentinel voice id.

fn stop_stream(_voice_id: i32) {}
fn apply_volume(_voice_id: i32, _vol: i32) {}
fn stream_is_linked(_voice_id: i32) -> bool { false }
fn play_jag_fx(_sound_id: i32, _vol: i32, _loop_count: i32) -> i32 { -1 }

fn ambient_volume() -> i32 {
    // Java's `Client.ambientVolume` — 0..255. Stub at 128 until the
    // Client settings struct exposes it.
    128
}

fn rand_unit() -> f64 {
    // Java uses `Math.random()` — 0..1.0. We use a cheap LCG keyed by
    // a thread-local counter to keep things deterministic for tests.
    static SEED: Mutex<u64> = Mutex::new(0xDEAD_BEEF_C0FFEE_42);
    let mut g = SEED.lock().unwrap();
    *g = g.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    let v = (*g >> 11) as f64 / (1u64 << 53) as f64;
    v.clamp(0.0, 1.0 - f64::EPSILON)
}
