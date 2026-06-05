//! Decode every IfType subcomponent in archive 3.

use std::path::PathBuf;

use cache::iftype::IfType;
use cache::{Cache, INTERFACES_ARCHIVE};

fn cache_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../cache")
}

#[test]
fn decodes_every_interface_component() {
    let mut c = Cache::open(&cache_dir()).expect("open cache");
    let groups: Vec<i32> = c.index(INTERFACES_ARCHIVE).group_ids.clone();

    let mut parents = 0;
    let mut components = 0;
    let mut v3_count = 0;
    let mut hooked = 0;
    for &gid in &groups {
        let files = c
            .read_files(INTERFACES_ARCHIVE, gid as u32)
            .expect("read_files")
            .expect("group missing");
        parents += 1;
        for (sub, bytes) in files {
            let parent_id = (gid << 16) | sub;
            let if_ = IfType::decode(parent_id, sub, &bytes);
            components += 1;
            if if_.v3 {
                v3_count += 1;
            }
            if if_.hashook {
                hooked += 1;
            }
        }
    }
    eprintln!("  parent interfaces:  {parents}");
    eprintln!("  total components:   {components}");
    eprintln!("  v3 format:          {v3_count}");
    eprintln!("  with script hooks:  {hooked}");
    assert!(components > 1000);
}
