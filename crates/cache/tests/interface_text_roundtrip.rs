//! Whole-interface .if round-trip: decode_group → text → encode_group must
//! reproduce every component's bytes. Reports per-group conversion.
use std::path::PathBuf;
use cache::{Cache, INTERFACES_ARCHIVE};
use cache::content::interface_text;

fn cache_dir() -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../cache") }

#[test]
fn interfaces_if_roundtrip() {
    let mut c = Cache::open(&cache_dir()).expect("open");
    let groups = c.index(INTERFACES_ARCHIVE).group_ids.clone();
    let (mut total, mut converted) = (0u64, 0u64);
    let mut fails = Vec::new();
    for g in groups {
        let files = c.read_files(INTERFACES_ARCHIVE, g as u32).ok().flatten().unwrap_or_default();
        if files.is_empty() { continue; }
        total += 1;
        let comps: Vec<(i32, Vec<u8>)> = files.iter().map(|(f, b)| (*f, b.clone())).collect();
        match interface_text::decode_group(g as u32, &comps) {
            Some(text) => {
                let re = interface_text::encode_group(g as u32, &text).expect("re-encode");
                assert_eq!(re.len(), comps.len(), "if {g}: component count");
                for (a, b) in comps.iter().zip(&re) { assert_eq!(a, b, "if {g}: bytes"); }
                converted += 1;
            }
            None => { if fails.len() < 12 { fails.push(g); } }
        }
    }
    let pct = converted * 100 / total.max(1);
    eprintln!("interfaces: {converted}/{total} groups → .if ({pct}%)  fallback e.g. {fails:?}");
    assert_eq!(pct, 100, "all interfaces must convert to .if");
}
