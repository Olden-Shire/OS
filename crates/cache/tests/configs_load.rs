//! End-to-end verification: load every ConfigType from the rev1 cache via `Configs::load`
//! and print per-table counts.

use std::path::PathBuf;

use cache::{Cache, Configs};

fn cache_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../cache")
}

#[test]
fn loads_every_config_record() {
    let mut c = Cache::open(&cache_dir()).expect("open cache");
    let cfg = Configs::load(&mut c).expect("Configs::load");

    eprintln!("  FluType:    {:>6}", cfg.flus.len());
    eprintln!("  IdkType:    {:>6}", cfg.idks.len());
    eprintln!("  FloType:    {:>6}", cfg.flos.len());
    eprintln!("  InvType:    {:>6}", cfg.invs.len());
    eprintln!("  LocType:    {:>6}", cfg.locs.len());
    eprintln!("  EnumType:   {:>6}", cfg.enums.len());
    eprintln!("  NpcType:    {:>6}", cfg.npcs.len());
    eprintln!("  ObjType:    {:>6}", cfg.objs.len());
    eprintln!("  SeqType:    {:>6}", cfg.seqs.len());
    eprintln!("  SpotType:   {:>6}", cfg.spots.len());
    eprintln!("  VarBitType: {:>6}", cfg.varbits.len());
    eprintln!("  VarpType:   {:>6}", cfg.varps.len());
    eprintln!("  ──────────────────");
    eprintln!("  Total:      {:>6}", cfg.total());

    assert!(cfg.npcs.len() > 1000);
    assert!(cfg.objs.len() > 5000);
    assert!(cfg.locs.len() > 5000);
    assert!(cfg.seqs.len() > 1000);
    assert!(cfg.total() > 50_000, "only {} total records", cfg.total());
}

#[test]
fn well_known_npcs_have_plausible_fields() {
    // Spot-check a few well-known rev1 NPCs by name. These IDs are stable in OSRS rev1.
    let mut c = Cache::open(&cache_dir()).expect("open cache");
    let cfg = Configs::load(&mut c).expect("Configs::load");

    // Just make sure name decoding worked at all: a few non-empty names exist.
    let named: Vec<_> =
        cfg.npcs.values().filter(|n| n.name != "null" && !n.name.is_empty()).collect();
    assert!(named.len() > 1000, "only {} npcs have proper names", named.len());

    let with_models: Vec<_> = cfg.npcs.values().filter(|n| !n.models.is_empty()).collect();
    assert!(with_models.len() > 1000, "only {} npcs have model lists", with_models.len());
}

#[test]
fn well_known_objs_have_plausible_fields() {
    let mut c = Cache::open(&cache_dir()).expect("open cache");
    let cfg = Configs::load(&mut c).expect("Configs::load");

    // Coins (item id 995 in OSRS) — should be named "Coins" and stackable.
    let coins = cfg.objs.get(&995).expect("item 995 (coins) missing");
    eprintln!("  obj 995: name={:?} stackable={} cost={}", coins.name, coins.stackable, coins.cost);
    assert_eq!(coins.name, "Coins");
    assert_eq!(coins.stackable, 1);

    // Members items should have members=true on at least some objs.
    let members_count = cfg.objs.values().filter(|o| o.members).count();
    assert!(members_count > 100, "only {} members items", members_count);

    // Cert template merge: every cert obj (certtemplate != -1) should have stackable=1
    // and a non-default name (copied from the linked obj).
    let certs: Vec<_> = cfg.objs.values().filter(|o| o.certtemplate != -1).collect();
    assert!(certs.len() > 100, "only {} cert objs", certs.len());
    for cert in &certs {
        assert_eq!(cert.stackable, 1, "cert {} not stackable after merge", cert.id);
    }
    let named_certs = certs.iter().filter(|o| o.name != "null").count();
    assert!(named_certs > 100, "only {named_certs}/{} certs got linked names", certs.len());
    eprintln!("  cert objs: {} total, {} with non-default name", certs.len(), named_certs);
}
