// custom — `::mem` heap breakdown (dev tooling, not part of the gamepack).
//
// Walks the big retained-byte holders and prints one line each to stderr so
// memory work is driven by data instead of guesses. Model/anim sizes are
// estimates from element counts (labeled "est"); byte buffers are exact.

#![allow(dead_code)]

use crate::dash3d::model_lit::ModelLit;
use crate::dash3d::model_unlit::ModelUnlit;

fn kb(bytes: usize) -> String {
    format!("{:>8}k", bytes / 1024)
}

// Rough live-size of an unlit model: 3 vertex coord arrays + face index /
// colour arrays + the common optional per-face attributes.
fn model_unlit_est(m: &ModelUnlit) -> usize {
    let v = m.point_x.len();
    let f = m.face_vertex_a.len();
    v * 12 + f * 20
}

// Lit models add per-corner colours and normals.
fn model_lit_est(m: &ModelLit) -> usize {
    let v = m.point_x.len();
    let f = m.face_vertex_a.len();
    v * 24 + f * 36
}

// Called once per mainloop tick while in-game: dumps shortly after login
// settles, then every 60s, so the breakdown lands in the log without
// needing the ::mem cheat typed.
pub fn tick() {
    use std::sync::atomic::{AtomicI32, Ordering};
    static TICKS: AtomicI32 = AtomicI32::new(0);
    let t = TICKS.fetch_add(1, Ordering::Relaxed) + 1;
    if t == 500 || t % 3000 == 0 {
        report();
    }
}

