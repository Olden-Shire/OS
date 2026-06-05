//! Decode every model in archive 7. Failure modes the test catches: unknown face-order
//! codes (Ob2 / Ob3 mismatch), trailer offset miscomputation (panic on OOB read), and
//! trailing-bytes asserts in the texture decode block.

use std::path::PathBuf;

use cache::model::Model;
use cache::{Cache, MODELS_ARCHIVE};

fn cache_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../cache")
}

#[test]
fn decodes_every_model() {
    let mut c = Cache::open(&cache_dir()).expect("open cache");
    let groups: Vec<i32> = c.index(MODELS_ARCHIVE).group_ids.clone();

    let mut ob2_count = 0u32;
    let mut ob3_count = 0u32;
    let mut total_points = 0u64;
    let mut total_faces = 0u64;
    let mut max_points = 0;
    let mut max_faces = 0;

    for &gid in &groups {
        let bytes = c
            .read_group(MODELS_ARCHIVE, gid as u32)
            .expect("read_group")
            .expect("model missing");
        let n = bytes.len();
        let is_ob3 = n >= 2 && bytes[n - 1] == 0xFF && bytes[n - 2] == 0xFF;
        if is_ob3 { ob3_count += 1; } else { ob2_count += 1; }

        let m = Model::decode(&bytes);
        total_points += m.num_points as u64;
        total_faces += m.num_faces as u64;
        if m.num_points > max_points { max_points = m.num_points; }
        if m.num_faces > max_faces { max_faces = m.num_faces; }
    }

    eprintln!("  models loaded:     {}", groups.len());
    eprintln!("    Ob3 format:      {ob3_count}");
    eprintln!("    Ob2 format:      {ob2_count}");
    eprintln!("  total vertices:    {total_points}");
    eprintln!("  total faces:       {total_faces}");
    eprintln!("  largest model:     {max_points} verts / {max_faces} faces");
}
