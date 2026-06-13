//! Byte-exact decode→encode coverage for IfType across every interface
//! component in the cache. Reports how many reproduce exactly (the rest
//! safely fall back to .dat in the text codec).
use std::path::PathBuf;
use cache::{Cache, INTERFACES_ARCHIVE};
use cache::iftype::IfType;

fn cache_dir() -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../cache") }

#[test]
fn iftype_encode_roundtrips() {
    let mut c = Cache::open(&cache_dir()).expect("open");
    let groups = c.index(INTERFACES_ARCHIVE).group_ids.clone();
    let (mut total, mut exact, mut v3, mut v1) = (0u64, 0u64, 0u64, 0u64);
    let mut fail_types = std::collections::BTreeMap::new();
    for g in groups {
        let files = c.read_files(INTERFACES_ARCHIVE, g as u32).ok().flatten().unwrap_or_default();
        for (fid, bytes) in &files {
            if bytes.is_empty() { continue; }
            total += 1;
            let parent = ((g as i32) << 16) | (*fid);
            let t = IfType::decode(parent, *fid, bytes);
            if t.v3 { v3 += 1; } else { v1 += 1; }
            match t.encode() {
                Some(b) if b == *bytes => exact += 1,
                _ => { *fail_types.entry((t.v3, t.type_, t.button_type)).or_insert(0u32) += 1; }
            }
        }
    }
    let pct = exact * 100 / total.max(1);
    eprintln!("IfType: {exact}/{total} byte-exact ({pct}%)  [v1={v1} v3={v3}]");
    eprintln!("fallback (v3,type,btn)→count: {fail_types:?}");
    assert_eq!(pct, 100, "interface components must round-trip byte-exact");
}
