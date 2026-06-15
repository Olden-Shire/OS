//! Reformat npc list fields to content-old's indexed-line form.
//!
//! The cache codec used to render npc multi-value fields as one joined line:
//!   `models = a b c` · `headmodels = a b` · `recol = s/d s/d` · `retex = …`
//!   `walkanims = a, b, c, d`
//! content-old (and the matching codec change in `config_text.rs`) breaks them
//! into one indexed line each — `model1`/`head1`, `recol1s`/`recol1d`, and the
//! 4-direction set as `walkanim` — so this rewrites the on-disk `.npc` text to
//! that shape. Every other line (name, ops, single anims, server-only keys,
//! comments) is passed through verbatim.
//!
//! Byte-neutral: the new codec re-encodes the indexed form to the exact same
//! cache bytes (verify with `--example verify_content -- Content`). Idempotent.
//!
//! Usage: `cargo run --release --example reformat_npc_fields -p cache [-- --write]`
//! (dry run without `--write`).

use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let write = std::env::args().any(|a| a == "--write");
    let npc_dir = PathBuf::from("Content/config/npc");
    let files = npc_files(&npc_dir);

    let mut changed_files = 0usize;
    let mut sample: Option<(PathBuf, String, String)> = None;
    for path in &files {
        let text = fs::read_to_string(path).expect("read .npc");
        let new = reformat(&text);
        if new == text {
            continue;
        }
        changed_files += 1;
        if sample.is_none() {
            sample = Some((path.clone(), text.clone(), new.clone()));
        }
        if write {
            fs::write(path, &new).expect("write .npc");
        }
    }

    println!("{} npc files · {changed_files} reformatted", files.len());
    if let Some((path, before, after)) = sample {
        println!("\n── sample: {} ──\nBEFORE:\n{before}\nAFTER:\n{after}", path.display());
    }
    if !write {
        println!("(dry run — pass --write to rewrite the .npc files)");
    }
}

/// Rewrite the list fields of one `.npc` file's text to the indexed form.
fn reformat(text: &str) -> String {
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
            "models" => emit_list(&mut out, "model", val),
            "headmodels" => emit_list(&mut out, "head", val),
            "recol" => emit_pairs(&mut out, "recol", val),
            "retex" => emit_pairs(&mut out, "retex", val),
            "walkanims" => {
                out.push_str("walkanim = ");
                out.push_str(val);
                out.push('\n');
            }
            _ => {
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    out
}

/// `a b c` → `<stem>1 = a` / `<stem>2 = b` / … (one line each).
fn emit_list(out: &mut String, stem: &str, val: &str) {
    for (i, tok) in val.split_whitespace().enumerate() {
        out.push_str(&format!("{stem}{} = {tok}\n", i + 1));
    }
}

/// `s/d s/d` → `<stem>1s = s` / `<stem>1d = d` / `<stem>2s = …` (one per half).
fn emit_pairs(out: &mut String, stem: &str, val: &str) {
    for (i, pair) in val.split_whitespace().enumerate() {
        let (s, d) = pair.split_once('/').expect("recol pair has '/'");
        out.push_str(&format!("{stem}{}s = {s}\n", i + 1));
        out.push_str(&format!("{stem}{}d = {d}\n", i + 1));
    }
}

/// Every `*.npc` under `dir`, recursing shard subdirs.
fn npc_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = fs::read_dir(&d) else { continue };
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().and_then(|s| s.to_str()) == Some("npc") {
                out.push(p);
            }
        }
    }
    out
}
