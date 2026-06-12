//! Structured-decompiler proof over the whole vanilla cache: every clientscript must
//! lift to IR (`cs2_decompile`), recompile (`cs2_compile`), and encode back to the
//! original group bytes exactly. This is the ratchet for the decompiler — any change
//! to the lifter, the structurer, or codegen that breaks a canonical shape fails here.

use std::collections::BTreeMap;
use std::path::PathBuf;

use cache::cs2::ClientScript;
use cache::cs2_compile::compile;
use cache::cs2_decompile::lift;
use cache::cs2_sig::analyze_all;
use cache::Cache;

const CLIENTSCRIPTS_ARCHIVE: u8 = 12;

fn cache_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../cache")
}

#[test]
fn every_clientscript_decompiles_and_recompiles_byte_identical() {
    let mut cache = Cache::open(&cache_dir()).expect("open cache");
    let group_ids: Vec<i32> = cache.index(CLIENTSCRIPTS_ARCHIVE).group_ids.clone();
    assert!(!group_ids.is_empty(), "clientscripts archive is empty");

    let mut scripts: BTreeMap<u32, ClientScript> = BTreeMap::new();
    let mut originals: BTreeMap<u32, Vec<u8>> = BTreeMap::new();
    for gid in group_ids {
        let gid = gid as u32;
        let bytes = cache
            .read_group(CLIENTSCRIPTS_ARCHIVE, gid)
            .expect("io")
            .expect("group present");
        let script = ClientScript::decode(&bytes)
            .unwrap_or_else(|| panic!("script {gid} failed to decode"));
        scripts.insert(gid, script);
        originals.insert(gid, bytes);
    }

    let analysis = analyze_all(&scripts);
    assert!(analysis.diags.is_empty(), "signature inference failed: {:?}", analysis.diags);

    let mut checked = 0u64;
    for (&id, s) in &scripts {
        let ir = lift(id, s, &analysis.sigs)
            .unwrap_or_else(|e| panic!("script {id}: lift failed: {e}"));
        let back = compile(&ir)
            .unwrap_or_else(|e| panic!("script {id}: compile failed: {e}"));
        assert_eq!(
            back.encode(),
            originals[&id],
            "script {id}: decompile → recompile is not byte-identical"
        );
        checked += 1;
    }
    eprintln!("byte-identical structured round-trips: {checked}");
}
