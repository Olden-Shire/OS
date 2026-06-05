//! Map regions: terrain (`m{x}_{y}`) and loc placements (`l{x}_{y}`, XTEA-encrypted).
//!
//! Each rev1 region covers a 64×64 tile area at 4 floor levels (0..=3). Region coordinates
//! `(x, y)` pack into a "mapsquare" id as `(x << 8) | y`, matching the `mapsquare` field in
//! `cache/keys.json`.
//!
//! ## File format references
//!
//! * Terrain: `jagex3.client.ClientBuild.loadGround` + `loadGroundSquare`. Per-tile opcode
//!   stream until a 0 byte; opcodes encode height, overlay (texture id + shape + rotation),
//!   underlay id, and map flags.
//! * Locs: `jagex3.client.ClientBuild.loadLocations`. Delta-encoded loc id outer loop;
//!   delta-encoded packed position inner loop; per-placement shape+rotation byte.

mod perlin;

use std::collections::HashMap;
use std::path::Path;

use io::Packet;
use serde::Deserialize;

/// 128-bit XTEA key as 4 × i32 (same shape as keys.json's `"key"` field).
pub type XteaKey = [i32; 4];

/// Region size: 64 tiles × 64 tiles × 4 levels.
pub const REGION_SIZE: usize = 64;
pub const REGION_LEVELS: usize = 4;

/// Encode region `(x, y)` to the canonical mapsquare id used in keys.json.
#[must_use]
pub const fn mapsquare(x: u32, y: u32) -> u32 {
    (x << 8) | y
}

/// Per-tile terrain data. Fields with `i32` defaults reflect "opcode wasn't emitted for
/// this tile" — most tiles don't emit all opcodes.
#[derive(Debug, Default, Clone, Copy)]
pub struct Tile {
    /// Height in Jagex units (1/8 of a render unit). If the on-disk stream emitted an
    /// explicit height (opcode 1), that's used; otherwise level 0 is filled from a
    /// seeded perlin noise function and levels 1..=3 are `parent - 240`.
    pub height: i32,
    /// Map flag bits (bit 0 = blocked, bit 1 = bridge, bit 2 = roof, bit 3 = bridge-block).
    pub mapflags: u8,
    /// Underlay floor id (FluType) + 1; 0 = no underlay.
    pub underlay: u8,
    /// Overlay floor id (FloType) + 1; 0 = no overlay.
    pub overlay: u8,
    /// Overlay shape (0..=11); only meaningful when `overlay > 0`.
    pub overlay_shape: u8,
    /// Overlay rotation (0..=3); only meaningful when `overlay > 0`. The Java client adds
    /// an additional "base rotation" to this when streaming instance rebuilds — we store
    /// the unrotated component since instance rotation is a runtime/world-builder
    /// concern, not a decode concern.
    pub overlay_rotation: u8,
}

/// One placed loc instance within a region.
#[derive(Debug, Clone, Copy)]
pub struct LocPlacement {
    pub id: i32,
    /// 0..=3
    pub level: u8,
    /// 0..=63 (local tile x within the region)
    pub x: u8,
    /// 0..=63 (local tile z within the region)
    pub z: u8,
    /// LocType shape index (0..=22).
    pub shape: u8,
    /// 0..=3
    pub rotation: u8,
}

/// One decoded region: every tile + every loc placement.
#[derive(Debug, Clone)]
pub struct Region {
    /// `tiles[level][x][z]`
    pub tiles: Box<[[[Tile; REGION_SIZE]; REGION_SIZE]; REGION_LEVELS]>,
    pub locs: Vec<LocPlacement>,
}

impl Region {
    /// Decode a terrain stream and an optional (already-decrypted) loc stream. Default
    /// tile heights (where the stream emitted no opcode 1) are filled from perlin noise
    /// using the region's `(x, y)` coords.
    ///
    /// `loc_bytes = None` for regions with no loc file (rare; usually instance areas).
    #[must_use]
    pub fn decode(
        region_x: u32,
        region_y: u32,
        terrain_bytes: &[u8],
        loc_bytes: Option<&[u8]>,
    ) -> Self {
        let tiles = decode_terrain(terrain_bytes, region_x, region_y);
        let locs = loc_bytes.map(decode_locs).unwrap_or_default();
        Self { tiles, locs }
    }
}

