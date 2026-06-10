//! Validates the ScriptFile/Provider decode against the reference
//! compiled pack (Engine-TS data, same compiler v26 format). Skips
//! when the reference pack isn't present.

use engine::script::provider::ScriptProvider;

#[test]
fn load_reference_pack() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../Engine-TS/data/pack");
    if !std::path::Path::new(&format!("{dir}/server/script.dat")).exists() {
        eprintln!("reference pack not present; skipping");
        return;
    }
    let provider = ScriptProvider::load(dir).expect("parse reference pack");
    // The 2005 content pack ships thousands of scripts; spot-check
    // lookup machinery.
    let login = provider.get_by_trigger(engine::script::trigger::LOGIN, -1, -1);
    assert!(login.is_some(), "expected a [login,_] script in the reference pack");
    let s = login.unwrap();
    eprintln!("login script: {} ({} ops)", s.name(), s.opcodes.len());
    assert!(!s.opcodes.is_empty());
}
