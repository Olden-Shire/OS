//! Whole-world map, baked at startup (re-baked only when the `.jm2` maps
//! change). Two products, both cached to disk:
//!
//! * a single low-res **overview** composite (1px/tile) for the zoomed-out view;
//! * a **tile store** of full-detail per-region images (416², 4px/tile, with
//!   floors + walls + scenery + map-function icons) — read on demand when zoomed
//!   in, so panning never rebuilds a World on the GUI thread (no lag, walls are
//!   present immediately).

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const REGION_TILES: i32 = 64;
const TILE_W: usize = 104 * 4; // 416 — full build-area detail tile
const OVERVIEW_PATH: &str = "cache/worldmap.bin";
const TILES_PATH: &str = "cache/worldmap_tiles.bin";
const MAGIC: u64 = 0x4F53_3157_4D41_5032; // "OS1WMAP2"
const VERSION: u32 = 2;

/// The low-res overview composite + the world-tile origin for positioning.
pub struct WorldMap {
    pub image: Vec<i32>,
    pub w: usize,
    pub h: usize,
    pub min_rx: i32,
    pub min_ry: i32,
    pub max_ry: i32,
}

impl WorldMap {
    /// World-tile span the image covers (for click-mapping back to coords).
    pub fn bounds(&self) -> (i32, i32, i32, i32) {
        let ox = self.min_rx * REGION_TILES;
        let oz = self.min_ry * REGION_TILES;
        (ox, oz, ox + self.w as i32, oz + self.h as i32)
    }
}

/// On-disk store of full-detail region tiles; reads one tile on demand.
pub struct TileStore {
    file: Mutex<std::fs::File>,
    index: HashMap<(u32, u32), u64>, // region → byte offset of its blob
    tile_w: usize,
}

impl TileStore {
    /// Decode region `(rx, ry)`'s detail tile (416² RGB) from disk, or `None`.
    pub fn get(&self, rx: u32, ry: u32) -> Option<(Vec<i32>, usize)> {
        let off = *self.index.get(&(rx, ry))?;
        let len = self.tile_w * self.tile_w * 4;
        let mut buf = vec![0u8; len];
        {
            let mut f = self.file.lock().ok()?;
            f.seek(SeekFrom::Start(off)).ok()?;
            f.read_exact(&mut buf).ok()?;
        }
        let px: Vec<i32> = buf.chunks_exact(4).map(|c| i32::from_le_bytes(c.try_into().unwrap())).collect();
        Some((px, self.tile_w))
    }

    pub fn len(&self) -> usize {
        self.index.len()
    }
}

/// Region list + the bounding box of the populated world.
fn scan_regions(maps_dir: &Path) -> Option<(Vec<(u32, u32, PathBuf)>, i32, i32, i32, i32)> {
    let mut regions = Vec::new();
    let (mut min_rx, mut max_rx, mut min_ry, mut max_ry) = (i32::MAX, i32::MIN, i32::MAX, i32::MIN);
    for entry in std::fs::read_dir(maps_dir).ok()? {
        let path = entry.ok()?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jm2") {
            continue;
        }
        let stem = path.file_stem().and_then(|s| s.to_str())?;
        let mut it = stem.split('_');
        let (Some(rx), Some(ry)) = (it.next(), it.next()) else { continue };
        let (Ok(rx), Ok(ry)) = (rx.parse::<u32>(), ry.parse::<u32>()) else { continue };
        min_rx = min_rx.min(rx as i32);
        max_rx = max_rx.max(rx as i32);
        min_ry = min_ry.min(ry as i32);
        max_ry = max_ry.max(ry as i32);
        regions.push((rx, ry, path));
    }
    if regions.is_empty() {
        return None;
    }
    regions.sort_by_key(|(rx, ry, _)| (*rx, *ry));
    Some((regions, min_rx, max_rx, min_ry, max_ry))
}

/// Fingerprint of every map file (name + size + mtime) — re-bake when it moves.
fn maps_hash(regions: &[(u32, u32, PathBuf)]) -> u64 {
    let mut h = DefaultHasher::new();
    VERSION.hash(&mut h);
    for (rx, ry, path) in regions {
        rx.hash(&mut h);
        ry.hash(&mut h);
        if let Ok(md) = std::fs::metadata(path) {
            md.len().hash(&mut h);
            if let Ok(t) = md.modified() {
                if let Ok(d) = t.duration_since(std::time::UNIX_EPOCH) {
                    d.as_secs().hash(&mut h);
                }
            }
        }
    }
    h.finish()
}

