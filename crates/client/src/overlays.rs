// jag::oldscape::Client — the post-scene entity overlay pass.
//
// Java draws these inside gameDrawMain right after world.renderAll:
//   entityOverlays (Client.java:4458-4669) — head icons, hint icons,
//     chat-above-head with the colour/effect table, combat health
//     bars, hit splats with damage numbers;
//   coordArrow (Client.java:4673-4681) — the tile-hint blink icon;
//   otherOverlays (Client.java:4685-4725) — click crosshair, FPS.
//
// All of it keys off getOverlayPos (Client.java:4729-4760): project an
// entity's fine world coords through the frame camera into
// viewport-centre-relative screen coords (projectX/projectY, -1 when
// behind the near plane).
//
// Split like crate::minimap: Client snapshots the entity state once
// per tick (`snapshot`), the scene renderer publishes the camera it
// rendered with (`set_frame_camera`) and calls `draw` at the end of
// the viewport pass.

#![allow(dead_code)]

use std::sync::Mutex;

use crate::client::Client;
use crate::graphics::pix2d;
use crate::graphics::pix32::Pix32;
use crate::graphics::pix_font_generic::PixFontGeneric;
use crate::js5::js5_net;

// @ObfuscatedName("client.??") — CHAT_COLOURS (Client.java:1008).
const CHAT_COLOURS: [i32; 6] = [16776960, 16711680, 65280, 65535, 16711935, 16777215];

// Java's chat overlay buffer is 50 entries.
const MAX_CHATS: usize = 50;

pub struct OverlayEntity {
    pub x: i32,
    pub z: i32,
    pub height: i32,
    pub is_npc: bool,
    // NPC: NpcType.headicon (multinpc-resolved); players: -1.
    pub npc_headicon: i32,
    // Players: ClientPlayer.headiconPk / headiconPrayer; NPCs: -1.
    pub headicon_pk: i32,
    pub headicon_prayer: i32,
    // Whether the active hint (type 1 npc / type 10 player) targets
    // this entity.
    pub hinted: bool,
    pub chat: Option<String>,
    pub chat_visible: bool,
    pub chat_colour: i32,
    pub chat_effect: i32,
    pub chat_timer: i32,
    pub combat_cycle: i32,
    pub health: i32,
    pub total_health: i32,
    pub damage_values: [i32; 4],
    pub damage_types: [i32; 4],
    pub damage_cycles: [i32; 4],
}

pub struct Overlays {
    sprites_slot: i32,
    // @ObfuscatedName("bf.fq" / "i.ft" / "ef.fx" / "cp.fa" / cross)
    pub headicons_pk: Option<Vec<Pix32>>,
    pub headicons_prayer: Option<Vec<Pix32>>,
    pub headicons_hint: Option<Vec<Pix32>>,
    pub hitmarks: Option<Vec<Pix32>>,
    pub cross: Option<Vec<Pix32>>,
    // @ObfuscatedName(Client.scrollbar) — the two 16×16 arrow caps
    // (Pix8) drawScrollbar blits at the track ends.
    pub scrollbar: Option<Vec<crate::graphics::pix8::Pix8>>,
    // Fonts (cloned once from Client.b12 / p11 / p12).
    pub b12: Option<PixFontGeneric>,
    pub p11: Option<PixFontGeneric>,
    pub p12: Option<PixFontGeneric>,
    // Per-tick snapshot.
    pub ents: Vec<OverlayEntity>,
    // hintType 2 tile hint: world-relative fine coords + height*2.
    pub tile_hint: Option<(i32, i32, i32)>,
    pub loop_cycle: i32,
    pub chat_effects_setting: i32,
    // @ObfuscatedName("client.??") — crossMode / crossX / crossY /
    // crossCycle: the yellow/red click crosshair. Set by the input
    // layer on walk / interact clicks.
    pub cross_mode: i32,
    pub cross_x: i32,
    pub cross_y: i32,
    pub cross_cycle: i32,
    pub show_fps: bool,
    pub fps: i32,
    // Client.minusedlevel mirror for the height sampling.
    pub minusedlevel: i32,
    // The viewport component's top-left from the last frame — the
    // minimenu feedback line anchors here (Java passes the viewport
    // x/y into drawFeedback).
    pub viewport_x: i32,
    pub viewport_y: i32,
    pub viewport_w: i32,
    pub viewport_h: i32,
    // @ObfuscatedName("client.??") — chatDisabled (Tutorial Island
    // region gate computed in otherOverlays).
    pub chat_disabled: bool,
}

