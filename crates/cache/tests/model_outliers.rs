//! For every model in the cache, compute the bounding box of vertices REFERENCED by
//! faces, vs the box of ALL vertices, and flag models where they diverge significantly.
//! Goal: detect whether any model has outlier vertices (decoder bug or genuine cache
//! garbage) that could produce sliver triangles in the viewer.

#[test]
fn find_models_with_vertex_outliers() {
    let path = std::path::Path::new("../../cache");
    if !path.join("main_file_cache.dat2").exists() {
        eprintln!("skip: no cache");
        return;
    }
    let mut c = cache::Cache::open(path).unwrap();
    let group_ids: Vec<u32> = c
        .index(7)
        .group_ids
        .iter()
        .map(|&g| g as u32)
        .collect();
    let mut worst: Vec<(u32, i32, i32)> = Vec::new();
    for gid in group_ids {
        let bytes = match c.read_group(7, gid) {
            Ok(Some(b)) if !b.is_empty() => b,
            _ => continue,
        };
        let m = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            cache::model::Model::decode(&bytes)
        })) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if m.num_points == 0 || m.num_faces == 0 {
            continue;
        }
        // Range across all vertices.
        let max_abs_all = m
            .point_x
            .iter()
            .chain(m.point_y.iter())
            .chain(m.point_z.iter())
            .map(|v| v.abs())
            .max()
            .unwrap_or(0);
        // Range across only vertices referenced by faces.
        let mut max_abs_used = 0i32;
        for f in 0..m.num_faces as usize {
            for &vi in &[m.face_vertex_a[f], m.face_vertex_b[f], m.face_vertex_c[f]] {
                let v = vi as usize;
                if v < m.point_x.len() {
                    max_abs_used = max_abs_used
                        .max(m.point_x[v].abs())
                        .max(m.point_y[v].abs())
                        .max(m.point_z[v].abs());
                }
            }
        }
        if max_abs_all > 5_000 || max_abs_used > 5_000 {
            worst.push((gid, max_abs_all, max_abs_used));
        }
    }
    worst.sort_by_key(|(_, m, _)| -m);
    for (gid, m_all, m_used) in worst.iter().take(20) {
        eprintln!("model {gid:>5}: all-vert max abs {m_all:>10}, used max abs {m_used:>10}");
    }
}