fn load_overview(hash: u64) -> Option<WorldMap> {
    let mut f = std::fs::File::open(OVERVIEW_PATH).ok()?;
    let mut hdr = [0u8; 8 + 4 + 8 + 4 * 3 + 4 * 2];
    f.read_exact(&mut hdr).ok()?;
    let rd_u64 = |o: usize| u64::from_le_bytes(hdr[o..o + 8].try_into().unwrap());
    let rd_u32 = |o: usize| u32::from_le_bytes(hdr[o..o + 4].try_into().unwrap());
    let rd_i32 = |o: usize| i32::from_le_bytes(hdr[o..o + 4].try_into().unwrap());
    if rd_u64(0) != MAGIC || rd_u32(8) != VERSION || rd_u64(12) != hash {
        return None;
    }
    let min_rx = rd_i32(20);
    let min_ry = rd_i32(24);
    let max_ry = rd_i32(28);
    let w = rd_u32(32) as usize;
    let h = rd_u32(36) as usize;
    let mut bytes = Vec::new();
    f.read_to_end(&mut bytes).ok()?;
    if bytes.len() < w * h * 4 {
        return None;
    }
    let image = bytes[..w * h * 4].chunks_exact(4).map(|c| i32::from_le_bytes(c.try_into().unwrap())).collect();
    Some(WorldMap { image, w, h, min_rx, min_ry, max_ry })
}

fn save_overview(map: &WorldMap, hash: u64) {
    let Ok(mut f) = std::fs::File::create(OVERVIEW_PATH) else { return };
    let mut hdr = Vec::with_capacity(40);
    hdr.extend_from_slice(&MAGIC.to_le_bytes());
    hdr.extend_from_slice(&VERSION.to_le_bytes());
    hdr.extend_from_slice(&hash.to_le_bytes());
    hdr.extend_from_slice(&map.min_rx.to_le_bytes());
    hdr.extend_from_slice(&map.min_ry.to_le_bytes());
    hdr.extend_from_slice(&map.max_ry.to_le_bytes());
    hdr.extend_from_slice(&(map.w as u32).to_le_bytes());
    hdr.extend_from_slice(&(map.h as u32).to_le_bytes());
    let _ = f.write_all(&hdr);
    let mut buf = Vec::with_capacity(map.image.len() * 4);
    for &p in &map.image {
        buf.extend_from_slice(&p.to_le_bytes());
    }
    let _ = f.write_all(&buf);
}

/// Tiles-file layout: header, then `count` × (rx,ry) index, then `count` blobs
/// of `tile_w²·4` bytes each (in index order). Open + read the index for `get`.
fn tiles_header_len(count: usize) -> u64 {
    (8 + 4 + 8 + 4 + 4) as u64 + count as u64 * 8
}

fn open_tile_store(hash: u64) -> Option<TileStore> {
    let mut f = std::fs::File::open(TILES_PATH).ok()?;
    let mut hdr = [0u8; 8 + 4 + 8 + 4 + 4];
    f.read_exact(&mut hdr).ok()?;
    if u64::from_le_bytes(hdr[0..8].try_into().unwrap()) != MAGIC
        || u32::from_le_bytes(hdr[8..12].try_into().unwrap()) != VERSION
        || u64::from_le_bytes(hdr[12..20].try_into().unwrap()) != hash
    {
        return None;
    }
    let count = u32::from_le_bytes(hdr[20..24].try_into().unwrap()) as usize;
    let tile_w = u32::from_le_bytes(hdr[24..28].try_into().unwrap()) as usize;
    let mut idx_bytes = vec![0u8; count * 8];
    f.read_exact(&mut idx_bytes).ok()?;
    let blob_len = (tile_w * tile_w * 4) as u64;
    let data_start = tiles_header_len(count);
    let mut index = HashMap::with_capacity(count);
    for (i, c) in idx_bytes.chunks_exact(8).enumerate() {
        let rx = u32::from_le_bytes(c[0..4].try_into().unwrap());
        let ry = u32::from_le_bytes(c[4..8].try_into().unwrap());
        index.insert((rx, ry), data_start + i as u64 * blob_len);
    }
    Some(TileStore { file: Mutex::new(f), index, tile_w })
}