pub static OVERLAYS: std::sync::LazyLock<Mutex<Overlays>> = std::sync::LazyLock::new(|| {
    Mutex::new(Overlays {
        sprites_slot: -1,
        headicons_pk: None,
        headicons_prayer: None,
        headicons_hint: None,
        hitmarks: None,
        cross: None,
        scrollbar: None,
        b12: None,
        p11: None,
        p12: None,
        ents: Vec::new(),
        tile_hint: None,
        loop_cycle: 0,
        chat_effects_setting: 0,
        cross_mode: 0,
        cross_x: 0,
        cross_y: 0,
        cross_cycle: 0,
        show_fps: false,
        fps: 0,
        minusedlevel: 0,
        viewport_x: 8,
        viewport_y: 11,
        viewport_w: 512,
        viewport_h: 334,
        chat_disabled: false,
    })
});

// The camera the viewport was rendered with this frame —
// (cam_x, cam_y, cam_z, pitch, yaw). Java reads the camX/camY/camZ/
// camPitch/camYaw statics directly.
pub static FRAME_CAM: Mutex<(i32, i32, i32, i32, i32)> = Mutex::new((0, 0, 0, 256, 0));

pub fn set_frame_camera(cam_x: i32, cam_y: i32, cam_z: i32, pitch: i32, yaw: i32) {
    *FRAME_CAM.lock().unwrap() = (cam_x, cam_y, cam_z, pitch, yaw);
}

// @ObfuscatedName("client.hr") — sceneCycle, bumped once per viewport
// frame; drives chat wave effects and blink timers.
pub static SCENE_CYCLE: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

pub fn install(sprites_slot: i32) {
    OVERLAYS.lock().unwrap().sprites_slot = sprites_slot;
}

fn try_load_sprites(o: &mut Overlays) {
    if o.headicons_pk.is_some() && o.headicons_prayer.is_some()
        && o.headicons_hint.is_some() && o.hitmarks.is_some() && o.cross.is_some()
    {
        return;
    }
    if o.sprites_slot < 0 {
        return;
    }
    let mut reg = js5_net::LOADERS.lock().unwrap();
    let Some(loader) = reg.get_mut(o.sprites_slot as usize).and_then(|s| s.as_mut()) else {
        return;
    };
    use crate::graphics::pix_loader;
    if o.headicons_pk.is_none() {
        o.headicons_pk = pix_loader::make_pix32_array(loader, "headicons_pk", "");
    }
    if o.headicons_prayer.is_none() {
        o.headicons_prayer = pix_loader::make_pix32_array(loader, "headicons_prayer", "");
    }
    if o.headicons_hint.is_none() {
        o.headicons_hint = pix_loader::make_pix32_array(loader, "headicons_hint", "");
    }
    if o.hitmarks.is_none() {
        o.hitmarks = pix_loader::make_pix32_array(loader, "hitmarks", "");
    }
    if o.cross.is_none() {
        o.cross = pix_loader::make_pix32_array(loader, "cross", "");
    }
    if o.scrollbar.is_none() {
        o.scrollbar = pix_loader::make_pix8_array(loader, "scrollbar", "");
    }
    // PixFont.modicons for `<img=N>` markup rides along here (Java
    // loads "mod_icons" in the same step block).
    if crate::graphics::pix_font::MODICONS.lock().unwrap().is_empty() {
        if let Some(icons) = pix_loader::make_pix8_array(loader, "mod_icons", "") {
            crate::graphics::pix_font::install_modicons(icons);
        }
    }
}

