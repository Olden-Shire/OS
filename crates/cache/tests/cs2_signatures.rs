//! Stack-balance + signature-inference proof over the whole vanilla cache: every
//! clientscript in archive 12 must pass the abstract interpretation in `cs2_sig` —
//! consistent depths at every join, no underflow, all dynamic arities resolvable, and
//! agreeing depths at every return site. A single failure means the opcode arity table
//! disagrees with what the Jagex compiler actually emitted.

use std::collections::BTreeMap;
use std::path::PathBuf;

use cache::cs2::ClientScript;
use cache::cs2_sig::analyze_all;
use cache::Cache;

const CLIENTSCRIPTS_ARCHIVE: u8 = 12;

fn cache_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../cache")
}

#[test]
fn every_clientscript_passes_stack_balance_and_yields_a_signature() {
    let mut cache = Cache::open(&cache_dir()).expect("open cache");
    let group_ids: Vec<i32> = cache.index(CLIENTSCRIPTS_ARCHIVE).group_ids.clone();
    assert!(!group_ids.is_empty(), "clientscripts archive is empty");

    let mut scripts: BTreeMap<u32, ClientScript> = BTreeMap::new();
    for gid in group_ids {
        let gid = gid as u32;
        let bytes = cache
            .read_group(CLIENTSCRIPTS_ARCHIVE, gid)
            .expect("io")
            .expect("group present");
        let script = ClientScript::decode(&bytes)
            .unwrap_or_else(|| panic!("script {gid} failed to decode"));
        scripts.insert(gid, script);
    }
    let total = scripts.len();

    let analysis = analyze_all(&scripts);

    for d in &analysis.diags {
        eprintln!("FAIL {d}");
    }
    assert!(
        analysis.diags.is_empty(),
        "{} scripts failed stack-balance analysis",
        analysis.diags.len()
    );
    assert_eq!(analysis.sigs.len(), total, "every script must get a signature");

    let with_returns = analysis
        .sigs
        .values()
        .filter(|s| s.int_returns > 0 || s.str_returns > 0)
        .count();
    let with_args = analysis
        .sigs
        .values()
        .filter(|s| s.int_args > 0 || s.str_args > 0)
        .count();
    eprintln!(
        "signatures inferred for {total} scripts ({with_args} take args, {with_returns} return values)"
    );
}
