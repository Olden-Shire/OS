//! Port the SERVER-side config from the reference (Content-old / rev377)
//! `all.<type>` onto our matching `.<type>` files (npc / obj / loc). We already
//! own the client-cache config; what we lack is the server config — for npc the
//! combat stats / AI / params, for obj the weight / wearpos / equipment params
//! (bonuses, death_drop), for loc the door/stage params. `desc` + `category` too.
//!
//! Matches are same-id + same-display-name. For each match we MERGE: add the
//! reference's server keys our file is missing, under a `//Server` line, without
//! clobbering any server key/param we already authored. Server keys are skipped
//! by the cache codec, so this stays CRC-IDENTICAL.
//!
//! Usage: `cargo run --release --example port_ref_server_props -p cache -- <npc|obj|loc> [--write]`

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use cache::content::pack_file;

/// Server-only keys per config type (everything else in all.<type> is client
/// cache config we already have). `param` is handled separately (multi-valued).
fn server_keys(kind: &str) -> &'static [&'static str] {
    match kind {
        "npc" => &[
            "wanderrange", "maxrange", "huntrange", "timer", "respawnrate", "moverestrict",
            "attackrange", "blockwalk", "huntmode", "defaultmode", "members", "patrol",
            "givechase", "regenrate", "category", "debugname", "desc",
            "hitpoints", "attack", "strength", "defence", "ranged", "magic",
        ],
        "obj" => &[
            "desc", "weight", "tradeable", "category",
            "wearpos", "wearpos2", "wearpos3", "dummyitem", "respawnrate",
        ],
        "loc" => &["desc", "category"],
        _ => &[],
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let kind = args.iter().skip(1).find(|a| !a.starts_with("--")).cloned().unwrap_or_default();
    let write = args.iter().any(|a| a == "--write");
    if !["npc", "obj", "loc"].contains(&kind.as_str()) {
        eprintln!("usage: port_ref_server_props -- <npc|obj|loc> [--write]");
        std::process::exit(2);
    }
    let ref_pack_path = format!("reference/Content-old/pack/{kind}.pack");
    let ref_all = format!("reference/Content-old/scripts/_unpack/377/all.{kind}");
    let our_dir = format!("Content/config/{kind}");
    let header = format!("// {kind} ");

    let ref_pack = pack_file::read(Path::new(&ref_pack_path)).expect("read ref pack");
    let (ref_sections, ref_names) = parse_all(&ref_all, server_keys(&kind));
    let ref_id_lines: HashMap<u32, Vec<(String, String)>> = ref_pack
        .iter()
        .filter_map(|(&id, stem)| ref_sections.get(stem).map(|l| (id, l.clone())))
        .collect();
    let ref_id_name: HashMap<u32, String> = ref_pack
        .iter()
        .filter_map(|(&id, stem)| ref_names.get(stem).map(|n| (id, n.clone())))
        .collect();

    let our = index_ours(&our_dir, &header);
    let norm = |s: &str| s.trim().to_lowercase();
    let (mut files_changed, mut keys_added, mut skipped) = (0usize, 0usize, 0usize);

    for (&id, (our_name, path)) in &our {
        let (Some(ref_name), Some(ref_lines)) = (ref_id_name.get(&id), ref_id_lines.get(&id)) else {
            continue;
        };
        if norm(ref_name) != norm(our_name) {
            skipped += 1;
            continue;
        }
        let text = fs::read_to_string(path).expect("read config");
        let (present_scalar, present_params) = existing_server(&text);
        let to_add: Vec<&(String, String)> = ref_lines
            .iter()
            .filter(|(k, v)| {
                if k == "param" {
                    !present_params.contains(v.split(',').next().unwrap_or("").trim())
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
            fs::write(path, out).expect("write config");
        }
    }

    println!(
        "[{kind}] {files_changed} gained server config · {keys_added} keys added · {skipped} skipped (name mismatch)",
    );
    if !write {
        println!("(dry run — pass --write to merge server keys into the .{kind} files)");
    }
}

/// Parse all.<type>: stem -> server (key,value) lines + stem -> display name.
fn parse_all(
    path: &str,
    keys: &[&str],
) -> (HashMap<String, Vec<(String, String)>>, HashMap<String, String>) {
    let server: HashSet<&str> = keys.iter().copied().collect();
    let (mut lines, mut names) = (HashMap::new(), HashMap::new());
    let Ok(text) = fs::read_to_string(path) else {
        eprintln!("WARN: could not read {path}");
        return (lines, names);
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
            if k == "name" {
                names.insert(stem.clone(), v.to_string());
            }
            if k == "param" || server.contains(k) {
                lines.entry(stem.clone()).or_insert_with(Vec::new).push((k.to_string(), v.to_string()));
            }
        }
    }
    (lines, names)
}

/// Server keys already in one of our files: (scalar keys, param names).
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

fn index_ours(dir: &str, header: &str) -> HashMap<u32, (String, PathBuf)> {
    let ext = header.trim_start_matches("// ").trim();
    let mut out = HashMap::new();
    let mut stack = vec![PathBuf::from(dir)];
    while let Some(d) = stack.pop() {
        let Ok(rd) = fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().and_then(|x| x.to_str()) == Some(ext)
                && let Some((id, name)) = read_id_name(&p, header)
            {
                out.insert(id, (name, p));
            }
        }
    }
    out
}

fn read_id_name(path: &Path, header: &str) -> Option<(u32, String)> {
    let text = fs::read_to_string(path).ok()?;
    let mut id = None;
    let mut name = None;
    for line in text.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix(header) {
            id = rest.trim().parse::<u32>().ok();
        } else if let Some((k, v)) = t.split_once('=')
            && k.trim() == "name"
        {
            name = Some(v.trim().to_string());
        }
    }
    Some((id?, name?))
}