// Per-tick snapshot from the logged-in Client (Java reads the live
// entity arrays during the draw; our tick and frame are 1:1 in the
// mainloop, so this is the same data).
pub fn snapshot(c: &Client) {
    let mut o = OVERLAYS.lock().unwrap();
    let o = &mut *o;
    try_load_sprites(o);
    if o.b12.is_none() && c.b12.is_some() {
        o.b12 = c.b12.clone();
    }
    if o.p11.is_none() && c.p11.is_some() {
        o.p11 = c.p11.clone();
    }
    if o.p12.is_none() && c.p12.is_some() {
        o.p12 = c.p12.clone();
    }
    o.loop_cycle = c.loop_cycle;
    o.chat_effects_setting = c.chat_effects;
    o.show_fps = c.show_fps;
    o.fps = crate::game_shell::SHELL.lock().unwrap().fps;
    o.minusedlevel = c.minusedlevel.clamp(0, 3);

    o.ents.clear();
    // Java's iteration order: local player (-1), players, then NPCs.
    let mut push_player = |o: &mut Overlays, p: &crate::dash3d::ClientPlayer,
                           hinted: bool, c: &Client| {
        if !p.ready() {
            return;
        }
        let e = &p.entity;
        let chat_visible = e.chat.is_some()
            && (c.chat_public_mode == 0
                || c.chat_public_mode == 3
                || (c.chat_public_mode == 1 && crate::client::is_friend(c, Some(&p.name))));
        o.ents.push(OverlayEntity {
            x: e.x,
            z: e.z,
            height: e.height,
            is_npc: false,
            npc_headicon: -1,
            headicon_pk: p.headicon_pk,
            headicon_prayer: p.headicon_prayer,
            hinted,
            chat: e.chat.clone(),
            chat_visible,
            chat_colour: e.chat_colour,
            chat_effect: e.chat_effect,
            chat_timer: e.chat_timer,
            combat_cycle: e.combat_cycle,
            health: e.health,
            total_health: e.total_health.max(1),
            damage_values: e.damage_values,
            damage_types: e.damage_types,
            damage_cycles: e.damage_cycles,
        });
    };
    if let Some(lp) = c.local_player.as_ref() {
        push_player(o, lp, false, c);
    }
    for i in 0..c.player_count as usize {
        let id = c.player_ids.get(i).copied().unwrap_or(-1);
        if id < 0 {
            continue;
        }
        if let Some(p) = c.players.get(id as usize).and_then(|s| s.as_ref()) {
            let hinted = c.hint_type == 10 && c.hint_player == id;
            push_player(o, p, hinted, c);
        }
    }
    for i in 0..c.npc_count as usize {
        let id = c.npc_ids.get(i).copied().unwrap_or(-1);
        if id < 0 {
            continue;
        }
        let Some(n) = c.npcs.get(id as usize).and_then(|s| s.as_ref()) else { continue };
        if !n.ready() {
            continue;
        }
        let mut t = crate::config::npc_type::list(n.type_id);
        if t.multinpc.is_some() {
            match t.get_multi_npc() {
                Some(resolved) => t = resolved,
                None => continue,
            }
        }
        let e = &n.entity;
        o.ents.push(OverlayEntity {
            x: e.x,
            z: e.z,
            height: e.height,
            is_npc: true,
            npc_headicon: t.headicon,
            headicon_pk: -1,
            headicon_prayer: -1,
            hinted: c.hint_type == 1 && c.hint_npc == id,
            chat: e.chat.clone(),
            chat_visible: e.chat.is_some(),
            chat_colour: e.chat_colour,
            chat_effect: e.chat_effect,
            chat_timer: e.chat_timer,
            combat_cycle: e.combat_cycle,
            health: e.health,
            total_health: e.total_health.max(1),
            damage_values: e.damage_values,
            damage_types: e.damage_types,
            damage_cycles: e.damage_cycles,
        });
    }
    o.tile_hint = if c.hint_type == 2 {
        Some((((c.hint_tile_x - c.map_build_base_x) << 7) + c.hint_offset_x,
              ((c.hint_tile_z - c.map_build_base_z) << 7) + c.hint_offset_z,
              c.hint_height * 2))
    } else {
        None
    };

    // otherOverlays' chatDisabled region gate (Client.java:4694-4705 —
    // Tutorial Island absolute coords).
    if let Some(lp) = c.local_player.as_ref() {
        let ax = (lp.entity.x >> 7) + c.map_build_base_x;
        let az = (lp.entity.z >> 7) + c.map_build_base_z;
        let mut disabled = (ax >= 3053 && ax <= 3156 && az >= 3056 && az <= 3136)
            || (ax >= 3072 && ax <= 3118 && az >= 9492 && az <= 9535);
        if disabled && ax >= 3139 && ax <= 3199 && az >= 3008 && az <= 3062 {
            disabled = false;
        }
        o.chat_disabled = disabled;
    }
}

