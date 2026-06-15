//! Model-name + recol/retex breakup refactor for obj and spot configs.
//!
//! Brings obj/spot into line with the npc treatment:
//!   - the `model` field references its model by pack NAME — unnamed models
//!     referenced here get the generic `model_<id>` (the rest keep whatever
//!     `rename_obj_models` / earlier passes already gave them);
//!   - `recol`/`retex` are broken into content-old's indexed lines
//!     (`recol1s`/`recol1d`/…) to match the matching codec change.
//!
//! Model files are single-file sharded groups whose on-disk stem IS the
//! `model.pack` value, so naming renames the `.ob2` in place + rewrites the
//! pack line (byte-identical on repack). The `.obj`/`.spot` rewrite is a
//! line-level text transform, so every other field + comments survive.
//! Idempotent. Verify with `--example verify_content -- Content`.
//!
//! Usage: `cargo run --release --example reformat_obj_spot -p cache [-- --write]`
//! (dry run without `--write`).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use cache::content::pack_file;

fn main() {
    let write = std::env::args().any(|a| a == "--write");
    let pack_dir = PathBuf::from("Content/pack");
    let models_dir = PathBuf::from("Content/models");
    let obj_files = config_files("Content/config/obj", "obj");
    let spot_files = config_files("Content/config/spot", "spot");
    let all: Vec<PathBuf> = obj_files.iter().chain(&spot_files).cloned().collect();

    let mut model_pack = pack_file::read(&pack_dir.join("model.pack")).expect("read model.pack");

    // Pass 1: every model id referenced by an obj/spot `model = <id>` line.
    let mut model_ids: HashSet<u32> = HashSet::new();
    for path in &all {
        let text = fs::read_to_string(path).expect("read config");
        for line in text.lines() {
            if let Some(id) = model_ref(line) {
                model_ids.insert(id);
            }
        }
    }
    // Name the still-unnamed ones `model_<id>`.
    let mut model_name: BTreeMap<u32, String> = BTreeMap::new();
    for &id in &model_ids {
        if model_pack.get(&id).is_none_or(|n| n.parse::<u32>().is_ok()) {
            model_name.insert(id, format!("model_{id}"));
        }
    }

    println!(
        "{} obj + {} spot files · {} models referenced ({} to name)",
        obj_files.len(), spot_files.len(), model_ids.len(), model_name.len(),
    );
    if !write {
        println!("(dry run — pass --write to rename models + rewrite obj/spot text)");
        return;
    }

    // Rename the sharded `.ob2` files + pack lines for the newly-named ids.
    let model_files = index_files(&models_dir, "ob2");
    let mut renamed = 0usize;
    for (id, name) in &model_name {
        if let Some(src) = model_files.get(&id.to_string()) {
            let dst = src.with_file_name(format!("{name}.ob2"));
            fs::rename(src, &dst).unwrap_or_else(|e| panic!("rename {src:?}: {e}"));
            renamed += 1;
        }
        model_pack.insert(*id, name.clone());
    }
    pack_file::write(&pack_dir.join("model.pack"), &model_pack).expect("write model.pack");
    println!("renamed {renamed} .ob2 files; updated model.pack");

    // id -> name for the ref rewrite (named entries only).
    let lut: HashMap<u32, String> = model_pack.iter()
        .filter(|(_, n)| n.parse::<u32>().is_err()).map(|(&i, n)| (i, n.clone())).collect();

    // Pass 2: rewrite `model` refs to names + break up recol/retex.
    let mut rewritten = 0usize;
    for path in &all {
        let text = fs::read_to_string(path).expect("read config");
        let new = reformat(&text, &lut);
        if new != text {
            fs::write(path, new).expect("write config");
            rewritten += 1;
        }
    }
    println!("rewrote {rewritten} obj/spot files");
}

/// The model id of a `model = <numeric>` line, if any (named refs return None).
fn model_ref(line: &str) -> Option<u32> {
    let (key, val) = line.split_once('=')?;
    if key.trim() == "model" { val.trim().parse::<u32>().ok() } else { None }
}

/// Rewrite one obj/spot file's text: name the `model` ref, index recol/retex.
fn reformat(text: &str, lut: &HashMap<u32, String>) -> String {
    let mut out = String::with_capacity(text.len() + text.len() / 8);
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") || !line.contains('=') {
            out.push_str(line);
            out.push('\n');
            continue;
        }
        let (key_raw, val) = line.split_once('=').expect("has '='");
        let key = key_raw.trim();
        let val = val.trim();
        match key {
            "model" => {
                let name = val.parse::<u32>().ok().and_then(|id| lut.get(&id)).cloned();
                out.push_str(&format!("model = {}\n", name.as_deref().unwrap_or(val)));
            }
            "recol" => emit_pairs(&mut out, "recol", val),
            "retex" => emit_pairs(&mut out, "retex", val),
            _ => {
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    out
}

/// `s/d s/d` → `<stem>1s = s` / `<stem>1d = d` / `<stem>2s = …` (one per half).
fn emit_pairs(out: &mut String, stem: &str, val: &str) {
    for (i, pair) in val.split_whitespace().enumerate() {
        let (s, d) = pair.split_once('/').expect("recol pair has '/'");
        out.push_str(&format!("{stem}{}s = {s}\n", i + 1));
        out.push_str(&format!("{stem}{}d = {d}\n", i + 1));
    }
}

/// Every `*.{ext}` under `dir`, recursing shard subdirs.
fn config_files(dir: &str, ext: &str) -> Vec<PathBuf> {
    index_files(Path::new(dir), ext).into_values().collect()
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
