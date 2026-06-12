//! Byte-exact CS2 codec proof over the whole vanilla cache: for every clientscript
//! (archive 12), `decode → disassemble → assemble → encode` must reproduce the original
//! decompressed group bytes. Uses empty name maps so the test exercises the codec itself,
//! independent of any `.pack` naming.

use std::path::PathBuf;

use cache::cs2::ClientScript;
use cache::cs2_asm::{assemble, disassemble, NameMaps};
use cache::Cache;

const CLIENTSCRIPTS_ARCHIVE: u8 = 12;

fn cache_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../cache")
}

#[test]
fn every_clientscript_round_trips_byte_identical() {
    let mut cache = Cache::open(&cache_dir()).expect("open cache");
    let names = NameMaps::new();
    let group_ids: Vec<i32> = cache.index(CLIENTSCRIPTS_ARCHIVE).group_ids.clone();
    assert!(!group_ids.is_empty(), "clientscripts archive is empty");

    let mut checked = 0u64;
    for gid in group_ids {
        let gid = gid as u32;
        let original = cache
            .read_group(CLIENTSCRIPTS_ARCHIVE, gid)
            .expect("io")
            .expect("group present");

        let script = ClientScript::decode(&original)
            .unwrap_or_else(|| panic!("script {gid} failed to decode ({} bytes)", original.len()));

        // The decoder itself must be lossless before we even involve the text form.
        assert_eq!(
            script.encode(),
            original,
            "script {gid}: encode(decode) is not byte-identical"
        );

        let text = disassemble(&script, &names);
        let back = assemble(&text, &names)
            .unwrap_or_else(|e| panic!("script {gid}: assemble failed: {e}\n{text}"));
        assert_eq!(
            back.encode(),
            original,
            "script {gid}: decompile → recompile is not byte-identical"
        );
        checked += 1;
    }
    eprintln!("byte-identical clientscripts verified: {checked}");
}
