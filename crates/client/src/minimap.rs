// jag::oldscape::minimap — the Client.java minimap subsystem.
//
// Java keeps all of this as Client statics: the 512×512 `minimap`
// Pix32 image (rebuilt whenever minusedlevel changes, Client.java
// 5025-5104), the mapback/compass/mapedge/mapdots/mapmarker/
// mapfunction/mapscene sprites (loaded during mainLoad steps 80-130),
// the ellipse mask line tables (prepareMinimap, Client.java
// 2009-2049), and the per-frame minimapDraw renderer (Client.java
// 11918-12077).
//
// We group them into one module-level Mutex. `update` runs from the
// logged-in tick (Java's updateWorld) with full &mut Client access —
// that's where the image rebuild + the dots snapshot happen.
// `draw` runs from the interface renderer (clientCode 1338) and only
// reads the snapshot, mirroring the Java split.

#![allow(dead_code)]

use std::sync::Mutex;

use crate::client::Client;
use crate::graphics::{pix2d, pix_loader};
use crate::graphics::pix32::Pix32;
use crate::graphics::pix8::Pix8;
use crate::js5::js5_net;

pub struct Dots {
    // localPlayer.x / .z — fine world coords.
    pub local_x: i32,
    pub local_z: i32,
    // Ground-obj piles at minusedlevel — tile coords.
    pub obj_tiles: Vec<(i32, i32)>,
    // NPCs with NpcType.minimap && active — fine world coords.
    pub npcs: Vec<(i32, i32)>,
    // Other players — fine world coords + dot kind (2 = white,
    // 3 = friend green, 4 = team blue).
    pub players: Vec<(i32, i32, u8)>,
    // Hint target in scene-relative /32 units (tile*4+2 style), or
    // None when no hint is active.
    pub hint: Option<(i32, i32)>,
    // @ObfuscatedName("client.hu"/"client.hl") — minimapFlagX/Z (route
    // tile coords; 0 = no flag).
    pub flag_x: i32,
    pub flag_z: i32,
    pub loop_cycle: i32,
}

pub struct Minimap {
    sprites_slot: i32,
    // @ObfuscatedName("client.gb") — the 512×512 scene image.
    pub image: Pix32,
    // @ObfuscatedName("client.??") — minimapLevel (-1 forces rebuild).
    pub minimap_level: i32,
    // @ObfuscatedName("client.??") — minimapState (0 normal, 2/5
    // blacked map, ≥3 blacked compass).
    pub state: i32,
    // @ObfuscatedName("client.??") — macroMinimapAngle/Zoom + drift
    // modifiers (anti-macro wobble).
    pub macro_angle: i32,
    pub macro_angle_mod: i32,
    pub macro_zoom: i32,
    pub macro_zoom_mod: i32,
    // Sprites (Java mainLoad steps 80-130; we lazy-load from JS5).
    pub compass: Option<Pix32>,
    pub mapedge: Option<Pix32>,
    pub mapback: Option<Pix8>,
    pub mapdots: Option<Vec<Pix32>>,
    pub mapmarker: Option<Vec<Pix32>>,
    pub mapfunction: Option<Vec<Pix32>>,
    pub mapscene: Option<Vec<Pix8>>,
    // prepareMinimap mask tables.
    pub compass_mask_offsets: Vec<i32>,
    pub compass_mask_lengths: Vec<i32>,
    pub minimap_mask_offsets: Vec<i32>,
    pub minimap_mask_lengths: Vec<i32>,
    // @ObfuscatedName("client.??") — activeMapFunction triples
    // (tile_x, tile_z, mapfunction sprite index).
    pub active_fn: Vec<(i32, i32, usize)>,
    pub dots: Dots,
}

