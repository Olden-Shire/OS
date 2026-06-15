//! Rename our npc pack stems + `.npc` filenames to the reference (Content-old /
//! rev377) `all.npc` identifiers, for every npc that matches 1:1 by cache id AND
//! display name (the safe set from `match_ref_npcs`). Names are tooling-only —
//! the cache keys npcs by id — so this is CRC-neutral.
//!
//! A reference stem that would collide with a non-renamed npc's current stem
//! gets an `_<id>` suffix (our usual dedup) instead of clobbering it.
//!
//! Usage: `cargo run --release --example rename_npcs_to_ref -p cache [-- --write]`
//! (dry run without `--write`).

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use cache::content::pack_file;

const REF_PACK: &str = "reference/Content-old/pack/npc.pack";
const REF_ALL: &str = "reference/Content-old/scripts/_unpack/377/all.npc";
const OUR_PACK: &str = "Content/pack/npc.pack";
const OUR_NPC_DIR: &str = "Content/config/npc";

fn main() {
    let write = std::env::args().any(|a| a == "--write");

    let ref_pack = pack_file::read(Path::new(REF_PACK)).expect("read ref npc.pack");
    let ref_names = parse_all_npc_names(REF_ALL); // ref stem -> display name
    // ref id -> display name (only ids that have an all.npc config).
    let ref_id_name: HashMap<u32, String> = ref_pack
        .iter()
        .filter_map(|(&id, stem)| ref_names.get(stem).map(|n| (id, n.clone())))
        .collect();

    let mut our_pack = pack_file::read(Path::new(OUR_PACK)).expect("read our npc.pack");
    let our_npcs = index_our_npcs(OUR_NPC_DIR); // id -> (display name, path)

    let norm = |s: &str| s.trim().to_lowercase();

    // Safe matches: same id + same display name → take the reference stem.
    let mut matched: Vec<(u32, String)> = Vec::new();
    for (&id, ref_name) in &ref_id_name {
        if let Some((our_name, _)) = our_npcs.get(&id)
            && norm(ref_name) == norm(our_name)
            && let Some(ref_stem) = ref_pack.get(&id)
        {
            matched.push((id, ref_stem.clone()));
        }
    }
    matched.sort_by_key(|(id, _)| *id);
    let matched_ids: HashSet<u32> = matched.iter().map(|(id, _)| *id).collect();

    // Stems already taken by npcs we are NOT renaming — don't clobber them.
    let mut taken: HashSet<String> = our_pack
        .iter()
        .filter(|(id, _)| !matched_ids.contains(id))
        .map(|(_, s)| s.clone())
        .collect();

    let mut renames: Vec<(u32, String, String)> = Vec::new(); // (id, old, new)
    let mut suffixed = 0usize;
    for (id, ref_stem) in &matched {
        let old = our_pack.get(id).cloned().unwrap_or_default();
        let mut new = ref_stem.clone();
        if taken.contains(&new) {
            new = format!("{ref_stem}_{id}");
            suffixed += 1;
        }
        taken.insert(new.clone());
        if new != old {
            renames.push((*id, old, new));
        }
    }

    println!(
        "{} matches · {} renamed (stem changed) · {} collision-suffixed · {} already named correctly",
        matched.len(),
        renames.len(),
        suffixed,
        matched.len() - renames.len(),
    );
    for (id, old, new) in renames.iter().take(12) {
        println!("    {id:>5}: {old}  ->  {new}");
    }
    if !write {
        println!("(dry run — pass --write to rename .npc files + rewrite npc.pack)");
        return;
    }

    // Two-phase rename: a target stem can equal another renamed npc's CURRENT
    // filename (e.g. giant_bat_78 -> "bat" while a different npc IS "bat.npc"
    // until its own turn), and fs::rename overwrites the destination. So move
    // everything to a unique temp name first, then temp -> final.
    for (id, _old, _new) in &renames {
        if let Some((_, path)) = our_npcs.get(id) {
            let tmp = path.with_file_name(format!("__ref_tmp_{id}.npc"));
            fs::rename(path, &tmp).unwrap_or_else(|e| panic!("tmp rename {path:?}: {e}"));
        }
    }
    let mut renamed_files = 0usize;
    for (id, _old, new) in &renames {
        if let Some((_, path)) = our_npcs.get(id) {
            let tmp = path.with_file_name(format!("__ref_tmp_{id}.npc"));
            let dst = path.with_file_name(format!("{new}.npc"));
            fs::rename(&tmp, &dst).unwrap_or_else(|e| panic!("rename {tmp:?} -> {dst:?}: {e}"));
            renamed_files += 1;
        }
        our_pack.insert(*id, new.clone());
    }
    pack_file::write(Path::new(OUR_PACK), &our_pack).expect("write npc.pack");
    println!("renamed {renamed_files} .npc files; updated npc.pack");
}

/// Parse `all.npc`: `[stem]` sections → each section's `name=` value.
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

/// Our per-id `.npc` files → id → (display name, path).
fn index_our_npcs(dir: &str) -> HashMap<u32, (String, PathBuf)> {
    let mut out = HashMap::new();
    let mut stack = vec![PathBuf::from(dir)];
    while let Some(d) = stack.pop() {
        let Ok(rd) = fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().and_then(|x| x.to_str()) == Some("npc")
                && let Some((id, name)) = read_npc_id_name(&p)
            {
                out.insert(id, (name, p));
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
