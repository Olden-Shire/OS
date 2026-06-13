//! Pack a Content tree back to a cache and verify every group's raw bytes
//! match the original cache. Exit 0 = CRC-identical.
use std::path::PathBuf;
use cache::Cache;
use cache::content::pack;

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let src = std::env::args().nth(1).expect("usage: verify_content <content_dir>");
    let src = root.join(src);
    let repacked = root.join("target/verify_repacked");
    eprintln!("packing {} → {}", src.display(), repacked.display());
    pack(&src, &repacked).expect("pack");

    let mut orig = Cache::open(&root.join("cache")).expect("open orig");
    let mut new = Cache::open(&repacked).expect("open repacked");
    let mut groups = 0u64;
    let mut mism = 0u64;
    for archive in 0..cache::ARCHIVE_COUNT {
        for gid in orig.index(archive).group_ids.clone() {
            let a = orig.read_raw(archive, gid as u32).expect("orig").expect("orig grp");
            let b = new.read_raw(archive, gid as u32).expect("new").unwrap_or_default();
            groups += 1;
            if a != b {
                mism += 1;
                if mism <= 10 { eprintln!("  MISMATCH archive {archive} group {gid}"); }
            }
        }
    }
    eprintln!("{groups} groups checked, {mism} mismatches");
    if mism == 0 { eprintln!("CRC-IDENTICAL"); } else { std::process::exit(1); }
}
