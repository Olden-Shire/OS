//! Map our (rev1/2007) seq ids to reference (Content-old/rev377) seq NAMES.
//! Seq frame ids + the binary layout diverge across revisions (rev1 packs
//! `frameset<<16|frame`; rev377 stores flat anim ids), so a byte-compare is
//! impossible — and pure structure is too common to be unique. But seq IDS are
//! mostly STABLE across these close revisions, so we anchor on the shared id
//! (from the reference seq.pack) and VERIFY by structure (loop/frame/iframe
//! counts + per-frame delays, which survive the rev change). A name is adopted
//! only when our seq at that id structurally matches the reference's — so an id
//! that was reused for a different anim in rev1 is left unnamed, not mislabelled.
//!
//! Rename = tooling-only (cache keys by id): seq.pack `{id}={id}` -> `{id}={name}`.
//! CRC-identical on repack. Idempotent. The reference's first-frame delay is
//! often omitted (defaulted), so we compare delays from the 2nd frame on.
//!
//! Usage: `cargo run --release --example rename_seqs_to_ref -p cache [-- --write]`

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

const REF_ALL_SEQ: &str = "reference/Content-old/scripts/_unpack/377/all.seq";
const REF_PACK: &str = "reference/Content-old/pack/seq.pack";
const OUR_SEQ_DIR: &str = "Content/config/seq";
const OUR_PACK: &str = "Content/pack/seq.pack";

/// Rev-robust structural fingerprint of a seq.
#[derive(Clone, PartialEq, Eq, Hash)]
struct Sig {
    loops: i32,
    frames: usize,
    iframes: usize,
    /// Delays from the 2nd frame onward (the 1st is often omitted in the ref).
    tail_delays: Vec<i32>,
}

