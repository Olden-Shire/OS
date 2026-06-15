//! Port the SERVER-side npc config from the reference (Content-old / rev377)
//! `all.npc` onto our matching `.npc` files. We already own the client-cache
//! config (models/recol/anims/…); what we lack is the server config — combat
//! stats, AI ranges, `category`, `desc`, and the `param=` lines.
//!
//! Matches are the same-id + same-display-name set (see `match_ref_npcs`). For
//! each match we MERGE: add the reference's server keys our file is missing,
//! under a `//Server` line, WITHOUT clobbering any server key/param we already
//! authored. Server keys are tooling-only (skipped by the cache codec), so this
//! stays CRC-IDENTICAL.
//!
//! Usage: `cargo run --release --example port_ref_server_props -p cache [-- --write]`

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use cache::content::pack_file;

const REF_PACK: &str = "reference/Content-old/pack/npc.pack";
const REF_ALL: &str = "reference/Content-old/scripts/_unpack/377/all.npc";
const OUR_NPC_DIR: &str = "Content/config/npc";

/// Server-only npc keys (everything else in all.npc is client-cache config we
/// already have). Mirrors cache `NPC_SERVER_KEYS` (+ `param`, handled separately).
const SERVER_KEYS: &[&str] = &[
    "wanderrange", "maxrange", "huntrange", "timer", "respawnrate", "moverestrict",
    "attackrange", "blockwalk", "huntmode", "defaultmode", "members", "patrol",
    "givechase", "regenrate", "category", "debugname", "desc",
    "hitpoints", "attack", "strength", "defence", "ranged", "magic",
];

fn main() {
    let write = std::env::args().any(|a| a == "--write");

    let ref_pack = pack_file::read(Path::new(REF_PACK)).expect("read ref npc.pack");
    let ref_sections = parse_all_npc(REF_ALL); // stem -> server lines (in order)
    let our = index_our_npcs(OUR_NPC_DIR); // id -> (display name, path)

    // ref id -> server lines (only ids with an all.npc section).
    let ref_id_lines: HashMap<u32, Vec<(String, String)>> = ref_pack
        .iter()
        .filter_map(|(&id, stem)| ref_sections.get(stem).map(|l| (id, l.clone())))
        .collect();
    // ref id -> display name (the `name=` line), for the match check.
    let ref_id_name = parse_all_npc_names(REF_ALL);
    let ref_id_name: HashMap<u32, String> = ref_pack
        .iter()
        .filter_map(|(&id, stem)| ref_id_name.get(stem).map(|n| (id, n.clone())))
        .collect();

    let norm = |s: &str| s.trim().to_lowercase();
    let (mut files_changed, mut keys_added, mut skipped_no_match) = (0usize, 0usize, 0usize);

    for (&id, (our_name, path)) in &our {
        let (Some(ref_name), Some(ref_lines)) = (ref_id_name.get(&id), ref_id_lines.get(&id)) else {
            continue;
        };
        if norm(ref_name) != norm(our_name) {
            skipped_no_match += 1;
            continue;
        }
        let text = fs::read_to_string(path).expect("read .npc");
        let (present_scalar, present_params) = existing_server(&text);
        // Reference server lines we don't already have.
        let to_add: Vec<&(String, String)> = ref_lines
            .iter()
            .filter(|(k, v)| {
                if k == "param" {
                    let pname = v.split(',').next().unwrap_or("").trim();
                    !present_params.contains(pname)
                } else {
                    !present_scalar.contains(k.as_str())
                }
            })
            .collect();
        if to_add.is_empty() {
            continue;
        }
        files_changed += 1;
        keys_added += to_add.len();
        if write {
            let mut out = text.trim_end().to_string();
            if !text.lines().any(|l| l.trim() == "//Server") {
                out.push_str("\n//Server");
            }
            for (k, v) in to_add {
                out.push_str(&format!("\n{k}={v}"));
            }
            out.push('\n');
            fs::write(path, out).expect("write .npc");
        }
    }

    println!(
        "{} npcs gained server config · {keys_added} keys added · {skipped_no_match} skipped (name mismatch)",
        files_changed,
    );
    if !write {
        println!("(dry run — pass --write to merge server keys into the .npc files)");
    }
}

/// Parse `all.npc` into stem -> server `(key, value)` lines (client keys dropped,
/// order preserved). Params keep their full `name,value` value.
fn parse_all_npc(path: &str) -> HashMap<String, Vec<(String, String)>> {
    let server: HashSet<&str> = SERVER_KEYS.iter().copied().collect();
    let mut out: HashMap<String, Vec<(String, String)>> = HashMap::new();
    let Ok(text) = fs::read_to_string(path) else {
        eprintln!("WARN: could not read {path}");
        return out;
    };
    let mut cur: Option<String> = None;
    for line in text.lines() {
        let t = line.trim();
        if let Some(inner) = t.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            cur = Some(inner.trim().to_string());
        } else if let Some((k, v)) = t.split_once('=')
            && let Some(stem) = &cur
        {
            let (k, v) = (k.trim(), v.trim());
            if k == "param" || server.contains(k) {
                out.entry(stem.clone()).or_default().push((k.to_string(), v.to_string()));
            }
        }
    }
    out
}

/// `all.npc` stem -> display `name=`.
fn parse_all_npc_names(path: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let Ok(text) = fs::read_to_string(path) else { return out };
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

/// Server keys already in one of our `.npc` files: (scalar keys, param names).
fn existing_server(text: &str) -> (HashSet<String>, HashSet<String>) {
    let (mut scalar, mut params) = (HashSet::new(), HashSet::new());
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("//") {
            continue;
        }
        if let Some((k, v)) = t.split_once('=') {
            let k = k.trim();
            if k == "param" {
                params.insert(v.split(',').next().unwrap_or("").trim().to_string());
            } else {
                scalar.insert(k.to_string());
            }
        }
    }
    (scalar, params)
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
