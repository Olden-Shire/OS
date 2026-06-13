//! Name every named config entry (obj / npc / loc) by its OWN decoded
//! `name` field, in a `lower_case_abc123` scheme for use as .rs2/.cs2
//! identifiers. Null/blank names stay numeric. Duplicate names (common —
//! noted items, "Cannonball", generic scenery) are disambiguated with an
//! `_<id>` suffix so every identifier is globally unique within its pack.
//!
//! Both the pack file AND the on-disk config text files are renamed
//! together — the pack stem IS the filename (`Content/config/<g>/<stem>.<g>`).
//!
//! Usage: `cargo run --release --example rename_configs -p cache [-- --write]`
//! (dry run without `--write`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use cache::configs::Configs;
use cache::content::pack_file;
use cache::Cache;

fn main() {
    let write = std::env::args().any(|a| a == "--write");
    let pack_dir = PathBuf::from("Content/pack");
    let cfg_root = PathBuf::from("Content/config");
    let mut cache = Cache::open(Path::new("cache")).expect("open cache/");
    let configs = Configs::load(&mut cache).expect("load configs");

    let obj: HashMap<i32, String> = configs.objs.iter().map(|(&k, v)| (k, v.name.clone())).collect();
    let npc: HashMap<i32, String> = configs.npcs.iter().map(|(&k, v)| (k, v.name.clone())).collect();
    let loc: HashMap<i32, String> = configs.locs.iter().map(|(&k, v)| (k, v.name.clone())).collect();

    rename("obj", &pack_dir.join("obj.pack"), &cfg_root.join("obj"), &obj, write);
    rename("npc", &pack_dir.join("npc.pack"), &cfg_root.join("npc"), &npc, write);
    rename("loc", &pack_dir.join("loc.pack"), &cfg_root.join("loc"), &loc, write);

    if !write {
        println!("\n(dry run — pass --write to update packs + rename config files)");
    }
}

/// A display name → `lower_case_abc123` identifier. Apostrophes are
/// dropped (so "Nulodion's notes" → "nulodions_notes"); every other run
/// of non-alphanumerics becomes a single `_`. Returns None for blank /
/// "null" names or ones that would start with a digit (not a valid id).
fn to_ident(name: &str) -> Option<String> {
    if name.is_empty() || name.eq_ignore_ascii_case("null") {
        return None;
    }
    let mut out = String::new();
    let mut pending_sep = false;
    for ch in name.chars() {
        let c = ch.to_ascii_lowercase();
        if c.is_ascii_alphanumeric() {
            if pending_sep && !out.is_empty() {
                out.push('_');
            }
            pending_sep = false;
            out.push(c);
        } else if c == '\'' || c == '\u{2019}' {
            // Drop apostrophes entirely (don't split the word).
        } else {
            pending_sep = true;
        }
    }
    if out.is_empty() || out.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    Some(out)
}

fn rename(
    label: &str,
    pack_path: &Path,
    cfg_dir: &Path,
    names: &HashMap<i32, String>,
    write: bool,
) {
    // Start from the existing pack's id set (numeric stubs) so we keep
    // the exact id coverage and only override names.
    let mut pack = pack_file::read(pack_path).unwrap_or_default();
    if pack.is_empty() {
        for &id in names.keys() {
            pack.insert(id as u32, id.to_string());
        }
    }

    // 1) base ident per id (None = stays numeric).
    let mut base: HashMap<i32, String> = HashMap::new();
    for (&id, name) in names {
        if let Some(b) = to_ident(name) {
            base.insert(id, b);
        }
    }
    // 2) frequency of each base — unique bases stay bare, collisions get
    //    an `_<id>` suffix.
    let mut freq: HashMap<&str, u32> = HashMap::new();
    for b in base.values() {
        *freq.entry(b.as_str()).or_default() += 1;
    }

    // 3) assign, with a final global-uniqueness guard (a bare unique name
    //    could still equal another entry's `base_id` form).
    let mut used: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut assigned: Vec<(i32, String)> = Vec::new();
    let mut ids: Vec<i32> = base.keys().copied().collect();
    ids.sort_unstable();
    for id in ids {
        let b = &base[&id];
        let mut ident = if freq[b.as_str()] == 1 { b.clone() } else { format!("{b}_{id}") };
        while used.contains(&ident) {
            ident = format!("{ident}_{id}");
        }
        used.insert(ident.clone());
        assigned.push((id, ident));
    }

    let null_or_blank = names.values().filter(|n| n.is_empty() || n.eq_ignore_ascii_case("null")).count();
    let digit_skipped = names.len() - base.len() - null_or_blank;
    let dupe_suffixed = assigned.iter().filter(|(_, n)| !base.values().any(|b| b == n)).count();
    println!(
        "{label}: {} entries → {} named ({} unique, {} dupe-suffixed); {} null/blank, {} digit-leading kept numeric",
        names.len(),
        assigned.len(),
        assigned.len() - dupe_suffixed,
        dupe_suffixed,
        null_or_blank,
        digit_skipped,
    );
    for (id, ident) in assigned.iter().take(6) {
        println!("    {id:>6}  {ident}   ({:?})", names[id]);
    }

    if !write {
        return;
    }

    // Apply: rename on-disk files first (fail loudly before touching the
    // pack if a source is missing), then write the pack.
    let ext = label; // obj/loc/npc dir == ext
    for (id, ident) in &assigned {
        let src = cfg_dir.join(format!("{id}.{ext}"));
        let dst = cfg_dir.join(format!("{ident}.{ext}"));
        if src == dst {
            continue;
        }
        if !src.exists() {
            // Already renamed (non-pristine re-run) — skip.
            continue;
        }
        std::fs::rename(&src, &dst).unwrap_or_else(|e| panic!("rename {src:?} -> {dst:?}: {e}"));
        pack.insert(*id as u32, ident.clone());
    }
    pack_file::write(pack_path, &pack).expect("write pack");
    println!("  wrote {pack_path:?} + renamed {} files in {cfg_dir:?}", assigned.len());
}
