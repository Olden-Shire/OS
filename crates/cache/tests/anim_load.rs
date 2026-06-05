//! Decode every AnimFrameSet in archive 0 and every AnimBase referenced by them.

use std::collections::HashMap;
use std::path::PathBuf;

use cache::anim::{AnimBase, AnimFrameSet};
use cache::{ANIMS_ARCHIVE, Cache};

fn cache_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../cache")
}

#[test]
fn decodes_every_anim_frame_set() {
    let mut c = Cache::open(&cache_dir()).expect("open cache");
    let group_ids: Vec<i32> = c.index(ANIMS_ARCHIVE).group_ids.clone();
    let mut base_cache: HashMap<i32, AnimBase> = HashMap::new();

    let mut total_frame_sets = 0;
    let mut total_frames = 0;
    let mut max_frames_in_set = 0;
    for &gid in &group_ids {
        let set = AnimFrameSet::load(&mut c, gid as u32, &mut base_cache).expect("load");
        total_frame_sets += 1;
        let n = set.frames.iter().filter(|f| f.is_some()).count();
        total_frames += n;
        if n > max_frames_in_set {
            max_frames_in_set = n;
        }
    }

    eprintln!("  frame sets loaded: {total_frame_sets}");
    eprintln!("  total frames:      {total_frames}");
    eprintln!("  unique anim bases: {}", base_cache.len());
    eprintln!("  most frames in a set: {max_frames_in_set}");

    assert!(total_frame_sets > 0);
    assert!(total_frames > 1000);
    assert!(!base_cache.is_empty());
}
