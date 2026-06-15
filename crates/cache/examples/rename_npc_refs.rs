//! Name the SEQS and MODELS referenced by npc configs, in their packs + on disk,
//! then rewrite the `.npc` text to reference them by name.
//!
//! NPC anim fields (`readyanim`, `walkanim`, `turnleftanim`, `turnrightanim`)
//! point at seq-config ids; the indexed `model<N>`/`head<N>` lines at model
//! ids. We
//! give each referenced seq the generic name `seq_<id>` (the IDE plugin renames
//! them to something meaningful later) and each referenced model its existing
//! `model.pack` name, or `model_<id>` if unnamed. Config-file stems come from the
//! pack, so we rename the sharded `.seq`/`.ob2` files in place + rewrite the pack
//! line (byte-identical on repack — the cache keys groups by id, not name).
//!
//! The `.npc` rewrite is surgical (line-level) so authored server-only keys
//! (`wanderrange`, `huntmode`, …) and comments survive. Idempotent.
//!
//! Usage: `cargo run --release --example rename_npc_refs -p cache [-- --write]`
//! (dry run without `--write`).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use cache::content::pack_file;

/// Seq-valued keys (each a single id now that the 4-direction walk is broken
/// into `walkanim`/`walkanim_b`/`walkanim_r`/`walkanim_l`). Comma-split is kept
/// for resilience against any still-joined `walkanim` line.
const SEQ_KEYS: &[&str] =
    &["readyanim", "walkanim", "walkanim_b", "walkanim_r", "walkanim_l", "turnleftanim", "turnrightanim"];

/// A model-ref line in content-old's indexed form: `model<N>` / `head<N>`,
/// each a single id. (Replaces the old joined `models`/`headmodels` lists.)
fn is_model_key(key: &str) -> bool {
    for stem in ["model", "head"] {
        if let Some(rest) = key.strip_prefix(stem)
            && !rest.is_empty()
            && rest.bytes().all(|b| b.is_ascii_digit())
        {
            return true;
        }
    }
    false
}