/// Box-downscale the centre 64×64 region of a detail tile (px [80..336)) to a
/// 1px/tile block for the overview.
fn downscale_region(tile: &[i32]) -> Vec<i32> {
    const B: usize = 20 * 4; // 80
    let mut out = vec![0i32; REGION_TILES as usize * REGION_TILES as usize];
    for oy in 0..REGION_TILES as usize {
        for ox in 0..REGION_TILES as usize {
            let (mut r, mut g, mut b) = (0u32, 0u32, 0u32);
            for dy in 0..4 {
                for dx in 0..4 {
                    let p = tile[(B + oy * 4 + dy) * TILE_W + B + ox * 4 + dx];
                    r += ((p >> 16) & 0xFF) as u32;
                    g += ((p >> 8) & 0xFF) as u32;
                    b += (p & 0xFF) as u32;
                }
            }
            out[oy * REGION_TILES as usize + ox] = (((r / 16) << 16) | ((g / 16) << 8) | (b / 16)) as i32;
        }
    }
    out
}

/// Bake (or load) the whole-world overview + detail tile store. `progress(done,
/// total)` fires per region while baking. Re-bakes only when the maps change.
pub fn bake_or_load(content_dir: &str, mut progress: impl FnMut(usize, usize)) -> Option<(WorldMap, TileStore)> {
    let maps_dir = Path::new(content_dir).join("maps");
    let (regions, min_rx, max_rx, min_ry, max_ry) = scan_regions(&maps_dir)?;
    let hash = maps_hash(&regions);
    if let (Some(map), Some(store)) = (load_overview(hash), open_tile_store(hash)) {
        return Some((map, store));
    }
    crate::scene::install_client().ok()?;
    let (mapscene, mapfunction) = crate::scene::load_map_sprites();

    let regions_w = (max_rx - min_rx + 1) as usize;
    let regions_h = (max_ry - min_ry + 1) as usize;
    let w = regions_w * REGION_TILES as usize;
    let h = regions_h * REGION_TILES as usize;
    let mut image = vec![0i32; w * h];

    // Open the tiles file and write header + index up front (blobs stream after,
    // sequentially, through the same handle).
    let count = regions.len();
    let blob_len = TILE_W * TILE_W * 4;
    let zero_blob = vec![0u8; blob_len];
    let mut tf = std::fs::File::create(TILES_PATH).ok();
    if let Some(f) = tf.as_mut() {
        let mut hdr = Vec::new();
        hdr.extend_from_slice(&MAGIC.to_le_bytes());
        hdr.extend_from_slice(&VERSION.to_le_bytes());
        hdr.extend_from_slice(&hash.to_le_bytes());
        hdr.extend_from_slice(&(count as u32).to_le_bytes());
        hdr.extend_from_slice(&(TILE_W as u32).to_le_bytes());
        for (rx, ry, _) in &regions {
            hdr.extend_from_slice(&rx.to_le_bytes());
            hdr.extend_from_slice(&ry.to_le_bytes());
        }
        let _ = f.write_all(&hdr);
    }

    let total = regions.len();
    for (i, (rx, ry, _)) in regions.iter().enumerate() {
        progress(i, total);
        let tile = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::scene::bake_region_detail(content_dir, *rx, *ry, 0, mapscene.as_deref(), mapfunction.as_deref())
        })).ok().flatten();

        match tile {
            Some((px, _)) => {
                // Overview block from the tile's centre region.
                let block = downscale_region(&px);
                let cx0 = (*rx as i32 - min_rx) as usize * REGION_TILES as usize;
                let cy0 = (max_ry - *ry as i32) as usize * REGION_TILES as usize;
                for ry_t in 0..REGION_TILES as usize {
                    let dst = (cy0 + ry_t) * w + cx0;
                    image[dst..dst + REGION_TILES as usize]
                        .copy_from_slice(&block[ry_t * REGION_TILES as usize..(ry_t + 1) * REGION_TILES as usize]);
                }
                // Detail blob to disk.
                if let Some(f) = tf.as_mut() {
                    let mut buf = Vec::with_capacity(blob_len);
                    for p in &px {
                        buf.extend_from_slice(&p.to_le_bytes());
                    }
                    let _ = f.write_all(&buf);
                }
            }
            None => {
                // Keep blob alignment so the index stays valid.
                if let Some(f) = tf.as_mut() {
                    let _ = f.write_all(&zero_blob);
                }
            }
        }
    }
    progress(total, total);
    drop(tf);

    let map = WorldMap { image, w, h, min_rx, min_ry, max_ry };
    save_overview(&map, hash);
    let store = open_tile_store(hash)?;
    Some((map, store))
}
