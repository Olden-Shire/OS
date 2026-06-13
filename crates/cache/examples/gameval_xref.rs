//! Cross-reference RuneLite `gameval` symbolic ids (rev ~240) against our
//! rev1 cache configs. For the typed archives that carry a display name
//! (obj / npc / loc) we can VALIDATE alignment: a gameval entry is "true"
//! for rev1 when its de-symbolised name matches our decoded `name` at the
//! same id. For nameless archives (varbit/varp/anim/…) we just report
//! coverage (gameval is the only naming source there).
//!
//! Usage: `cargo run --release --example gameval_xref -p cache [-- gameval_dir]`

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use cache::configs::Configs;
use cache::Cache;

fn main() {
    let gv_dir = std::env::args().nth(1).map_or_else(|| PathBuf::from("gameval"), PathBuf::from);
    let mut cache = Cache::open(Path::new("cache")).expect("open cache/");
    let configs = Configs::load(&mut cache).expect("load configs");

    println!("== named archives (validated against our decoded config names) ==\n");
    // (gameval file, our id→name map, max id present in our cache)
    let obj_names: HashMap<i32, String> =
        configs.objs.iter().map(|(&k, v)| (k, v.name.clone())).collect();
    let npc_names: HashMap<i32, String> =
        configs.npcs.iter().map(|(&k, v)| (k, v.name.clone())).collect();
    let loc_names: HashMap<i32, String> =
        configs.locs.iter().map(|(&k, v)| (k, v.name.clone())).collect();

    xref_named("ItemID", &gv_dir.join("ItemID.java"), &obj_names);
    xref_named("NpcID", &gv_dir.join("NpcID.java"), &npc_names);
    // ObjectID extends ObjectID1 — load both into one map.
    let mut object_ids = parse_gameval(&gv_dir.join("ObjectID.java"));
    object_ids.extend(parse_gameval(&gv_dir.join("ObjectID1.java")));
    report_named("ObjectID(+1)", &object_ids, &loc_names);

    println!("\n== nameless archives (coverage only — gameval is the naming source) ==\n");
    coverage("VarbitID", &gv_dir.join("VarbitID.java"), &configs.varbits.keys().copied().collect());
    coverage("VarPlayerID", &gv_dir.join("VarPlayerID.java"), &configs.varps.keys().copied().collect());
    coverage("AnimationID", &gv_dir.join("AnimationID.java"), &configs.seqs.keys().copied().collect());
    coverage("SpotanimID", &gv_dir.join("SpotanimID.java"), &configs.spots.keys().copied().collect());
    coverage("InventoryID", &gv_dir.join("InventoryID.java"), &configs.invs.keys().copied().collect());
    coverage("EnumID?", &gv_dir.join("DBTableID.java"), &configs.enums.keys().copied().collect());
}

/// Parse a gameval `public static final int NAME = N;` file → N → NAME.
fn parse_gameval(path: &Path) -> HashMap<i32, String> {
    let mut map = HashMap::new();
    let Ok(text) = std::fs::read_to_string(path) else {
        eprintln!("  (missing {path:?})");
        return map;
    };
    for line in text.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("public static final int ") else { continue };
        let Some((name, val)) = rest.split_once('=') else { continue };
        let name = name.trim();
        let val = val.trim().trim_end_matches(';').trim();
        if let Ok(id) = val.parse::<i32>() {
            // Later duplicate ids (aliases) keep the first symbolic name.
            map.entry(id).or_insert_with(|| name.to_string());
        }
    }
    map
}

/// Normalise for fuzzy name comparison: lowercase alphanumerics only.
fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_lowercase()).collect()
}

fn xref_named(label: &str, gv_path: &Path, ours: &HashMap<i32, String>) {
    report_named(label, &parse_gameval(gv_path), ours);
}

fn report_named(label: &str, gv: &HashMap<i32, String>, ours: &HashMap<i32, String>) {
    let mut shared = 0u32; // id present in both
    let mut name_match = 0u32; // de-symbolised gameval name ~ our display name
    let mut samples: Vec<String> = Vec::new();
    let mut mismatches: Vec<String> = Vec::new();
    for (&id, gv_name) in gv {
        let Some(our_name) = ours.get(&id) else { continue };
        if our_name.is_empty() || our_name.eq_ignore_ascii_case("null") {
            continue;
        }
        shared += 1;
        let g = norm(gv_name);
        let o = norm(our_name);
        // gameval names are word-joined (CANNONBALL, BRONZE_DAGGER); our
        // display names have spaces/punct. Match if one contains the other
        // after normalisation (handles "Bronze dagger" vs BRONZE_DAGGER).
        let hit = !g.is_empty() && !o.is_empty() && (g.contains(&o) || o.contains(&g));
        if hit {
            name_match += 1;
            if samples.len() < 6 {
                samples.push(format!("    {id:>6}  {gv_name}  ==  {our_name:?}"));
            }
        } else if mismatches.len() < 6 {
            mismatches.push(format!("    {id:>6}  {gv_name}  !=  {our_name:?}"));
        }
    }
    let pct = if shared > 0 { name_match * 100 / shared } else { 0 };
    println!(
        "{label}: {} gameval ids, {} share an id with our named configs, {name_match} names align ({pct}%)",
        gv.len(),
        shared,
    );
    // Alignment by id band — low ids are rev1-era and should align far
    // better than later content. This is the actionable part: the band
    // where alignment stays high is the range gameval names are safe to
    // import into our rev1 packs.
    let band = 2000;
    let mut band_shared: HashMap<i32, u32> = HashMap::new();
    let mut band_hit: HashMap<i32, u32> = HashMap::new();
    for (&id, gv_name) in gv {
        let Some(our_name) = ours.get(&id) else { continue };
        if our_name.is_empty() || our_name.eq_ignore_ascii_case("null") {
            continue;
        }
        let b = id / band;
        *band_shared.entry(b).or_default() += 1;
        let g = norm(gv_name);
        let o = norm(our_name);
        if !g.is_empty() && !o.is_empty() && (g.contains(&o) || o.contains(&g)) {
            *band_hit.entry(b).or_default() += 1;
        }
    }
    let mut bands: Vec<i32> = band_shared.keys().copied().collect();
    bands.sort_unstable();
    print!("  by id band:");
    for b in bands {
        let s = band_shared[&b];
        let h = band_hit.get(&b).copied().unwrap_or(0);
        print!(" [{}-{}k:{}%]", b * band / 1000, (b + 1) * band / 1000, if s > 0 { h * 100 / s } else { 0 });
    }
    println!();
    for s in &samples {
        println!("{s}");
    }
    if !mismatches.is_empty() {
        println!("  sample divergences (rev drift / naming style):");
        for m in &mismatches {
            println!("{m}");
        }
    }
    println!();
}

fn coverage(label: &str, gv_path: &Path, our_ids: &std::collections::HashSet<i32>) {
    let gv = parse_gameval(gv_path);
    let in_range = gv.keys().filter(|id| our_ids.contains(id)).count();
    let our_max = our_ids.iter().copied().max().unwrap_or(-1);
    let gv_max = gv.keys().copied().max().unwrap_or(-1);
    println!(
        "{label}: gameval {} ids (max {gv_max}); our cache has {} ids (max {our_max}); \
         {in_range} gameval ids fall on an id we have",
        gv.len(),
        our_ids.len(),
    );
}