fn decode_terrain(
    bytes: &[u8],
    region_x: u32,
    region_z: u32,
) -> Box<[[[Tile; REGION_SIZE]; REGION_SIZE]; REGION_LEVELS]> {
    let mut tiles: Box<[[[Tile; REGION_SIZE]; REGION_SIZE]; REGION_LEVELS]> =
        Box::new([[[Tile::default(); REGION_SIZE]; REGION_SIZE]; REGION_LEVELS]);
    // Tracks tiles that received an explicit height from the stream, so the default-height
    // pass doesn't overwrite them.
    let mut explicit = [[[false; REGION_SIZE]; REGION_SIZE]; REGION_LEVELS];

    let mut p = Packet::from_vec(bytes.to_vec());
    for level in 0..REGION_LEVELS {
        for x in 0..REGION_SIZE {
            for z in 0..REGION_SIZE {
                decode_tile(&mut p, &mut tiles[level][x][z], &mut explicit[level][x][z]);
            }
        }
    }

    // Default heights (mirrors ClientBuild::loadGroundSquare's opcode-0 branch).
    let base_x = region_x as i32 * REGION_SIZE as i32;
    let base_z = region_z as i32 * REGION_SIZE as i32;
    for level in 0..REGION_LEVELS {
        for x in 0..REGION_SIZE {
            for z in 0..REGION_SIZE {
                if explicit[level][x][z] {
                    continue;
                }
                let h = if level == 0 {
                    -perlin::perlin(
                        base_x + x as i32 + 932_731,
                        base_z + z as i32 + 556_238,
                    ) * 8
                } else {
                    tiles[level - 1][x][z].height - 240
                };
                tiles[level][x][z].height = h;
            }
        }
    }

    tiles
}

fn decode_tile(p: &mut Packet, tile: &mut Tile, explicit_height: &mut bool) {
    loop {
        let code = p.g1();
        if code == 0 {
            return;
        }
        if code == 1 {
            // Explicit height; the magic `1 -> 0` lets the encoder emit height 0 without
            // colliding with the opcode-0 terminator.
            let mut h = p.g1();
            if h == 1 {
                h = 0;
            }
            tile.height = h;
            *explicit_height = true;
            return;
        }
        if code <= 49 {
            tile.overlay = p.g1() as u8;
            tile.overlay_shape = ((code - 2) / 4) as u8;
            tile.overlay_rotation = ((code - 2) & 0x3) as u8;
        } else if code <= 81 {
            tile.mapflags = (code - 49) as u8;
        } else {
            tile.underlay = (code - 81) as u8;
        }
    }
}

fn decode_locs(bytes: &[u8]) -> Vec<LocPlacement> {
    let mut out = Vec::new();
    let mut p = Packet::from_vec(bytes.to_vec());
    let mut loc_id: i32 = -1;
    loop {
        let id_delta = p.gsmart();
        if id_delta == 0 {
            return out;
        }
        loc_id += id_delta;
        let mut packed_pos: i32 = 0;
        loop {
            let pos_delta = p.gsmart();
            if pos_delta == 0 {
                break;
            }
            packed_pos += pos_delta - 1;
            let z = (packed_pos & 0x3F) as u8;
            let x = ((packed_pos >> 6) & 0x3F) as u8;
            let level = ((packed_pos >> 12) & 0x3) as u8;
            let info = p.g1();
            let shape = (info >> 2) as u8;
            let rotation = (info & 0x3) as u8;
            out.push(LocPlacement { id: loc_id, level, x, z, shape, rotation });
        }
    }
}

// ───── keys.json ─────────────────────────────────────────────────────────────

/// One XTEA-encrypted loc-file entry from `keys.json`.
#[derive(Debug, Clone, Deserialize)]
pub struct KeyEntry {
    pub archive: u8,
    pub group: u32,
    pub name_hash: i32,
    pub name: String,
    pub mapsquare: u32,
    pub key: XteaKey,
}

/// Map of region `mapsquare` id → XTEA key for its loc file.
#[derive(Debug, Default, Clone)]
pub struct XteaKeys {
    pub by_mapsquare: HashMap<u32, XteaKey>,
}

impl XteaKeys {
    /// Load `cache/keys.json`. Returns IO/parse errors verbatim.
    pub fn load(path: &Path) -> std::io::Result<Self> {
        let bytes = std::fs::read(path)?;
        let entries: Vec<KeyEntry> = serde_json::from_slice(&bytes)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let mut by_mapsquare = HashMap::with_capacity(entries.len());
        for e in entries {
            by_mapsquare.insert(e.mapsquare, e.key);
        }
        Ok(Self { by_mapsquare })
    }

    #[must_use]
    pub fn get(&self, x: u32, y: u32) -> Option<&XteaKey> {
        self.by_mapsquare.get(&mapsquare(x, y))
    }
}
