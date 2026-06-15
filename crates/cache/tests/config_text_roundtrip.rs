//! Byte-exact round-trip of the readable config text codec across every
//! record of each supported config group: decode → text → encode must
//! reproduce the original bytes (the codec self-verifies, so a Some()
//! result is already proof; we additionally assert coverage so a
//! regression that silently drops everything to the .dat fallback fails
//! the test).

use std::path::PathBuf;

use cache::content::config_text;
use cache::{CONFIG_ARCHIVE, Cache};

fn cache_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../cache")
}

/// Every config group that has a text schema: each record must either
/// round-trip byte-exact or (safely) fall back. Reports per-type coverage
/// and asserts the high-volume types convert nearly everything.
#[test]
fn all_config_text_roundtrips_byte_exact() {
    use cache::config::group;
    let mut c = Cache::open(&cache_dir()).expect("open cache");

    // (group id, min conversion ratio %) — types we expect ~full coverage.
    let groups: &[(u32, u32)] = &[
        (group::OBJ, 99),
        (group::LOC, 90),
        (group::NPC, 90),
        (group::SEQ, 99),
        (group::FLO, 99),
        (group::FLU, 99),
        (group::IDK, 99),
        (group::INV, 99),
        (group::SPOT, 99),
        (group::VARBIT, 99),
        (group::VARP, 99),
        (group::ENUM, 0), // int-maps convert; string-maps fall back — no floor
    ];

    for &(gid, min_pct) in groups {
        let files = c
            .read_files(CONFIG_ARCHIVE, gid)
            .expect("read group")
            .unwrap_or_default();
        if files.is_empty() {
            continue;
        }
        let (schema, kind) = config_text::schema_for_group(gid).expect("schema");
        let mut converted = 0usize;
        let mut sample = Vec::new();
        let refs = config_text::ConfigRefs::default();
        for (fid, bytes) in &files {
            if let Some(text) = config_text::decode(schema, kind, *fid as u32, bytes, &refs) {
                let re = config_text::encode(schema, &text, &refs).expect("re-encode");
                assert_eq!(&re, bytes, "{kind} {fid} re-encode mismatch");
                converted += 1;
            } else if sample.len() < 8 {
                sample.push(*fid);
            }
        }
        let pct = (converted * 100 / files.len()) as u32;
        eprintln!(
            "{kind:>7}: {converted:>5}/{:<5} ({pct:>3}%) exact   fallback e.g. {sample:?}",
            files.len()
        );
        assert!(
            pct >= min_pct,
            "{kind}: only {pct}% converted (floor {min_pct}%)"
        );
    }
}