pub static MINIMAP: std::sync::LazyLock<Mutex<Minimap>> = std::sync::LazyLock::new(|| {
    Mutex::new(Minimap {
        sprites_slot: -1,
        image: Pix32 { data: vec![0; 512 * 512], wi: 512, hi: 512, owi: 512, ohi: 512, xof: 0, yof: 0 },
        minimap_level: -1,
        state: 0,
        // Java randomises these at login (±60 / -20..10); fixed start
        // keeps builds deterministic — the drift loop still wobbles
        // them every tick.
        macro_angle: 0,
        macro_angle_mod: 2,
        macro_zoom: 0,
        macro_zoom_mod: 1,
        compass: None,
        mapedge: None,
        mapback: None,
        mapdots: None,
        mapmarker: None,
        mapfunction: None,
        mapscene: None,
        compass_mask_offsets: Vec::new(),
        compass_mask_lengths: Vec::new(),
        minimap_mask_offsets: Vec::new(),
        minimap_mask_lengths: Vec::new(),
        active_fn: Vec::new(),
        dots: Dots {
            local_x: 0, local_z: 0,
            obj_tiles: Vec::new(),
            npcs: Vec::new(),
            players: Vec::new(),
            hint: None,
            flag_x: 0, flag_z: 0,
            loop_cycle: 0,
        },
    })
});

pub fn install(sprites_slot: i32) {
    MINIMAP.lock().unwrap().sprites_slot = sprites_slot;
}

// Reset for logout / new login (Java Client.java:2846-2853).
pub fn reset() {
    let mut mm = MINIMAP.lock().unwrap();
    mm.state = 0;
    mm.minimap_level = -1;
    mm.dots.flag_x = 0;
    mm.dots.flag_z = 0;
}

// Lazy sprite load (Java mainLoad steps 80-130: compass, mapedge,
// mapback, mapdots, mapmarker, mapfunction, mapscene + mapedge.trim()
// + prepareMinimap masks). Returns true once everything needed for
// the chrome draw is in.
fn try_load_sprites(mm: &mut Minimap) -> bool {
    if mm.mapback.is_some() && mm.compass.is_some() && mm.mapedge.is_some()
        && mm.mapdots.is_some() && mm.mapmarker.is_some()
    {
        return true;
    }
    if mm.sprites_slot < 0 {
        return false;
    }
    let mut reg = js5_net::LOADERS.lock().unwrap();
    let Some(loader) = reg.get_mut(mm.sprites_slot as usize).and_then(|o| o.as_mut()) else {
        return false;
    };
    if mm.compass.is_none() {
        mm.compass = pix_loader::make_pix32(loader, "compass", "");
    }
    if mm.mapedge.is_none() {
        mm.mapedge = pix_loader::make_pix32(loader, "mapedge", "");
        if let Some(me) = mm.mapedge.as_mut() {
            me.trim();
        }
    }
    if mm.mapback.is_none() {
        mm.mapback = pix_loader::make_pix8(loader, "mapback", "");
        if mm.mapback.is_some() {
            prepare_masks(mm);
        }
    }
    if mm.mapdots.is_none() {
        mm.mapdots = pix_loader::make_pix32_array(loader, "mapdots", "");
    }
    if mm.mapmarker.is_none() {
        mm.mapmarker = pix_loader::make_pix32_array(loader, "mapmarker", "");
    }
    if mm.mapfunction.is_none() {
        mm.mapfunction = pix_loader::make_pix32_array(loader, "mapfunction", "");
    }
    if mm.mapscene.is_none() {
        mm.mapscene = pix_loader::make_pix8_array(loader, "mapscene", "");
    }
    mm.mapback.is_some() && mm.compass.is_some() && mm.mapedge.is_some()
        && mm.mapdots.is_some() && mm.mapmarker.is_some()
}

