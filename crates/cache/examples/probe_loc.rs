//! Probe the locs at a world tile: list placements, dump their LocType
//! configs, and dump per-face model data (colour / render type / alpha /
//! texture tables). Debugging aid for renderer divergences.
//!
//! Usage: `cargo run --example probe_loc -p cache -- [world_x world_z [level]]`
//! Default: 3215 3219 0 (Lumbridge castle rug corridor).

use std::collections::BTreeSet;
use std::path::Path;

use cache::configs::Configs;
use cache::maps::XteaKeys;
use cache::model::Model;
use cache::Cache;

const MODELS_ARCHIVE: u8 = 7;

fn main() {
    let args: Vec<i32> = std::env::args().skip(1).filter_map(|s| s.parse().ok()).collect();
    let wx = *args.first().unwrap_or(&3215);
    let wz = *args.get(1).unwrap_or(&3219);
    let lvl = *args.get(2).unwrap_or(&0) as u8;

    let dir = Path::new("cache");
    let keys = XteaKeys::load(&dir.join("keys.json")).expect("keys.json");
    let mut cache = Cache::open(dir).expect("open cache/");
    let configs = Configs::load(&mut cache).expect("configs");

    let (rx, rz) = ((wx >> 6) as u32, (wz >> 6) as u32);
    let (lx, lz) = ((wx & 63) as u8, (wz & 63) as u8);
    println!("world ({wx},{wz}) lvl {lvl} -> region ({rx},{rz}) local ({lx},{lz})");

    let region = cache
        .region(rx, rz, &keys)
        .expect("region io")
        .expect("region missing");

    // Scan a window around the tile.
    let mut ids = BTreeSet::new();
    for p in &region.locs {
        if p.level != lvl { continue; }
        let dx = (p.x as i32 - lx as i32).abs();
        let dz = (p.z as i32 - lz as i32).abs();
        if dx <= 3 && dz <= 6 {
            println!(
                "  loc id={} at local ({},{}) world ({},{}) shape={} rot={}",
                p.id, p.x, p.z,
                (rx as i32 * 64) + p.x as i32, (rz as i32 * 64) + p.z as i32,
                p.shape, p.rotation
            );
            ids.insert(p.id);
        }
    }

    for id in ids {
        let Some(lt) = configs.locs.get(&id) else {
            println!("== loc {id}: no config ==");
            continue;
        };
        println!(
            "== loc {id} name={:?} models={:?} shapes={:?} sharelight={} wallwidth={} \
             ambient={} contrast={} recol={:?}->{:?} retex={:?}->{:?} skew={} anim={}",
            lt.name, lt.models, lt.shapes, lt.sharelight, lt.wallwidth,
            lt.ambient, lt.contrast, lt.recol_s, lt.recol_d, lt.retex_s, lt.retex_d,
            lt.skew_type, lt.anim
        );
        for &mid in &lt.models {
            let raw = match cache.read_group(MODELS_ARCHIVE, (mid & 0xFFFF) as u32) {
                Ok(Some(b)) => b,
                other => { println!("  model {mid}: unavailable ({other:?})"); continue; }
            };
            let m = Model::decode(&raw);
            println!(
                "  model {mid}: points={} faces={} num_t={} priority={} \
                 has_frt={} has_alpha={} has_texid={} has_texaxis={} has_trt={}",
                m.num_points, m.num_faces, m.num_t, m.priority,
                m.face_render_type.is_some(), m.face_alpha.is_some(),
                m.face_texture_id.is_some(), m.face_texture_axis.is_some(),
                m.texture_render_type.is_some()
            );
            for f in 0..(m.num_faces as usize) {
                let col = m.face_colour[f] as u16;
                let frt = m.face_render_type.as_ref().map_or(0, |v| v[f]);
                let alpha = m.face_alpha.as_ref().map_or(0, |v| v[f]);
                let tex = m.face_texture_id.as_ref().map_or(-1, |v| v[f]);
                let axis = m.face_texture_axis.as_ref().map_or(-1, |v| v[f]);
                println!(
                    "    f{f}: colour={col} (h={} s={} l={}) frt={frt} alpha={alpha} tex={tex} axis={axis}",
                    (col >> 10) & 0x3F, (col >> 7) & 0x7, col & 0x7F
                );
            }
            if m.num_t > 0 {
                println!(
                    "    tex tables: trt={:?} p={:?} m={:?} n={:?}",
                    m.texture_render_type, m.face_texture_p, m.face_texture_m, m.face_texture_n
                );
            }
        }
    }
}
