//! Count how many reference (Content-old / rev377) npc configs map onto OURS.
//!
//! The join key is the cache npc id (their `npc.pack` id→stem + `all.npc`
//! stem→config vs our `npc.pack` id→stem + per-id `.npc`), confirmed by the
//! display `name=`. A "match" = same id present in both AND identical display
//! name — high confidence it's the same npc across revisions, so the reference
//! config can be ported onto our id.
//!
//! Read-only analysis. Usage: `cargo run --release --example match_ref_npcs -p cache`

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

const REF_PACK: &str = "reference/Content-old/pack/npc.pack";
const REF_ALL: &str = "reference/Content-old/scripts/_unpack/377/all.npc";
const OUR_PACK: &str = "Content/pack/npc.pack";
const OUR_NPC_DIR: &str = "Content/config/npc";

fn main() {
    // ref: id -> stem, and stem -> display name (from all.npc sections).
    let ref_id_stem = read_pack(REF_PACK);
    let ref_stem_name = parse_all_npc_names(REF_ALL);
    // ref: id -> display name
    let ref_id_name: HashMap<u32, String> = ref_id_stem
        .iter()
        .filter_map(|(&id, stem)| ref_stem_name.get(stem).map(|n| (id, n.clone())))
        .collect();

    // ours: id -> display name (from the per-id .npc files).
    let our_id_name = read_our_npc_names(OUR_NPC_DIR);

    let ref_ids: std::collections::HashSet<u32> = ref_id_stem.keys().copied().collect();
    let our_ids: std::collections::HashSet<u32> = read_pack(OUR_PACK).keys().copied().collect();
    let id_overlap = ref_ids.intersection(&our_ids).count();

    let norm = |s: &str| s.trim().to_lowercase();
    let (mut name_id_match, mut name_mismatch, mut ref_no_name, mut our_no_name) = (0, 0, 0, 0);
    let mut mismatches: Vec<(u32, String, String)> = Vec::new();
    for &id in ref_ids.intersection(&our_ids) {
        match (ref_id_name.get(&id), our_id_name.get(&id)) {
            (Some(r), Some(o)) => {
                if norm(r) == norm(o) {
                    name_id_match += 1;
                } else {
                    name_mismatch += 1;
                    if mismatches.len() < 15 {
                        mismatches.push((id, r.clone(), o.clone()));
                    }
                }
            }
            (None, _) => ref_no_name += 1,
            (_, None) => our_no_name += 1,
        }
    }

    println!("reference npcs (npc.pack):        {}", ref_id_stem.len());
    println!("  with a name in all.npc:         {}", ref_id_name.len());
    println!("our npcs (npc.pack):              {}", our_ids.len());
    println!("  with a name in config:          {}", our_id_name.len());
    println!("id overlap (in both packs):       {id_overlap}");
    println!("  -> SAME id + SAME name (match): {name_id_match}");
    println!("  -> same id, different name:     {name_mismatch}");
    println!("  -> ref id has no all.npc name:  {ref_no_name}");
    println!("  -> our id has no name:          {our_no_name}");
    if !mismatches.is_empty() {
        println!("\nsample same-id name mismatches (id: ref vs ours):");
        for (id, r, o) in &mismatches {
            println!("  {id:>5}: {r:?}  vs  {o:?}");
        }
    }
}

/// Parse an `id=stem` pack file (skips numeric-stub names).
fn read_pack(path: &str) -> HashMap<u32, String> {
    let mut out = HashMap::new();
    let Ok(text) = fs::read_to_string(path) else {
        eprintln!("WARN: could not read {path}");
        return out;
    };
    for line in text.lines() {
        if let Some((id, stem)) = line.split_once('=')
            && let Ok(id) = id.trim().parse::<u32>()
        {
            out.insert(id, stem.trim().to_string());
        }
    }
    out
}

/// Parse `all.npc`: `[stem]` sections, grabbing each section's `name=` value.
fn parse_all_npc_names(path: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let Ok(text) = fs::read_to_string(path) else {
        eprintln!("WARN: could not read {path}");
        return out;
    };
    let mut stem: Option<String> = None;
    for line in text.lines() {
        let t = line.trim();
        if let Some(inner) = t.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            stem = Some(inner.trim().to_string());
        } else if let Some(rest) = t.strip_prefix("name=")
            && let Some(s) = &stem
        {
            out.insert(s.clone(), rest.trim().to_string());
        }
    }
    out
}

/// Our per-id `.npc` files: `// npc {id}` header + `name = …` line → id→name.
fn read_our_npc_names(dir: &str) -> HashMap<u32, String> {
    let mut out = HashMap::new();
    let mut stack = vec![PathBuf::from(dir)];
    while let Some(d) = stack.pop() {
        let Ok(rd) = fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().and_then(|x| x.to_str()) == Some("npc") {
                if let Some((id, name)) = read_npc_id_name(&p) {
                    out.insert(id, name);
                }
            }
        }
    }
    out
}

fn read_npc_id_name(path: &Path) -> Option<(u32, String)> {
    let text = fs::read_to_string(path).ok()?;
    let mut id = None;
    let mut name = None;
    for line in text.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("// npc ") {
            id = rest.trim().parse::<u32>().ok();
        } else if let Some((k, v)) = t.split_once('=')
            && k.trim() == "name"
        {
            name = Some(v.trim().to_string());
        }
    }
    Some((id?, name?))
}
