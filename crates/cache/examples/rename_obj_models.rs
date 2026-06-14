//! Name the MODELS referenced by named obj configs, in `model.pack` + on disk.
//!
//! Each named obj exposes up to three model ids:
//!   - `model`     — the 2D inventory model        → `obj_<item>`
//!   - `manwear`   — the equipped male model       ┐ same id  → `obj_<item>_wear`
//!   - `womanwear` — the equipped female model     ┘ differ   → `obj_<item>_man`
//!                                                              + `obj_<item>_woman`
//! (single-gender wear keeps its own `_man` / `_woman`.)
//!
//! `<item>` is the obj's already-assigned callable stem (`obj.pack` value, e.g.
//! `cannonball`, `toolkit_1`), so the model names are globally unique. A model id
//! reachable from several objs/roles is named ONCE (first by ascending obj id,
//! inventory before wear) — the rest keep that name (it's the same model).
//!
//! Models are single-file sharded groups whose on-disk stem IS the `model.pack`
//! value (pack.rs `single_file_stem`), so we rename the `.ob2` in place inside its
//! shard dir + rewrite the pack line — byte-identical on repack.
//!
//! Then rewrites the `.obj` config text so `model`/`manwear`/`womanwear` reference
//! the new model NAMES instead of raw ids (via the model-aware config_text codec).
//! Idempotent — safe to re-run after the model files are already renamed.
//!
//! Usage: `cargo run --release --example rename_obj_models -p cache [-- --write]`
//! (dry run without `--write`). Run AFTER `rename_configs` (needs obj.pack stems).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use cache::Cache;
use cache::config::group;
use cache::configs::Configs;
use cache::content::{config_text, pack_file};
use cache::content::config_text::ModelRefs;

fn main() {
    let write = std::env::args().any(|a| a == "--write");
    let pack_dir = PathBuf::from("Content/pack");
    let models_dir = PathBuf::from("Content/models");
    let mut cache = Cache::open(Path::new("cache")).expect("open cache/");
    let configs = Configs::load(&mut cache).expect("load configs");

    // obj id → callable stem (numeric stems = unnamed objs, skipped).
    let obj_pack = pack_file::read(&pack_dir.join("obj.pack")).expect("read obj.pack");

    // Assign each model id a name, first-come (ascending obj id, inventory then
    // wear) so a shared model is named once and names never collide.
    let mut model_name: BTreeMap<u32, String> = BTreeMap::new();
    let mut taken: HashSet<u32> = HashSet::new();
    let (mut n_inv, mut n_wear, mut n_man, mut n_woman) = (0u32, 0u32, 0u32, 0u32);

    let mut obj_ids: Vec<i32> = obj_pack.keys().map(|&k| k as i32).collect();
    obj_ids.sort_unstable();
    for id in obj_ids {
        let Some(stem) = obj_pack.get(&(id as u32)) else { continue };
        if stem.parse::<u32>().is_ok() {
            continue; // unnamed obj (numeric stem)
        }
        let Some(obj) = configs.objs.get(&id) else { continue };

        let mut claim = |mid: i32, name: String, counter: &mut u32| {
            if mid < 0 {
                return;
            }
            let mid = mid as u32;
            if taken.insert(mid) {
                model_name.insert(mid, name);
                *counter += 1;
            }
        };

        // 1) inventory model.
        claim(obj.model, format!("obj_{stem}"), &mut n_inv);

        // 2) wear models.
        match (obj.manwear, obj.womanwear) {
            (m, w) if m >= 0 && w >= 0 && m == w => {
                claim(m, format!("obj_{stem}_wear"), &mut n_wear);
            }
            (m, w) => {
                if m >= 0 {
                    claim(m, format!("obj_{stem}_man"), &mut n_man);
                }
                if w >= 0 {
                    claim(w, format!("obj_{stem}_woman"), &mut n_woman);
                }
            }
        }
    }

    println!(
        "obj-referenced models: {} named  ({n_inv} inventory, {n_wear} shared-wear, {n_man} man, {n_woman} woman)",
        model_name.len()
    );
    for (mid, name) in model_name.iter().take(8) {
        println!("    model {mid:>6}  ->  {name}");
    }
    if !write {
        println!("\n(dry run — pass --write to update model.pack + rename .ob2 files)");
        return;
    }

    // Apply: locate each model file by its numeric stem across the shard dirs,
    // rename in place, and override its model.pack line.
    let mut pack = pack_file::read(&pack_dir.join("model.pack")).expect("read model.pack");
    let files = index_files(&models_dir, "ob2");
    let mut renamed = 0usize;
    for (mid, name) in &model_name {
        let cur = mid.to_string();
        let Some(src) = files.get(&cur) else {
            // No file by numeric stem — already renamed, or model id has no file.
            continue;
        };
        let dst = src.with_file_name(format!("{name}.ob2"));
        fs::rename(src, &dst).unwrap_or_else(|e| panic!("rename {src:?} -> {dst:?}: {e}"));
        pack.insert(*mid, name.clone());
        renamed += 1;
    }
    pack_file::write(&pack_dir.join("model.pack"), &pack).expect("write model.pack");
    println!("wrote Content/pack/model.pack + renamed {renamed} .ob2 files");

    // Finally rewrite the .obj config text so model/manwear/womanwear reference
    // the new model NAMES instead of raw ids. Round-trip through the now
    // model-aware codec (encode tolerates both names and ids, so this is
    // idempotent): text -> bytes -> name-bearing text.
    let refs = ModelRefs::from_pack(&pack);
    let (schema, kind) = config_text::schema_for_group(group::OBJ).expect("obj schema");
    let obj_dir = PathBuf::from("Content/config/obj");
    let mut rewritten = 0usize;
    for path in index_files(&obj_dir, "obj").into_values() {
        let text = fs::read_to_string(&path).expect("read .obj");
        let Some(id) = text
            .lines()
            .next()
            .and_then(|l| l.strip_prefix("// obj "))
            .and_then(|s| s.trim().parse::<u32>().ok())
        else {
            continue;
        };
        let Some(bytes) = config_text::encode(schema, &text, &refs) else { continue };
        let Some(new_text) = config_text::decode(schema, kind, id, &bytes, &refs) else { continue };
        if new_text != text {
            fs::write(&path, new_text).expect("write .obj");
            rewritten += 1;
        }
    }
    println!("rewrote {rewritten} .obj files to reference models by name");
}

/// Index every `*.{ext}` file under `dir` (recursing the 1000-id shard subdirs)
/// by its current stem, so a sharded model file can be found by numeric id.
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