// @ObfuscatedName("Client.prepareMinimap") — mask line tables from the
// mapback alpha shape. Verbatim port of Client.java:2009-2049.
fn prepare_masks(mm: &mut Minimap) {
    let Some(mapback) = mm.mapback.as_ref() else { return };
    mm.compass_mask_offsets = vec![0; 33];
    mm.compass_mask_lengths = vec![0; 33];
    mm.minimap_mask_offsets = vec![0; 151];
    mm.minimap_mask_lengths = vec![0; 151];
    for row in 0..33usize {
        let mut start = 999;
        let mut end = 0;
        for col in 0..34i32 {
            let idx = (mapback.wi * row as i32 + col) as usize;
            if mapback.data.get(idx).copied().unwrap_or(1) == 0 {
                if start == 999 {
                    start = col;
                }
            } else if start != 999 {
                end = col;
                break;
            }
        }
        mm.compass_mask_offsets[row] = start;
        mm.compass_mask_lengths[row] = end - start;
    }
    for row in 5..156i32 {
        let mut start = 999;
        let mut end = 0;
        for col in 25..172i32 {
            let idx = (mapback.wi * row + col) as usize;
            if mapback.data.get(idx).copied().unwrap_or(1) == 0 && (col > 34 || row > 34) {
                if start == 999 {
                    start = col;
                }
            } else if start != 999 {
                end = col;
                break;
            }
        }
        mm.minimap_mask_offsets[(row - 5) as usize] = start - 25;
        mm.minimap_mask_lengths[(row - 5) as usize] = end - start;
    }
}

// Deterministic stand-in for Java's Math.random() in the icon-nudge
// walk — a small xorshift over the inputs so icon placement is stable
// across rebuilds (Java re-randomises per build).
fn pseudo_rand(seed: &mut u32) -> u32 {
    let mut s = *seed;
    s ^= s << 13;
    s ^= s >> 17;
    s ^= s << 5;
    *seed = s;
    s
}