pub fn report() {
    let mut total_tracked = 0usize;
    let mut line = |label: &str, bytes: usize, detail: String| {
        total_tracked += bytes;
        eprintln!("[mem] {} {label:<24} {detail}", kb(bytes));
    };

    // ── JS5 archives: packed (compressed) + unpacked (raw) bytes ───────
    {
        let reg = crate::js5::js5_net::LOADERS.lock().unwrap();
        for slot in reg.iter().flatten() {
            let mut packed = 0usize;
            let mut packed_n = 0usize;
            for g in slot.base.packed.iter().flatten() {
                packed += g.len();
                packed_n += 1;
            }
            let mut unpacked = 0usize;
            let mut unpacked_n = 0usize;
            for g in slot.base.unpacked.iter().flatten() {
                for f in g.iter().flatten() {
                    unpacked += f.len();
                    unpacked_n += 1;
                }
            }
            if packed + unpacked == 0 {
                continue;
            }
            line(
                &format!("js5 archive {}", slot.archive),
                packed + unpacked,
                format!("packed {packed_n} groups {}, unpacked {unpacked_n} files {}",
                        kb(packed).trim(), kb(unpacked).trim()),
            );
        }
    }

    // ── Decoded model caches ────────────────────────────────────────────
    {
        let c = crate::config::loc_type::MC1.lock().unwrap();
        let b: usize = c.values().map(|m| model_unlit_est(m)).sum();
        line("loc MC1 (unlit)", b, format!("est, {} models", c.len()));
    }
    {
        let c = crate::config::loc_type::MC2.lock().unwrap();
        let b: usize = c.values()
            .map(|m| match m {
                crate::config::loc_type::CachedLocModel::UnlitProto(u) => model_unlit_est(u),
                crate::config::loc_type::CachedLocModel::Lit(l) => model_lit_est(l),
            })
            .sum();
        line("loc MC2 (mixed)", b, format!("est, {} models", c.len()));
    }
    {
        let c = crate::config::loc_type::MC3.lock().unwrap();
        let b: usize = c.values().map(|m| model_lit_est(m)).sum();
        line("loc MC3 (lit)", b, format!("est, {} models", c.len()));
    }
    {
        let c = crate::config::obj_type::MODEL_CACHE.lock().unwrap();
        let b: usize = c.values().map(|m| model_lit_est(m)).sum();
        line("obj models", b, format!("est, {} models", c.len()));
    }
    {
        let c = crate::config::npc_type::MODEL_CACHE.lock().unwrap();
        let b: usize = c.values().map(|m| model_lit_est(m)).sum();
        line("npc models", b, format!("est, {} models", c.len()));
    }
    {
        let c = crate::config::spot_type::MODEL_CACHE.lock().unwrap();
        let b: usize = c.values().map(|m| model_lit_est(m)).sum();
        line("spot models", b, format!("est, {} models", c.len()));
    }
    {
        let c = crate::config::if_type::MODEL_CACHE.lock().unwrap();
        let b: usize = c.map.values().map(|m| model_lit_est(m)).sum();
        line("if models", b, format!("est, {} models", c.map.len()));
    }

    // ── Sprite caches (Pix32 = 4 bytes/px) ──────────────────────────────
    {
        let c = crate::config::if_type::SPRITE_CACHE.lock().unwrap();
        let b: usize = c.map.values().map(|p| p.data.len() * 4).sum();
        line("if sprites", b, format!("{} sprites", c.map.len()));
    }
    {
        let c = crate::config::obj_type::SPRITE_CACHE.lock().unwrap();
        let b: usize = c.values().map(|p| p.data.len() * 4).sum();
        line("obj icon sprites", b, format!("{} sprites", c.len()));
    }

    // ── Textures (128x128 RGB i32 texels) ───────────────────────────────
    {
        let s = crate::dash3d::texture_manager::STORE.lock().unwrap();
        let b: usize = s.textures.values().flatten()
            .filter_map(|t| t.texels.as_ref())
            .map(|t| t.len() * 4)
            .sum();
        line("texture texels", b, format!("{} textures", s.textures.values().flatten().count()));
    }

    // ── Framebuffers / pixel planes ─────────────────────────────────────
    {
        let p = crate::graphics::pix2d::STATE.lock().unwrap();
        line("pix2d plane", p.pixels.len() * 4, format!("{}x{}", p.width, p.height));
    }

    // ── World / scene ───────────────────────────────────────────────────
    {
        let s = crate::client_build::STATE.lock().unwrap();
        line("client_build locs", std::mem::size_of_val(s.locs.as_slice()),
             format!("{} locs", s.locs.len()));
    }
    {
        use crate::dash3d::scene_tile::Square;
        let w = crate::scene::WORLD_CACHE.lock().unwrap();
        if let Some(world) = w.world.as_ref() {
            // The Option<Square> slots are inline in the level/x/z vecs.
            let mut slot_bytes = 0usize;
            let mut squares = 0usize;
            let mut ground_b = 0usize;
            let mut grounds = 0usize;
            let ground_bytes = |g: &crate::dash3d::ground::Ground| {
                13 * 24
                    + 4 * (g.vertex_x.len() + g.vertex_y.len() + g.vertex_z.len()
                        + g.face_colour_a.len() + g.face_colour_b.len() + g.face_colour_c.len()
                        + g.face_vertex_a.len() + g.face_vertex_b.len() + g.face_vertex_c.len()
                        + g.face_texture.len())
            };
            for level in &world.squares {
                for col in level {
                    slot_bytes += std::mem::size_of::<Option<Square>>() * col.len();
                    for sq in col.iter().flatten() {
                        squares += 1;
                        let mut cur = Some(sq);
                        while let Some(s) = cur {
                            if let Some(g) = s.ground.as_ref() {
                                grounds += 1;
                                ground_b += ground_bytes(g);
                            }
                            cur = s.linked_square.as_deref();
                        }
                    }
                }
            }
            line("world square slots", slot_bytes,
                 format!("{} squares, Option<Square>={}B", squares, std::mem::size_of::<Option<Square>>()));
            line("world ground meshes", ground_b, format!("{grounds} grounds"));
            let heights: usize = world.groundh.iter()
                .map(|l| l.iter().map(|c| c.len() * 4).sum::<usize>())
                .sum();
            let occl: usize = world.occlusion_cycle.iter()
                .map(|l| l.iter().map(|c| c.len() * 4).sum::<usize>())
                .sum();
            line("world heights+occl", heights + occl, String::new());
        }
    }

    // ── Anims ───────────────────────────────────────────────────────────
    {
        let (bases, base_b, frames, frame_b) = crate::dash3d::anim_frame_set::cache_stats();
        line("anim bases", base_b, format!("est, {bases} bases"));
        line("anim frames", frame_b, format!("est, {frames} frames"));
    }

    // ── Title screen leftovers (Java frees these on login) ─────────────
    {
        let t = crate::title_screen::STATE.lock().unwrap();
        let pix32 = |p: &Option<crate::graphics::pix32::Pix32>| {
            p.as_ref().map_or(0, |p| p.data.len() * 4)
        };
        let b = pix32(&t.title_back) + pix32(&t.title_back2)
            + (t.flame_gradient.len() + t.flame_gradient2.len()
                + t.flame_buffer0.len() + t.flame_buffer1.len()
                + t.flame_buffer2.len() + t.flame_buffer3.len()) * 4
            + t.sl_back.as_ref().map_or(0, |v| v.iter().map(|p| p.data.len() * 4).sum())
            + t.runes.as_ref().map_or(0, |v| v.iter().map(|p| p.data.len()).sum());
        line("title screen", b, format!("backs+flames+runes, open={}", t.open));
    }

    let heap = crate::perf::HEAP_BYTES.load(std::sync::atomic::Ordering::Relaxed);
    eprintln!("[mem] {} tracked above", kb(total_tracked));
    eprintln!("[mem] {} total live heap (untracked: {})",
              kb(heap), kb(heap.saturating_sub(total_tracked)));
}