fn main() {
    let write = std::env::args().any(|a| a == "--write");
    let pack_dir = PathBuf::from("Content/pack");
    let npc_dir = PathBuf::from("Content/config/npc");
    let seq_dir = PathBuf::from("Content/config/seq");
    let models_dir = PathBuf::from("Content/models");

    let mut seq_pack = pack_file::read(&pack_dir.join("seq.pack")).expect("read seq.pack");
    let mut model_pack = pack_file::read(&pack_dir.join("model.pack")).expect("read model.pack");
    let npc_files: Vec<PathBuf> = index_files(&npc_dir, "npc").into_values().collect();

    // Pass 1: collect every referenced seq + model id from the .npc fields.
    let mut seq_ids: HashSet<u32> = HashSet::new();
    let mut model_ids: HashSet<u32> = HashSet::new();
    for path in &npc_files {
        let text = fs::read_to_string(path).expect("read .npc");
        for line in text.lines() {
            let Some((key, val)) = line.split_once('=') else { continue };
            let key = key.trim();
            if SEQ_KEYS.contains(&key) {
                for tok in val.split(',') {
                    if let Some(id) = num(tok) { seq_ids.insert(id); }
                }
            } else if is_model_key(key) {
                if let Some(id) = num(val) { model_ids.insert(id); }
            }
        }
    }

    // Assign names. Seqs: seq_<id> unless already named. Models: keep existing
    // name, else model_<id>.
    let mut seq_name: BTreeMap<u32, String> = BTreeMap::new();
    for &id in &seq_ids {
        let cur = seq_pack.get(&id);
        if cur.is_none_or(|n| n.parse::<u32>().is_ok()) {
            seq_name.insert(id, format!("seq_{id}"));
        }
    }
    let mut model_name: BTreeMap<u32, String> = BTreeMap::new();
    for &id in &model_ids {
        let cur = model_pack.get(&id);
        if cur.is_none_or(|n| n.parse::<u32>().is_ok()) {
            model_name.insert(id, format!("model_{id}"));
        }
    }

    println!(
        "{} npc files · {} seqs referenced ({} to name) · {} models referenced ({} to name)",
        npc_files.len(), seq_ids.len(), seq_name.len(), model_ids.len(), model_name.len(),
    );
    if !write {
        println!("(dry run — pass --write to rename files + packs + rewrite .npc)");
        return;
    }

    // Rename the sharded config/model files + pack lines for the newly-named ids.
    let seq_files = index_files(&seq_dir, "seq");
    let model_files = index_files(&models_dir, "ob2");
    let mut renamed_seq = 0usize;
    for (id, name) in &seq_name {
        if let Some(src) = seq_files.get(&id.to_string()) {
            let dst = src.with_file_name(format!("{name}.seq"));
            fs::rename(src, &dst).unwrap_or_else(|e| panic!("rename {src:?}: {e}"));
            renamed_seq += 1;
        }
        seq_pack.insert(*id, name.clone());
    }
    let mut renamed_model = 0usize;
    for (id, name) in &model_name {
        if let Some(src) = model_files.get(&id.to_string()) {
            let dst = src.with_file_name(format!("{name}.ob2"));
            fs::rename(src, &dst).unwrap_or_else(|e| panic!("rename {src:?}: {e}"));
            renamed_model += 1;
        }
        model_pack.insert(*id, name.clone());
    }
    pack_file::write(&pack_dir.join("seq.pack"), &seq_pack).expect("write seq.pack");
    pack_file::write(&pack_dir.join("model.pack"), &model_pack).expect("write model.pack");
    println!("renamed {renamed_seq} .seq + {renamed_model} .ob2 files; updated seq.pack + model.pack");

    // Full id->name maps for the rewrite (named entries only).
    let seq_lut: HashMap<u32, String> = seq_pack.iter()
        .filter(|(_, n)| n.parse::<u32>().is_err()).map(|(&i, n)| (i, n.clone())).collect();
    let model_lut: HashMap<u32, String> = model_pack.iter()
        .filter(|(_, n)| n.parse::<u32>().is_err()).map(|(&i, n)| (i, n.clone())).collect();

    // Pass 2: surgical rewrite of the anim/model lines (other lines untouched).
    let mut rewritten = 0usize;
    for path in &npc_files {
        let text = fs::read_to_string(path).expect("read .npc");
        let mut out = String::with_capacity(text.len());
        let mut changed = false;
        for line in text.lines() {
            let new = rewrite_line(line, &seq_lut, &model_lut);
            changed |= new != line;
            out.push_str(&new);
            out.push('\n');
        }
        if changed {
            fs::write(path, out).expect("write .npc");
            rewritten += 1;
        }
    }
    println!("rewrote {rewritten} .npc files to reference seqs/models by name");
}

/// Replace numeric ids with pack names in the anim/model fields of one line.
fn rewrite_line(line: &str, seq: &HashMap<u32, String>, model: &HashMap<u32, String>) -> String {
    let Some((key_raw, val)) = line.split_once('=') else { return line.to_string() };
    let key = key_raw.trim();
    let map_tok = |tok: &str, lut: &HashMap<u32, String>| -> String {
        match num(tok) {
            Some(id) => lut.get(&id).cloned().unwrap_or_else(|| tok.trim().to_string()),
            None => tok.trim().to_string(),
        }
    };
    let new_val = if SEQ_KEYS.contains(&key) {
        val.split(',').map(|t| map_tok(t, seq)).collect::<Vec<_>>().join(", ")
    } else if is_model_key(key) {
        map_tok(val, model)
    } else {
        return line.to_string();
    };
    format!("{key} = {new_val}")
}

fn num(s: &str) -> Option<u32> {
    s.trim().parse::<u32>().ok()
}

/// Index every `*.{ext}` under `dir` (recursing shard subdirs) by current stem.
fn index_files(dir: &Path, ext: &str) -> HashMap<String, PathBuf> {
    let mut out = HashMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = fs::read_dir(&d) else { continue };
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().and_then(|s| s.to_str()) == Some(ext)
                && let Some(stem) = p.file_stem().and_then(|s| s.to_str())
            {
                out.insert(stem.to_string(), p.clone());
            }
        }
    }
    out
}