// Per-tick update — Java's updateWorld minimap section: anti-macro
// drift (Client.java:2707-2725), the minimapLevel-gated image rebuild
// (Client.java:5025-5104), and the dots snapshot for the draw pass.
pub fn update(c: &mut Client) {
    let mut mm = MINIMAP.lock().unwrap();
    let mm = &mut *mm;

    // Anti-macro wobble.
    mm.macro_angle += mm.macro_angle_mod;
    mm.macro_zoom += mm.macro_zoom_mod;
    if mm.macro_angle < -60 {
        mm.macro_angle_mod = 2;
    }
    if mm.macro_angle > 60 {
        mm.macro_angle_mod = -2;
    }
    if mm.macro_zoom < -20 {
        mm.macro_zoom_mod = 1;
    }
    if mm.macro_zoom > 10 {
        mm.macro_zoom_mod = -1;
    }

    let sprites_ready = try_load_sprites(mm);
    let minused = c.minusedlevel.clamp(0, 3);

    // ── Image rebuild (Client.java:5025-5104) ────────────────────────
    if mm.minimap_level != minused && sprites_ready {
        let world_cache = crate::scene::WORLD_CACHE.lock().unwrap();
        if let Some(world) = world_cache.world.as_ref() {
            mm.minimap_level = minused;
            mm.image.data.iter_mut().for_each(|p| *p = 0);
            let state = crate::client_build::STATE.lock().unwrap();
            let lvl = minused as usize;
            // Tile colours via World.render2DGround, row by row.
            for tz in 1..103i32 {
                let mut offset = ((103 - tz) * 2048 + 24628) as usize;
                for tx in 1..103i32 {
                    if (state.mapl[lvl][tx as usize][tz as usize] & 0x18) == 0 {
                        world.render_2d_ground(&mut mm.image.data, offset, 512,
                                               minused, tx, tz);
                    }
                    if minused < 3
                        && (state.mapl[lvl + 1][tx as usize][tz as usize] & 0x8) != 0
                    {
                        world.render_2d_ground(&mut mm.image.data, offset, 512,
                                               minused + 1, tx, tz);
                    }
                    offset += 4;
                }
            }
            // Wall + mapscene detail pass — drawn through the bound
            // 512×512 surface like Java (minimap.setPixels()).
            // Java's per-build random tints (228..247) use the fixed
            // midpoint for deterministic builds.
            let wall_rgb = (238 << 16) + (238 << 8) + 238;
            let door_rgb = 238 << 16;
            mm.image.set_pixels();
            pix2d::set_clipping(0, 0, 512, 512);
            for tz in 1..103i32 {
                for tx in 1..103i32 {
                    if (state.mapl[lvl][tx as usize][tz as usize] & 0x18) == 0 {
                        draw_detail(world, mm.mapscene.as_deref(), minused, tx, tz,
                                    wall_rgb, door_rgb);
                    }
                    if minused < 3
                        && (state.mapl[lvl + 1][tx as usize][tz as usize] & 0x8) != 0
                    {
                        draw_detail(world, mm.mapscene.as_deref(), minused + 1, tx, tz,
                                    wall_rgb, door_rgb);
                    }
                }
            }
            // Read the composited image back (Java binds the array by
            // reference; our set_pixels clones, so copy back).
            {
                let s = pix2d::STATE.lock().unwrap();
                mm.image.data.copy_from_slice(&s.pixels[..512 * 512]);
            }
            // Map function icons (Client.java:5051-5088) — gathered
            // from ground-decor locs with a mapfunction id, nudged off
            // blocked tiles via the collision flags.
            mm.active_fn.clear();
            for tx in 0..104i32 {
                for tz in 0..104i32 {
                    let gd = world.gd_type(minused, tx, tz);
                    if gd == 0 {
                        continue;
                    }
                    let loc_id = (gd >> 14) & 0x7FFF;
                    let Some(lt) = crate::config::loc_type::list(loc_id) else { continue };
                    let func = lt.mapfunction;
                    if func < 0 {
                        continue;
                    }
                    let mut fx = tx;
                    let mut fz = tz;
                    let nudge_exempt = matches!(func, 22 | 29 | 34 | 36 | 46 | 47 | 48);
                    if !nudge_exempt {
                        if let Some(cm) = c.collision.get(minused as usize)
                            .and_then(|o| o.as_ref())
                        {
                            let mut seed = ((tx as u32) << 16) ^ (tz as u32) ^ 0x9E3779B9;
                            for _ in 0..10 {
                                let dir = pseudo_rand(&mut seed) % 4;
                                if dir == 0 && fx > 0 && fx > tx - 3
                                    && (cm.flags[(fx - 1) as usize][fz as usize] & 0x12C0108) == 0
                                {
                                    fx -= 1;
                                }
                                if dir == 1 && fx < 103 && fx < tx + 3
                                    && (cm.flags[(fx + 1) as usize][fz as usize] & 0x12C0180) == 0
                                {
                                    fx += 1;
                                }
                                if dir == 2 && fz > 0 && fz > tz - 3
                                    && (cm.flags[fx as usize][(fz - 1) as usize] & 0x12C0102) == 0
                                {
                                    fz -= 1;
                                }
                                if dir == 3 && fz < 103 && fz < tz + 3
                                    && (cm.flags[fx as usize][(fz + 1) as usize] & 0x12C0120) == 0
                                {
                                    fz += 1;
                                }
                            }
                        }
                    }
                    mm.active_fn.push((fx, fz, func as usize));
                }
            }
        }
    }

    // ── Dots snapshot ────────────────────────────────────────────────
    let d = &mut mm.dots;
    d.loop_cycle = c.loop_cycle;
    d.flag_x = c.minimap_flag_x;
    d.flag_z = c.minimap_flag_z;
    if let Some(lp) = c.local_player.as_ref() {
        d.local_x = lp.entity.x;
        d.local_z = lp.entity.z;
    }
    d.obj_tiles.clear();
    if let Some(level_objs) = c.ground_obj.get(minused as usize) {
        for (tx, col) in level_objs.iter().enumerate() {
            for (tz, pile) in col.iter().enumerate() {
                if !pile.is_empty() {
                    d.obj_tiles.push((tx as i32, tz as i32));
                }
            }
        }
    }
    d.npcs.clear();
    for i in 0..c.npc_count as usize {
        let id = c.npc_ids.get(i).copied().unwrap_or(-1);
        if id < 0 {
            continue;
        }
        let Some(npc) = c.npcs.get(id as usize).and_then(|o| o.as_ref()) else { continue };
        if !npc.ready() {
            continue;
        }
        let mut t = crate::config::npc_type::list(npc.type_id);
        if t.multinpc.is_some() {
            match t.get_multi_npc() {
                Some(resolved) => t = resolved,
                None => continue,
            }
        }
        if t.minimap && t.active {
            d.npcs.push((npc.entity.x, npc.entity.z));
        }
    }
    d.players.clear();
    let (local_team, local_name) = c.local_player.as_ref()
        .map(|lp| (lp.team, lp.name.clone()))
        .unwrap_or((0, String::new()));
    let _ = local_name;
    for i in 0..c.player_count as usize {
        let id = c.player_ids.get(i).copied().unwrap_or(-1);
        if id < 0 {
            continue;
        }
        let Some(p) = c.players.get(id as usize).and_then(|o| o.as_ref()) else { continue };
        if !p.ready() {
            continue;
        }
        let kind = if crate::client::is_friend(c, Some(&p.name)) {
            3
        } else if local_team != 0 && p.team != 0 && local_team == p.team {
            4
        } else {
            2
        };
        d.players.push((p.entity.x, p.entity.z, kind));
    }
    d.hint = match c.hint_type {
        1 => c.npcs.get(c.hint_npc.max(0) as usize)
            .and_then(|o| o.as_ref())
            .map(|n| (n.entity.x / 32, n.entity.z / 32)),
        2 => Some((c.hint_tile_x * 4 - c.map_build_base_x * 4 + 2,
                   c.hint_tile_z * 4 - c.map_build_base_z * 4 + 2)),
        10 => c.players.get(c.hint_player.max(0) as usize)
            .and_then(|o| o.as_ref())
            .map(|p| (p.entity.x / 32, p.entity.z / 32)),
        _ => None,
    };
}