// @ObfuscatedName("cl.ez(IIII)V") — Client.getOverlayPos. Projects a
// fine world coord (terrain height sampled via getAvH minus
// `height_off`) through the frame camera. Returns viewport-CENTRE-
// relative coords like Java's projectX/projectY (Java adds the fixed
// 256/167 half-viewport; we add the live half size in `draw`).
fn get_overlay_pos(world_x: i32, world_z: i32, height_off: i32,
                   minusedlevel: i32) -> Option<(i32, i32)> {
    if world_x < 128 || world_z < 128 || world_x > 13056 || world_z > 13056 {
        return None;
    }
    let (cam_x, cam_y, cam_z, pitch, yaw) = *FRAME_CAM.lock().unwrap();
    let y = crate::client::get_av_h(world_x, world_z, minusedlevel) - height_off;
    let dx = world_x - cam_x;
    let dy = y - cam_y;
    let dz = world_z - cam_z;
    let sin_t = crate::dash3d::pix3d::sin_table();
    let cos_t = crate::dash3d::pix3d::cos_table();
    let sin_pitch = sin_t[(pitch & 0x7FF) as usize];
    let cos_pitch = cos_t[(pitch & 0x7FF) as usize];
    let sin_yaw = sin_t[(yaw & 0x7FF) as usize];
    let cos_yaw = cos_t[(yaw & 0x7FF) as usize];
    let vx = (dx * cos_yaw + dz * sin_yaw) >> 16;
    let vt = (dz * cos_yaw - dx * sin_yaw) >> 16;
    let vy = (dy * cos_pitch - sin_pitch * vt) >> 16;
    let vz = (dy * sin_pitch + cos_pitch * vt) >> 16;
    if vz >= 50 {
        Some(((vx << 9) / vz, (vy << 9) / vz))
    } else {
        None
    }
}

