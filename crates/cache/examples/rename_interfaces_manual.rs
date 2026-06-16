//! Manually assign reference interface NAMES to specific cache interface ids,
//! for the dialogue interfaces that `rename_interfaces_to_ref` can't resolve:
//! the multi-option / objbox / skill series have several structurally-identical
//! candidates in our cache, so the canonical one is picked by EYE (rendering the
//! candidate in-client via `::if <id>` and seeing which behaves correctly), then
//! recorded here.
//!
//! Rename = tooling-only (the cache keys by id): `{id}.if` -> `{id}_{name}.if`,
//! interface.pack `if_{id}` -> `{name}` (group line + each `if_{id}:com_N` ->
//! `{name}:com_N`). CRC-IDENTICAL on repack. Idempotent — an id already carrying
//! its name is skipped, and a name already taken by a DIFFERENT id is refused.
//!
//! Usage: `cargo run --release --example rename_interfaces_manual -p cache [-- --write]`

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

const OUR_IF_DIR: &str = "Content/interfaces";
const OUR_PACK: &str = "Content/pack/interface.pack";

/// Eye-verified (id -> reference name) assignments. Add to this list as each
/// ambiguous dialogue interface is identified in-client.
///
/// The multi-option "Select an Option" dialogs, eye-verified in-client. Each
/// has several structurally-identical candidates; the option-button count
/// (buttontype=6 components) matches the name (multiN = N options):
///   multi2=228 (2 opts), multi3=230 (3), multi4=232 (4), multi5=234 (5).
/// For multi3, candidates 230/231/451 all show a "please wait" on the clicked
/// option while 458 does not — 230 is canonical.
/// doubleobjbox=131 — the two-item message box (two object models flanking a
/// descriptive text + "Click here to continue"). content-old's doubleobjbox
/// uses four separate 17px `Line` text comps; our 2007 cache consolidated them
/// into one tall 380×69 wrapping text block, so it's 4 comps (2 model, 2 text)
/// rather than content-old's 7 — matched by eye, not structure.
const OVERRIDES: &[(u32, &str)] = &[
    (131, "doubleobjbox"),
    (228, "multi2"),
    (230, "multi3"),
    (232, "multi4"),
    (234, "multi5"),
];

fn main() {
    let write = std::env::args().any(|a| a == "--write");

    // Map id -> current pack name (the group line `{id}={val}`).
    let pack = fs::read_to_string(OUR_PACK).expect("read interface.pack");
    let mut group_name: HashMap<u32, String> = HashMap::new();
    for line in pack.lines() {
        if let Some((key, val)) = line.split_once('=') {
            // The group line's key is the bare id; component keys are large.
            if let Ok(id) = key.trim().parse::<u32>() {
                group_name.insert(id, val.trim().to_string());
            }
        }
    }

    // Validate the overrides against the current pack.
    let mut todo: Vec<(u32, &str)> = Vec::new();
    for &(id, name) in OVERRIDES {
        match group_name.get(&id) {
            None => {
                eprintln!("⚠ id {id} not in interface.pack — skipping");
                continue;
            }
            Some(cur) if cur == name => {
                println!("  {id} already named {name} — skip");
                continue;
            }
            Some(cur) if cur != &format!("if_{id}") => {
                eprintln!("⚠ id {id} already named '{cur}' (not the default if_{id}) — refusing to clobber");
                continue;
            }
            _ => {}
        }
        // Refuse to hand `name` to id N if a different id already owns it.
        if let Some((other, _)) = group_name.iter().find(|(oid, n)| **oid != id && n.as_str() == name) {
            eprintln!("⚠ name '{name}' already owned by id {other} — skipping id {id}");
            continue;
        }
        todo.push((id, name));
    }

    if todo.is_empty() {
        println!("nothing to do (all overrides already applied / refused)");
        return;
    }
    println!("{} interface rename(s):", todo.len());
    for (id, name) in &todo {
        println!("    {id:>6} -> {name}");
    }
    if !write {
        println!("(dry run — pass --write to rename .if files + interface.pack)");
        return;
    }

    // Rewrite interface.pack: `if_{id}` -> `{name}` for the group + components.
    let by_id: HashMap<u32, &str> = todo.iter().copied().collect();
    let mut out = String::with_capacity(pack.len());
    for line in pack.lines() {
        if let Some((key, val)) = line.split_once('=') {
            if let Some(rest) = val.strip_prefix("if_") {
                let (idpart, suffix) = rest.split_once(':').map_or((rest, ""), |(a, b)| (a, b));
                if let Ok(id) = idpart.parse::<u32>()
                    && let Some(&name) = by_id.get(&id)
                {
                    if suffix.is_empty() {
                        out.push_str(&format!("{key}={name}\n"));
                    } else {
                        out.push_str(&format!("{key}={name}:{suffix}\n"));
                    }
                    continue;
                }
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    fs::write(OUR_PACK, out).expect("write interface.pack");

    // Rename the .if files: `{id}.if` -> `{id}_{name}.if`.
    let mut n = 0;
    for (id, name) in &todo {
        let src = PathBuf::from(format!("{OUR_IF_DIR}/{id}.if"));
        let dst = PathBuf::from(format!("{OUR_IF_DIR}/{id}_{name}.if"));
        if src.exists() && !dst.exists() {
            fs::rename(&src, &dst).unwrap_or_else(|e| panic!("rename {src:?}: {e}"));
            n += 1;
        }
    }
    println!("renamed {n} .if file(s) + updated interface.pack");
}