// @ObfuscatedName("Client.minimapDraw") — per-frame chrome render.
// Verbatim port of Client.java:11918-12032. (x, y) is the minimap
// component's top-left, exactly Java's arg0/arg1.
pub fn draw(x: i32, y: i32) {
    let mm = MINIMAP.lock().unwrap();
    let Some(mapback) = mm.mapback.as_ref() else {
        // Sprites not streamed yet — black panel until they land.
        pix2d::fill_rect(x, y, 168, 160, 0);
        return;
    };
    pix2d::set_clipping(x, y, mapback.wi + x, mapback.hi + y);

    let cam_yaw = crate::scene::CAMERA.lock().unwrap().yaw;
    let d = &mm.dots;

    if mm.state == 2 || mm.state == 5 {
        pix2d::fill_scan_line(x + 25, y + 5, 0,
                              &mm.minimap_mask_offsets, &mm.minimap_mask_lengths);
    } else {
        let yaw = (cam_yaw + mm.macro_angle) & 0x7FF;
        let anchor_x = d.local_x / 32 + 48;
        let anchor_y = 464 - d.local_z / 32;
        mm.image.scanline_rotate_plot_sprite(
            x + 25, y + 5, 146, 151, anchor_x, anchor_y, yaw,
            mm.macro_zoom + 256,
            &mm.minimap_mask_offsets, &mm.minimap_mask_lengths);

        // Map function icons.
        if let Some(funcs) = mm.mapfunction.as_ref() {
            for &(fx, fz, idx) in &mm.active_fn {
                let dx = fx * 4 + 2 - d.local_x / 32;
                let dz = fz * 4 + 2 - d.local_z / 32;
                if let Some(sprite) = funcs.get(idx) {
                    draw_dot(&mm, mapback, x, y, dx, dz, sprite, cam_yaw);
                }
            }
        }
        if let Some(dots) = mm.mapdots.as_ref() {
            // Ground-obj piles (yellow dots).
            if let Some(dot) = dots.first() {
                for &(tx, tz) in &d.obj_tiles {
                    let dx = tx * 4 + 2 - d.local_x / 32;
                    let dz = tz * 4 + 2 - d.local_z / 32;
                    draw_dot(&mm, mapback, x, y, dx, dz, dot, cam_yaw);
                }
            }
            // NPCs.
            if let Some(dot) = dots.get(1) {
                for &(nx, nz) in &d.npcs {
                    let dx = nx / 32 - d.local_x / 32;
                    let dz = nz / 32 - d.local_z / 32;
                    draw_dot(&mm, mapback, x, y, dx, dz, dot, cam_yaw);
                }
            }
            // Players (white / friend / team).
            for &(px, pz, kind) in &d.players {
                let dx = px / 32 - d.local_x / 32;
                let dz = pz / 32 - d.local_z / 32;
                if let Some(dot) = dots.get(kind as usize) {
                    draw_dot(&mm, mapback, x, y, dx, dz, dot, cam_yaw);
                }
            }
        }
        // Hint arrow (blinks at 20-cycle period).
        if d.loop_cycle % 20 < 10 {
            if let (Some((hx, hz)), Some(markers)) = (d.hint, mm.mapmarker.as_ref()) {
                if let Some(marker) = markers.get(1) {
                    let dx = hx - d.local_x / 32;
                    let dz = hz - d.local_z / 32;
                    draw_arrow(&mm, mapback, x, y, dx, dz, marker, cam_yaw);
                }
            }
        }
        // Walk-destination flag.
        if d.flag_x != 0 {
            if let Some(markers) = mm.mapmarker.as_ref() {
                if let Some(flag) = markers.first() {
                    let dx = d.flag_x * 4 + 2 - d.local_x / 32;
                    let dz = d.flag_z * 4 + 2 - d.local_z / 32;
                    draw_dot(&mm, mapback, x, y, dx, dz, flag, cam_yaw);
                }
            }
        }
        // Local player — white 3×3 at the map centre.
        pix2d::fill_rect(x + 93 + 4, y + 82 - 4, 3, 3, 0xFFFFFF);
    }

    // Compass (rotates with raw camera yaw, no macro angle).
    if mm.state < 3 {
        if let Some(compass) = mm.compass.as_ref() {
            compass.scanline_rotate_plot_sprite(
                x, y, 33, 33, 25, 25, cam_yaw, 256,
                &mm.compass_mask_offsets, &mm.compass_mask_lengths);
        }
    } else {
        pix2d::fill_scan_line(x, y, 0,
                              &mm.compass_mask_offsets, &mm.compass_mask_lengths);
    }

    mapback.plot_sprite(x, y);
}

