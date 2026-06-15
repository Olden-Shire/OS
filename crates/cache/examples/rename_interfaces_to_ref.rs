//! Remap our (2007) interface identifiers from the reference (Content-old /
//! rev377) interface NAMES, matched by COMPONENT STRUCTURE with extreme
//! confidence. Interfaces have no `name=`, and the ids diverge across revisions
//! (content-old `multi3`=2469 isn't in our cache) — but the dialogue interfaces
//! share an architecture, so we match on the structural signature:
//!   (component count, type multiset, the (type,x,y,w,h) of every text+model
//!    component — the semantic layout, stable across revs; decorative
//!    layer/graphic chrome differs in order so it's excluded from the fingerprint)
//! A content-old name is adopted only when it matches EXACTLY ONE of our
//! interfaces (and that interface matches exactly one name) — else skipped.
//!
//! Rename = tooling-only (cache keys by id): `{id}.if` -> `{id}_{name}.if`,
//! interface.pack `if_{id}` -> `{name}` (+ each `if_{id}:com_N` -> `{name}:com_N`).
//! CRC-IDENTICAL on repack. Idempotent.
//!
//! Usage: `cargo run --release --example rename_interfaces_to_ref -p cache [-- --write]`

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

const REF_IF_DIR: &str = "reference/Content-old/scripts/interface_chat/interfaces";
const OUR_IF_DIR: &str = "Content/interfaces";
const OUR_PACK: &str = "Content/pack/interface.pack";

/// content-old type name -> our numeric type.
fn type_num(name: &str) -> Option<i32> {
    Some(match name {
        "layer" => 0,
        "inv" => 2,
        "rectangle" | "rect" => 3,
        "text" => 4,
        "graphic" => 5,
        "model" => 6,
        _ => return None,
    })
}

/// A component's structural tuple.
#[derive(Clone, PartialEq, Eq, Hash)]
struct Comp {
    t: i32,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

/// The extreme-confidence fingerprint of an interface.
#[derive(Clone, PartialEq, Eq, Hash)]
struct Sig {
    count: usize,
    types: Vec<i32>,         // sorted type multiset
    semantic: Vec<Comp>,     // sorted text(4)+model(6) comps
}

fn main() {
    let write = std::env::args().any(|a| a == "--write");

    // Reference: name -> signature.
    let ref_sigs: Vec<(String, Sig)> = list_if(REF_IF_DIR)
        .into_iter()
        .filter_map(|p| {
            let name = p.file_stem()?.to_str()?.to_string();
            sig_of(&p, true).map(|s| (name, s))
        })
        .collect();

    // Ours: signature -> [ids] (id parsed from `{id}.if` / `{id}_{name}.if`).
    let mut our_by_sig: HashMap<Sig, Vec<(u32, PathBuf)>> = HashMap::new();
    for p in list_if(OUR_IF_DIR) {
        let Some(id) = leading_id(&p) else { continue };
        if let Some(s) = sig_of(&p, false) {
            our_by_sig.entry(s).or_default().push((id, p));
        }
    }

    // Match: a name claims our id iff the signature is unique on BOTH sides.
    let mut renames: Vec<(u32, String, PathBuf)> = Vec::new(); // (id, name, path)
    let mut claimed: HashMap<u32, String> = HashMap::new();
    let (mut no_match, mut ambiguous) = (Vec::new(), Vec::new());
    for (name, sig) in &ref_sigs {
        match our_by_sig.get(sig).map(Vec::as_slice) {
            Some([(id, path)]) if !claimed.contains_key(id) => {
                claimed.insert(*id, name.clone());
                renames.push((*id, name.clone(), path.clone()));
            }
            Some(many) if many.len() > 1 => ambiguous.push(name.clone()),
            _ => no_match.push(name.clone()),
        }
    }
    renames.sort_by_key(|(id, _, _)| *id);

    println!("{} ref interfaces · {} matched 1:1", ref_sigs.len(), renames.len());
    for (id, name, _) in &renames {
        println!("    {id:>6} -> {name}");
    }
    if !no_match.is_empty() {
        println!("no structural match: {}", no_match.join(", "));
    }
    if !ambiguous.is_empty() {
        println!("ambiguous (>1 ours): {}", ambiguous.join(", "));
    }
    if !write {
        println!("(dry run — pass --write to rename .if files + interface.pack)");
        return;
    }

    // Rewrite interface.pack: `if_{id}` -> `{name}` for the group + each component.
    let pack = fs::read_to_string(OUR_PACK).expect("read interface.pack");
    let by_id: HashMap<u32, &str> = renames.iter().map(|(id, n, _)| (*id, n.as_str())).collect();
    let mut out = String::with_capacity(pack.len());
    for line in pack.lines() {
        if let Some((key, val)) = line.split_once('=') {
            // val is `if_{id}` or `if_{id}:com_N`
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
    for (id, name, path) in &renames {
        let dst = path.with_file_name(format!("{id}_{name}.if"));
        if *path != dst {
            fs::rename(path, &dst).unwrap_or_else(|e| panic!("rename {path:?}: {e}"));
            n += 1;
        }
    }
    println!("renamed {n} .if files + updated interface.pack");
}

/// Parse one `.if` into its signature. `named` = content-old (type names).
fn sig_of(path: &Path, named: bool) -> Option<Sig> {
    let text = fs::read_to_string(path).ok()?;
    let mut comps: Vec<Comp> = Vec::new();
    let (mut t, mut x, mut y, mut w, mut h) = (None, 0, 0, 0, 0);
    let mut in_comp = false;
    let flush = |comps: &mut Vec<Comp>, t: Option<i32>, x, y, w, h| {
        if let Some(t) = t {
            comps.push(Comp { t, x, y, w, h });
        }
    };
    for line in text.lines() {
        let l = line.trim();
        if l.starts_with("[com_") {
            if in_comp {
                flush(&mut comps, t, x, y, w, h);
            }
            (t, x, y, w, h) = (None, 0, 0, 0, 0);
            in_comp = true;
        } else if let Some((k, v)) = l.split_once('=') {
            match k.trim() {
                "type" => t = if named { type_num(v.trim()) } else { v.trim().parse().ok() },
                "x" => x = v.trim().parse().unwrap_or(0),
                "y" => y = v.trim().parse().unwrap_or(0),
                "width" => w = v.trim().parse().unwrap_or(0),
                "height" => h = v.trim().parse().unwrap_or(0),
                _ => {}
            }
        }
    }
    if in_comp {
        flush(&mut comps, t, x, y, w, h);
    }
    if comps.is_empty() {
        return None;
    }
    let mut types: Vec<i32> = comps.iter().map(|c| c.t).collect();
    types.sort_unstable();
    let mut semantic: Vec<Comp> = comps.iter().filter(|c| c.t == 4 || c.t == 6).cloned().collect();
    semantic.sort_unstable_by_key(|c| (c.t, c.x, c.y, c.w, c.h));
    Some(Sig { count: comps.len(), types, semantic })
}

fn list_if(dir: &str) -> Vec<PathBuf> {
    let Ok(rd) = fs::read_dir(dir) else { return Vec::new() };
    rd.flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("if"))
        .collect()
}

/// Leading numeric id of `{id}.if` or `{id}_{name}.if`.
fn leading_id(path: &Path) -> Option<u32> {
    let stem = path.file_stem()?.to_str()?;
    stem.split('_').next()?.parse().ok()
}