fn main() {
    let write = std::env::args().any(|a| a == "--write");

    // Reference: name -> sig, and id -> name (from the reference seq.pack).
    let ref_text = fs::read_to_string(REF_ALL_SEQ).expect("read all.seq");
    let ref_sigs: HashMap<String, Sig> = parse_ref_blocks(&ref_text).into_iter().collect();
    let ref_pack = fs::read_to_string(REF_PACK).expect("read ref seq.pack");
    let ref_id_name: Vec<(i32, String)> = ref_pack.lines().filter_map(|l| {
        let (k, v) = l.split_once('=')?;
        Some((k.trim().parse().ok()?, v.trim().to_string()))
    }).collect();

    // Ours: id -> (sig, path). One `{id}.seq` file per id, sharded into subdirs.
    let mut our_sig: HashMap<i32, Sig> = HashMap::new();
    let mut our_path: HashMap<i32, PathBuf> = HashMap::new();
    for path in list_seq_files(OUR_SEQ_DIR) {
        let Some(id) = path.file_stem().and_then(|s| s.to_str()).and_then(|s| s.parse::<i32>().ok())
        else { continue };
        let Ok(text) = fs::read_to_string(&path) else { continue };
        if let Some(sig) = sig_of_ours(&text) {
            our_sig.insert(id, sig);
            our_path.insert(id, path);
        }
    }

    // Anchor on the shared id, verify by structure.
    let mut renames: Vec<(i32, String)> = Vec::new();
    let (mut id_mismatch, mut id_absent) = (0usize, 0usize);
    for (id, name) in &ref_id_name {
        let Some(refsig) = ref_sigs.get(name) else { continue };
        match our_sig.get(id) {
            Some(oursig) if oursig == refsig => renames.push((*id, name.clone())),
            Some(_) => id_mismatch += 1,   // id reused for a different anim in rev1
            None => id_absent += 1,        // id not present in our cache
        }
    }
    renames.sort_by_key(|(id, _)| *id);

    println!("{} ref named seqs · {} ours · {} id+structure matched · {id_mismatch} id reused (struct differs) · {id_absent} ref id absent here",
        ref_id_name.len(), our_sig.len(), renames.len());
    // Spot-check the chathead talk family.
    for (id, name) in &renames {
        if name.starts_with("chatneu") || name.starts_with("chathap") || name.starts_with("chatcon") {
            println!("    {id:>5} -> {name}");
        }
    }

    if !write {
        println!("(dry run — pass --write to update seq.pack)");
        return;
    }

    // Rewrite seq.pack: `{id}={id}` -> `{id}={name}` for matched ids.
    let by_id: HashMap<i32, &str> = renames.iter().map(|(i, n)| (*i, n.as_str())).collect();
    let pack = fs::read_to_string(OUR_PACK).expect("read seq.pack");
    let mut out = String::with_capacity(pack.len());
    let mut changed = 0;
    for line in pack.lines() {
        if let Some((k, _v)) = line.split_once('=') {
            if let Ok(id) = k.trim().parse::<i32>() {
                if let Some(&name) = by_id.get(&id) {
                    out.push_str(&format!("{id}={name}\n"));
                    changed += 1;
                    continue;
                }
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    fs::write(OUR_PACK, out).expect("write seq.pack");
    println!("seq.pack: {changed} ids named");

    // Rename the `.seq` files to match — the packer resolves a group's file by
    // its pack stem (single_file_stem), so a named pack entry needs a `{name}.seq`
    // file. Stays in the same shard dir; names are unique + non-numeric so they
    // can't collide with a remaining `{id}.seq`. Idempotent (skips if done).
    let mut renamed = 0;
    for (id, name) in &renames {
        let Some(src) = our_path.get(id) else { continue };
        let dst = src.with_file_name(format!("{name}.seq"));
        if src == &dst || !src.exists() {
            continue;
        }
        fs::rename(src, &dst).unwrap_or_else(|e| panic!("rename {src:?} -> {dst:?}: {e}"));
        renamed += 1;
    }
    println!("renamed {renamed} .seq files");
}

/// Parse our compact `.seq` text into a Sig.
fn sig_of_ours(text: &str) -> Option<Sig> {
    let mut loops = -1;
    let mut frames = 0usize;
    let mut iframes = 0usize;
    let mut delays: Vec<i32> = Vec::new();
    for line in text.lines() {
        let l = line.trim();
        if let Some(v) = l.strip_prefix("loops = ") {
            loops = v.trim().parse().unwrap_or(-1);
        } else if let Some(v) = l.strip_prefix("frames = ") {
            let toks: Vec<&str> = v.split_whitespace().collect();
            frames = toks.len();
            delays = toks.iter().filter_map(|t| t.split('/').next()?.parse().ok()).collect();
        } else if let Some(v) = l.strip_prefix("iframes = ") {
            iframes = v.split_whitespace().count();
        }
    }
    if frames == 0 {
        return None;
    }
    Some(Sig { loops, frames, iframes, tail_delays: delays.into_iter().skip(1).collect() })
}

/// Parse the reference all.seq `[name] … ` blocks into (name, Sig).
fn parse_ref_blocks(text: &str) -> Vec<(String, Sig)> {
    let mut out = Vec::new();
    let mut name: Option<String> = None;
    let mut loops = -1;
    let mut frames = 0usize;
    let mut iframes = 0usize;
    // delay by frame index -> value, so we can emit them in frame order.
    let mut delays: HashMap<usize, i32> = HashMap::new();
    let flush = |out: &mut Vec<(String, Sig)>, name: &Option<String>, loops, frames, iframes, delays: &HashMap<usize, i32>| {
        if let Some(n) = name {
            if frames > 0 {
                // Emit delays in frame order from index 2 on (1st often omitted).
                let mut tail: Vec<i32> = Vec::new();
                for i in 2..=frames {
                    tail.push(*delays.get(&i).unwrap_or(&0));
                }
                out.push((n.clone(), Sig { loops, frames, iframes, tail_delays: tail }));
            }
        }
    };
    for line in text.lines() {
        let l = line.trim();
        if let Some(inner) = l.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            flush(&mut out, &name, loops, frames, iframes, &delays);
            name = Some(inner.to_string());
            (loops, frames, iframes) = (-1, 0, 0);
            delays.clear();
        } else if let Some(v) = l.strip_prefix("loops=") {
            loops = v.trim().parse().unwrap_or(-1);
        } else if let Some(rest) = l.strip_prefix("frame") {
            if let Some((idx, _)) = rest.split_once('=') {
                if idx.chars().all(|c| c.is_ascii_digit()) {
                    frames = frames.max(idx.parse::<usize>().unwrap_or(0));
                }
            }
        } else if let Some(rest) = l.strip_prefix("iframe") {
            if let Some((idx, _)) = rest.split_once('=') {
                if idx.chars().all(|c| c.is_ascii_digit()) {
                    iframes = iframes.max(idx.parse::<usize>().unwrap_or(0));
                }
            }
        } else if let Some(rest) = l.strip_prefix("delay") {
            if let Some((idx, val)) = rest.split_once('=') {
                if let (Ok(i), Ok(v)) = (idx.parse::<usize>(), val.trim().parse::<i32>()) {
                    delays.insert(i, v);
                }
            }
        }
    }
    flush(&mut out, &name, loops, frames, iframes, &delays);
    out
}

fn list_seq_files(dir: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![PathBuf::from(dir)];
    while let Some(d) = stack.pop() {
        let Ok(rd) = fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().and_then(|s| s.to_str()) == Some("seq") {
                out.push(p);
            }
        }
    }
    out
}