// @ObfuscatedName("g.gw(IIIILfq;I)V") — Client.minimapDrawDot.
// Verbatim port of Client.java:12057-12077. (dx, dz) is the target's
// offset from the local player in /32 fine units.
fn draw_dot(mm: &Minimap, mapback: &Pix8, x: i32, y: i32, dx: i32, dz: i32,
            sprite: &Pix32, cam_yaw: i32) {
    let dist2 = dx * dx + dz * dz;
    if dist2 > 6400 {
        return;
    }
    let yaw = ((cam_yaw + mm.macro_angle) & 0x7FF) as usize;
    let sin = crate::dash3d::pix3d::sin_table()[yaw];
    let cos = crate::dash3d::pix3d::cos_table()[yaw];
    let sin = sin * 256 / (mm.macro_zoom + 256);
    let cos = cos * 256 / (mm.macro_zoom + 256);
    let rx = (dx * cos + dz * sin) >> 16;
    let rz = (dz * cos - dx * sin) >> 16;
    if dist2 > 2500 {
        sprite.scanline_plot_sprite(mapback,
                                    x + 94 + rx - sprite.owi / 2 + 4,
                                    y + 83 - rz - sprite.ohi / 2 - 4);
    } else {
        sprite.plot_sprite(x + 94 + rx - sprite.owi / 2 + 4,
                           y + 83 - rz - sprite.ohi / 2 - 4);
    }
}