// @ObfuscatedName("Client.entityOverlays" + coordArrow + otherOverlays)
// — drawn at the end of the viewport pass, clipped to the viewport.
// (vx, vy, vw, vh) is the viewport rect; Java's var12/var13/var31/var32.
pub fn draw(vx: i32, vy: i32, vw: i32, vh: i32) {
    let mut o = OVERLAYS.lock().unwrap();
    let o = &mut *o;
    o.viewport_x = vx;
    o.viewport_y = vy;
    o.viewport_w = vw;
    o.viewport_h = vh;
    let minusedlevel = o.minusedlevel;
    let scene_cycle = SCENE_CYCLE.load(std::sync::atomic::Ordering::Relaxed);
    let half_w = vw / 2;
    let half_h = vh / 2;
    // Centre-relative → viewport-relative, Java's `projectX + var12`.
    let to_screen = |p: (i32, i32)| (p.0 + half_w, p.1 + half_h);

    let mut chats: Vec<(i32, i32, i32, i32, i32, i32, i32, String)> = Vec::new();
    // (x, y, half_width, height, colour, effect, timer, text)

    for ent in &o.ents {
        // ── Head icons ──────────────────────────────────────────────
        if ent.is_npc {
            if let Some(prayer_icons) = o.headicons_prayer.as_ref() {
                if ent.npc_headicon >= 0 && (ent.npc_headicon as usize) < prayer_icons.len() {
                    if let Some(p) = get_overlay_pos(ent.x, ent.z, ent.height + 15, minusedlevel) {
                        let (px, py) = to_screen(p);
                        prayer_icons[ent.npc_headicon as usize]
                            .plot_sprite(px + vx - 12, py + vy - 30);
                    }
                }
            }
            if ent.hinted && o.loop_cycle % 20 < 10 {
                if let Some(hints) = o.headicons_hint.as_ref() {
                    if let Some(icon) = hints.first() {
                        if let Some(p) = get_overlay_pos(ent.x, ent.z, ent.height + 15, minusedlevel) {
                            let (px, py) = to_screen(p);
                            icon.plot_sprite(px + vx - 12, py + vy - 28);
                        }
                    }
                }
            }
        } else {
            let mut stack = 30;
            if ent.headicon_pk != -1 || ent.headicon_prayer != -1 {
                if let Some(p) = get_overlay_pos(ent.x, ent.z, ent.height + 15, minusedlevel) {
                    let (px, py) = to_screen(p);
                    if ent.headicon_pk != -1 {
                        if let Some(icon) = o.headicons_pk.as_ref()
                            .and_then(|v| v.get(ent.headicon_pk as usize))
                        {
                            icon.plot_sprite(px + vx - 12, py + vy - stack);
                        }
                        stack += 25;
                    }
                    if ent.headicon_prayer != -1 {
                        if let Some(icon) = o.headicons_prayer.as_ref()
                            .and_then(|v| v.get(ent.headicon_prayer as usize))
                        {
                            icon.plot_sprite(px + vx - 12, py + vy - stack);
                        }
                        stack += 25;
                    }
                }
            }
            if ent.hinted {
                if let Some(icon) = o.headicons_hint.as_ref().and_then(|v| v.get(1)) {
                    if let Some(p) = get_overlay_pos(ent.x, ent.z, ent.height + 15, minusedlevel) {
                        let (px, py) = to_screen(p);
                        icon.plot_sprite(px + vx - 12, py + vy - stack);
                    }
                }
            }
        }

        // ── Chat collection ─────────────────────────────────────────
        if let (Some(text), true) = (ent.chat.as_ref(), ent.chat_visible) {
            if chats.len() < MAX_CHATS {
                if let Some(b12) = o.b12.as_ref() {
                    if let Some(p) = get_overlay_pos(ent.x, ent.z, ent.height, minusedlevel) {
                        let (px, py) = to_screen(p);
                        chats.push((px, py,
                                    b12.base.string_wid(text) / 2,
                                    b12.base.ascent,
                                    ent.chat_colour, ent.chat_effect, ent.chat_timer,
                                    text.clone()));
                    }
                }
            }
        }

        // ── Combat health bar ───────────────────────────────────────
        if ent.combat_cycle > o.loop_cycle {
            if let Some(p) = get_overlay_pos(ent.x, ent.z, ent.height + 15, minusedlevel) {
                let (px, py) = to_screen(p);
                let mut green = ent.health * 30 / ent.total_health;
                if green > 30 {
                    green = 30;
                }
                pix2d::fill_rect(px + vx - 15, py + vy - 3, green, 5, 65280);
                pix2d::fill_rect(px + vx - 15 + green, py + vy - 3, 30 - green, 5, 16711680);
            }
        }

        // ── Hit splats ──────────────────────────────────────────────
        for slot in 0..4 {
            if ent.damage_cycles[slot] <= o.loop_cycle {
                continue;
            }
            let Some(p) = get_overlay_pos(ent.x, ent.z, ent.height / 2, minusedlevel) else {
                continue;
            };
            let (mut px, mut py) = to_screen(p);
            match slot {
                1 => py -= 20,
                2 => {
                    px -= 15;
                    py -= 10;
                }
                3 => {
                    px += 15;
                    py -= 10;
                }
                _ => {}
            }
            if let Some(mark) = o.hitmarks.as_ref()
                .and_then(|v| v.get(ent.damage_types[slot].max(0) as usize))
            {
                mark.plot_sprite(px + vx - 12, py + vy - 12);
            }
            if let Some(p11) = o.p11.as_ref() {
                p11.base.centre_string(&ent.damage_values[slot].to_string(),
                                       px + vx - 1, py + vy + 3, 16777215, 0);
            }
        }
    }

    // ── Chat layout + render (Client.java:4573-4668) ────────────────
    let Some(b12) = o.b12.as_ref() else { return };
    let mut placed: Vec<(i32, i32, i32, i32)> = Vec::new(); // x, y, half_w, h
    for (cx, cy0, half, height, colour, effect, timer, text) in chats {
        // Push the bubble up until it no longer overlaps an earlier one.
        let mut cy = cy0;
        let mut moved = true;
        while moved {
            moved = false;
            for &(ox, oy, ohalf, oh) in &placed {
                if cy + 2 > oy - oh && cy - height < oy + 2
                    && cx - half < ox + ohalf && cx + half > ox - ohalf
                    && oy - oh < cy
                {
                    cy = oy - oh;
                    moved = true;
                }
            }
        }
        placed.push((cx, cy, half, height));
        let px = cx;
        let py = cy;
        if o.chat_effects_setting == 0 {
            let mut rgb = 16776960;
            if colour < 6 {
                rgb = CHAT_COLOURS[colour.max(0) as usize];
            }
            if colour == 6 {
                rgb = if scene_cycle % 20 < 10 { 16711680 } else { 16776960 };
            }
            if colour == 7 {
                rgb = if scene_cycle % 20 < 10 { 255 } else { 65535 };
            }
            if colour == 8 {
                rgb = if scene_cycle % 20 < 10 { 45056 } else { 8454016 };
            }
            if colour == 9 {
                let t = 150 - timer;
                rgb = if t < 50 {
                    t * 1280 + 16711680
                } else if t < 100 {
                    16776960 - (t - 50) * 327680
                } else if t < 150 {
                    (t - 100) * 5 + 65280
                } else {
                    16776960
                };
            }
            if colour == 10 {
                let t = 150 - timer;
                rgb = if t < 50 {
                    t * 5 + 16711680
                } else if t < 100 {
                    16711935 - (t - 50) * 327680
                } else if t < 150 {
                    (t - 100) * 327680 + 255 - (t - 100) * 5
                } else {
                    16776960
                };
            }
            if colour == 11 {
                let t = 150 - timer;
                rgb = if t < 50 {
                    16777215 - t * 327685
                } else if t < 100 {
                    (t - 50) * 327685 + 65280
                } else if t < 150 {
                    16777215 - (t - 100) * 327680
                } else {
                    16776960
                };
            }
            match effect {
                0 => b12.base.centre_string(&text, px + vx, py + vy, rgb, 0),
                1 => b12.base.centre_string_wave(&text, px + vx, py + vy, rgb, 0, scene_cycle),
                2 => b12.base.centre_string_wave2(&text, px + vx, py + vy, rgb, 0, scene_cycle),
                3 => b12.base.centre_string_wave3(&text, px + vx, py + vy, rgb, 0,
                                                  scene_cycle, 150 - timer),
                4 => {
                    // Scrolling marquee, clipped to a 100px band.
                    let scroll = (150 - timer) * (b12.base.string_wid(&text) + 100) / 150;
                    pix2d::set_clipping(px + vx - 50, vy, px + vx + 50, vy + vh);
                    b12.base.draw_string(&text, px + vx + 50 - scroll, py + vy, rgb, 0);
                    pix2d::set_clipping(vx, vy, vx + vw, vy + vh);
                }
                5 => {
                    // Slide in/out vertically over the bubble's lifetime.
                    let t = 150 - timer;
                    let mut shift = 0;
                    if t < 25 {
                        shift = t - 25;
                    } else if t > 125 {
                        shift = t - 125;
                    }
                    pix2d::set_clipping(vx, py + vy - b12.base.ascent - 1, vx + vw, py + vy + 5);
                    b12.base.centre_string(&text, px + vx, py + vy + shift, rgb, 0);
                    pix2d::set_clipping(vx, vy, vx + vw, vy + vh);
                }
                _ => b12.base.centre_string(&text, px + vx, py + vy, rgb, 0),
            }
        } else {
            b12.base.centre_string(&text, px + vx, py + vy, 16776960, 0);
        }
    }

    // ── coordArrow (Client.java:4673-4681) ──────────────────────────
    if let Some((hx, hz, hh)) = o.tile_hint {
        if o.loop_cycle % 20 < 10 {
            if let Some(icon) = o.headicons_hint.as_ref().and_then(|v| v.first()) {
                if let Some(p) = get_overlay_pos(hx, hz, hh, minusedlevel) {
                    let (px, py) = to_screen(p);
                    icon.plot_sprite(px + vx - 12, py + vy - 28);
                }
            }
        }
    }

    // ── otherOverlays (Client.java:4685-4725) ───────────────────────
    if o.cross_mode == 1 {
        if let Some(c) = o.cross.as_ref().and_then(|v| v.get((o.cross_cycle / 100) as usize)) {
            c.plot_sprite(o.cross_x - 8, o.cross_y - 8);
        }
    }
    if o.cross_mode == 2 {
        if let Some(c) = o.cross.as_ref().and_then(|v| v.get((o.cross_cycle / 100 + 4) as usize)) {
            c.plot_sprite(o.cross_x - 8, o.cross_y - 8);
        }
    }
    if o.show_fps {
        if let Some(p12) = o.p12.as_ref() {
            p12.base.right_string(&format!("Fps:{}", o.fps),
                                  vx + 512 - 5, vy + 20, 0xFFFF00, -1);
        }
    }
}
