//! Generalized "adopt reference (Content-old / rev377) identifiers" for a config
//! type that has a display `name` (obj / loc / npc): rename our pack stems +
//! per-id config filenames to the reference stem for every entry that matches
//! 1:1 by cache id AND display name. Names are tooling-only → CRC-neutral.
//!
//! Two-phase rename (temp names first) so a target stem that equals another
//! not-yet-renamed entry's current filename can't be overwritten.
//!
//! Usage: `cargo run --release --example rename_configs_to_ref -p cache -- <obj|loc|npc> [--write]`
//! (dry run without `--write`).

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use cache::content::pack_file;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let kind = args.iter().skip(1).find(|a| !a.starts_with("--")).cloned().unwrap_or_default();
    let write = args.iter().any(|a| a == "--write");
    if !["obj", "loc", "npc"].contains(&kind.as_str()) {
        eprintln!("usage: rename_configs_to_ref -- <obj|loc|npc> [--write]");
        std::process::exit(2);
    }

    let ref_pack = pack_file::read(Path::new(&format!("reference/Content-old/pack/{kind}.pack")))
        .expect("read ref pack");
    let ref_names = parse_all_names(&format!("reference/Content-old/scripts/_unpack/377/all.{kind}"));
    let ref_id_name: HashMap<u32, String> = ref_pack
        .iter()
        .filter_map(|(&id, stem)| ref_names.get(stem).map(|n| (id, n.clone())))
        .collect();

    let our_pack_path = format!("Content/pack/{kind}.pack");
    let mut our_pack = pack_file::read(Path::new(&our_pack_path)).expect("read our pack");
    let our = index_ours(&format!("Content/config/{kind}"), &kind); // id -> (name, path)

    let norm = |s: &str| s.trim().to_lowercase();
    let mut matched: Vec<(u32, String)> = Vec::new();
    for (&id, ref_name) in &ref_id_name {
        if let Some((our_name, _)) = our.get(&id)
            && norm(ref_name) == norm(our_name)
            && let Some(ref_stem) = ref_pack.get(&id)
        {
            matched.push((id, ref_stem.clone()));
        }
    }
    matched.sort_by_key(|(id, _)| *id);
    let matched_ids: HashSet<u32> = matched.iter().map(|(id, _)| *id).collect();

    let mut taken: HashSet<String> = our_pack
        .iter()
        .filter(|(id, _)| !matched_ids.contains(id))
        .map(|(_, s)| s.clone())
        .collect();
    let mut renames: Vec<(u32, String, String)> = Vec::new();
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
        "[{kind}] {} ref-with-name · {} ours · {} matches · {} renamed · {} suffixed · {} already-correct",
        ref_id_name.len(),
        our.len(),
        matched.len(),
        renames.len(),
        suffixed,
        matched.len() - renames.len(),
    );
    for (id, old, new) in renames.iter().take(10) {
        println!("    {id:>6}: {old}  ->  {new}");
    }
    if !write {
        println!("(dry run — pass --write to rename {kind} files + {kind}.pack)");
        return;
    }

    for (id, _o, _n) in &renames {
        if let Some((_, path)) = our.get(id) {
            let tmp = path.with_file_name(format!("__ref_tmp_{id}.{kind}"));
            fs::rename(path, &tmp).unwrap_or_else(|e| panic!("tmp {path:?}: {e}"));
        }
    }
    let mut n = 0usize;
    for (id, _o, new) in &renames {
        if let Some((_, path)) = our.get(id) {
            let tmp = path.with_file_name(format!("__ref_tmp_{id}.{kind}"));
            let dst = path.with_file_name(format!("{new}.{kind}"));
            fs::rename(&tmp, &dst).unwrap_or_else(|e| panic!("{tmp:?} -> {dst:?}: {e}"));
            n += 1;
        }
        our_pack.insert(*id, new.clone());
    }
    pack_file::write(Path::new(&our_pack_path), &our_pack).expect("write pack");
    println!("renamed {n} .{kind} files; updated {kind}.pack");
}

fn parse_all_names(path: &str) -> HashMap<String, String> {
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

fn index_ours(dir: &str, kind: &str) -> HashMap<u32, (String, PathBuf)> {
    let header = format!("// {kind} ");
    let mut out = HashMap::new();
    let mut stack = vec![PathBuf::from(dir)];
    while let Some(d) = stack.pop() {
        let Ok(rd) = fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().and_then(|x| x.to_str()) == Some(kind)
                && let Some((id, name)) = read_id_name(&p, &header)
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