// @ObfuscatedName("ak.gm(IIIILfq;B)V") — Client.minimapDrawArrow.
// Verbatim port of Client.java:12036-12053: targets between 65 and
// 300 /32-units away render as an edge arrow pointing at them;
// anything nearer/farther falls back to a dot.
fn draw_arrow(mm: &Minimap, mapback: &Pix8, x: i32, y: i32, dx: i32, dz: i32,
              sprite: &Pix32, cam_yaw: i32) {
    let dist2 = dx * dx + dz * dz;
    if dist2 <= 4225 || dist2 >= 90000 {
        draw_dot(mm, mapback, x, y, dx, dz, sprite, cam_yaw);
        return;
    }
    let yaw = ((cam_yaw + mm.macro_angle) & 0x7FF) as usize;
    let sin = crate::dash3d::pix3d::sin_table()[yaw];
    let cos = crate::dash3d::pix3d::cos_table()[yaw];
    let sin = sin * 256 / (mm.macro_zoom + 256);
    let cos = cos * 256 / (mm.macro_zoom + 256);
    let rx = (dx * cos + dz * sin) >> 16;
    let rz = (dz * cos - dx * sin) >> 16;
    let theta = (rx as f64).atan2(rz as f64);
    let edge_x = (theta.sin() * 63.0) as i32;
    let edge_y = (theta.cos() * 57.0) as i32;
    if let Some(mapedge) = mm.mapedge.as_ref() {
        mapedge.rotate_trans_plot_sprite(
            x + 94 + edge_x + 4 - 10, y + 83 - edge_y - 20,
            20, 20, 15, 15, theta, 256);
    }
}

// @ObfuscatedName("bs.eh(IIIIII)V") — Client.drawDetail. Verbatim
// port of Client.java:5355-5485: stamps wall lines / diagonal-loc
// crosses / mapscene sprites for one tile into the bound 512×512
// minimap surface.
fn draw_detail(world: &crate::dash3d::world::World,
               mapscene: Option<&[Pix8]>,
               level: i32, tx: i32, tz: i32,
               wall_rgb: i32, door_rgb: i32) {
    let put = |idx: i32, rgb: i32| {
        let mut s = pix2d::STATE.lock().unwrap();
        if idx >= 0 && (idx as usize) < s.pixels.len() {
            let i = idx as usize;
            s.pixels[i] = rgb;
        }
    };
    let wall_tc = world.wall_type(level, tx, tz);
    if wall_tc != 0 {
        let tc2 = world.typecode2(level, tx, tz, wall_tc);
        let rot = (tc2 >> 6) & 0x3;
        let shape = tc2 & 0x1F;
        let rgb = if wall_tc > 0 { door_rgb } else { wall_rgb };
        let base = (103 - tz) * 2048 + tx * 4 + 24624;
        let loc_id = (wall_tc >> 14) & 0x7FFF;
        let lt = crate::config::loc_type::list(loc_id);
        let scene_id = lt.as_ref().map_or(-1, |t| t.mapscene);
        if scene_id != -1 {
            if let Some(sprite) = mapscene.and_then(|m| m.get(scene_id as usize)) {
                let (lw, ll) = lt.as_ref().map_or((1, 1), |t| (t.width, t.length));
                let ox = (lw * 4 - sprite.wi) / 2;
                let oy = (ll * 4 - sprite.hi) / 2;
                sprite.plot_sprite(tx * 4 + 48 + ox, (104 - tz - ll) * 4 + 48 + oy);
            }
        } else {
            if shape == 0 || shape == 2 {
                match rot {
                    0 => {
                        put(base, rgb);
                        put(base + 512, rgb);
                        put(base + 1024, rgb);
                        put(base + 1536, rgb);
                    }
                    1 => {
                        put(base, rgb);
                        put(base + 1, rgb);
                        put(base + 2, rgb);
                        put(base + 3, rgb);
                    }
                    2 => {
                        put(base + 3, rgb);
                        put(base + 3 + 512, rgb);
                        put(base + 3 + 1024, rgb);
                        put(base + 3 + 1536, rgb);
                    }
                    _ => {
                        put(base + 1536, rgb);
                        put(base + 1536 + 1, rgb);
                        put(base + 1536 + 2, rgb);
                        put(base + 1536 + 3, rgb);
                    }
                }
            }
            if shape == 3 {
                match rot {
                    0 => put(base, rgb),
                    1 => put(base + 3, rgb),
                    2 => put(base + 3 + 1536, rgb),
                    _ => put(base + 1536, rgb),
                }
            }
            if shape == 2 {
                match rot {
                    3 => {
                        put(base, rgb);
                        put(base + 512, rgb);
                        put(base + 1024, rgb);
                        put(base + 1536, rgb);
                    }
                    0 => {
                        put(base, rgb);
                        put(base + 1, rgb);
                        put(base + 2, rgb);
                        put(base + 3, rgb);
                    }
                    1 => {
                        put(base + 3, rgb);
                        put(base + 3 + 512, rgb);
                        put(base + 3 + 1024, rgb);
                        put(base + 3 + 1536, rgb);
                    }
                    _ => {
                        put(base + 1536, rgb);
                        put(base + 1536 + 1, rgb);
                        put(base + 1536 + 2, rgb);
                        put(base + 1536 + 3, rgb);
                    }
                }
            }
        }
    }
    let scene_tc = world.scene_type(level, tx, tz);
    if scene_tc != 0 {
        let tc2 = world.typecode2(level, tx, tz, scene_tc);
        let rot = (tc2 >> 6) & 0x3;
        let shape = tc2 & 0x1F;
        let loc_id = (scene_tc >> 14) & 0x7FFF;
        let lt = crate::config::loc_type::list(loc_id);
        let scene_id = lt.as_ref().map_or(-1, |t| t.mapscene);
        if scene_id != -1 {
            if let Some(sprite) = mapscene.and_then(|m| m.get(scene_id as usize)) {
                let (lw, ll) = lt.as_ref().map_or((1, 1), |t| (t.width, t.length));
                let ox = (lw * 4 - sprite.wi) / 2;
                let oy = (ll * 4 - sprite.hi) / 2;
                sprite.plot_sprite(tx * 4 + 48 + ox, (104 - tz - ll) * 4 + 48 + oy);
            }
        } else if shape == 9 {
            // Diagonal wall — 4-pixel slash; brighter when interactive.
            let rgb = if scene_tc > 0 { 15597568 } else { 15658734 };
            let base = (103 - tz) * 2048 + tx * 4 + 24624;
            if rot == 0 || rot == 2 {
                put(base + 1536, rgb);
                put(base + 1024 + 1, rgb);
                put(base + 512 + 2, rgb);
                put(base + 3, rgb);
            } else {
                put(base, rgb);
                put(base + 512 + 1, rgb);
                put(base + 1024 + 2, rgb);
                put(base + 1536 + 3, rgb);
            }
        }
    }
    let gd_tc = world.gd_type(level, tx, tz);
    if gd_tc != 0 {
        let loc_id = (gd_tc >> 14) & 0x7FFF;
        let lt = crate::config::loc_type::list(loc_id);
        let scene_id = lt.as_ref().map_or(-1, |t| t.mapscene);
        if scene_id != -1 {
            if let Some(sprite) = mapscene.and_then(|m| m.get(scene_id as usize)) {
                let (lw, ll) = lt.as_ref().map_or((1, 1), |t| (t.width, t.length));
                let ox = (lw * 4 - sprite.wi) / 2;
                let oy = (ll * 4 - sprite.hi) / 2;
                sprite.plot_sprite(tx * 4 + 48 + ox, (104 - tz - ll) * 4 + 48 + oy);
            }
        }
    }
}
