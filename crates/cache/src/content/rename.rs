//! On-disk side of an interface rename refactor (the IntelliJ plugin / jaged).
//!
//! An interface symbol's NAME lives in several places in the Content tree; a
//! rename has to edit all of them as one operation:
//!   - `pack/interface.pack` — the .rs2/.cs2 symbol table (root + component lines)
//!   - the `.if` filename — hybrid `{id}.if` (default `if_{id}`) / `{id}_{name}.if`
//!   - the `.if` component section header — `[com_N]` / `[com_N name]`
//!   - the `.rs2`/`.cs2` source REFERENCES (word-boundary aware)
//!
//! The `_meta.json` manifest needs NO edit: pack resolves the `.if` by id
//! (see `resolve_interface_if`), like the config tree resolves by pack stem.
//!
//! Names are tooling-only — they never reach the packed cache bytes, so a
//! rename leaves every group CRC-identical to the vanilla cache (proven by the
//! rename round-trip + verify tests).
//!
//! NOTE on `interface.pack` keys: components pack as `(id << 16) | child`, so
//! interface 0's components collide with low interface ids (`(0<<16)|2 == 2`).
//! That's fine for the compiler (it reads name→id, many names can share an id),
//! but means we must match pack lines by NAME, never by an id→name map.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Hybrid `.if` stem: bare `{id}` for the default name `if_{id}`, else
/// `{id}_{name}` (the id always leads, so files sort by id and the group id is
/// recoverable from the filename).
#[must_use]
pub fn if_stem(id: u32, name: &str) -> String {
    if name == format!("if_{id}") {
        id.to_string()
    } else {
        format!("{id}_{name}")
    }
}

/// What a rename touched.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct RenameReport {
    /// `interface.pack` lines rewritten (root + components).
    pub pack_lines_changed: usize,
    /// `(old, new)` `.if` filename, if it moved.
    pub file_renamed: Option<(String, String)>,
    /// `.if` component section header rewritten (component rename only).
    pub if_header_changed: bool,
    /// Symbol-reference occurrences rewritten across `.rs2`/`.cs2`.
    pub refs_changed: usize,
}

/// Rename a root interface `if_{id}` (or its current name) to `new_name`,
/// cascading to its components (`name:com_N`), the `.if` filename, and source
/// references.
pub fn rename_interface(content_dir: &Path, id: u32, new_name: &str) -> io::Result<RenameReport> {
    require_ident(new_name)?;
    let mut report = RenameReport::default();

    let pack_path = content_dir.join("pack").join("interface.pack");
    let text = fs::read_to_string(&pack_path)?;

    // Old root name = the `{id}=value` line whose value has no ':' (component
    // lines for the same id can share the key, but their value carries ':').
    let old_name = text
        .lines()
        .filter_map(split_pack_line)
        .find(|(k, v)| *k == id && !v.contains(':'))
        .map(|(_, v)| v.to_string())
        .ok_or_else(|| not_found(format!("interface {id} not in {}", pack_path.display())))?;
    if old_name == new_name {
        return Ok(report);
    }

    // Rewrite the pack: root (value == old_name) + components (value starts
    // `old_name:`). Match by value so interface-0 key collisions don't bite.
    let comp_prefix = format!("{old_name}:");
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let rewritten = match split_pack_line(line) {
            Some((k, v)) if v == old_name => {
                report.pack_lines_changed += 1;
                Some(format!("{k}={new_name}"))
            }
            Some((k, v)) if v.starts_with(&comp_prefix) => {
                report.pack_lines_changed += 1;
                Some(format!("{k}={new_name}:{}", &v[comp_prefix.len()..]))
            }
            _ => None,
        };
        out.push_str(rewritten.as_deref().unwrap_or(line));
        out.push('\n');
    }
    fs::write(&pack_path, out)?;

    // Rename the `.if` (hybrid stem), located by id so a prior rename is found.
    let if_dir = content_dir.join("interfaces");
    if let Some(cur) = find_if_file(&if_dir, id)? {
        let new_path = if_dir.join(format!("{}.if", if_stem(id, new_name)));
        if cur != new_path {
            fs::rename(&cur, &new_path)?;
            report.file_renamed = Some((file_label(&cur), file_label(&new_path)));
        }
    }

    // Source references: `if_549` -> `welcome` (also covers `if_549:com_2`,
    // whose `if_549` prefix is word-bounded by the ':').
    report.refs_changed += rename_refs(content_dir, &old_name, new_name)?;
    Ok(report)
}

/// Rename a component `if_{parent}:{old}` (child index `child`) to
/// `parent_name:new_name`, editing the pack line, the `.if` section header, and
/// source references. The parent interface keeps its name.
pub fn rename_component(
    content_dir: &Path,
    parent_id: u32,
    child: u32,
    new_name: &str,
) -> io::Result<RenameReport> {
    require_ident(new_name)?;
    let mut report = RenameReport::default();
    let packed = (parent_id << 16) | child;

    let pack_path = content_dir.join("pack").join("interface.pack");
    let text = fs::read_to_string(&pack_path)?;

    // The component line: key == packed AND value carries ':' (disambiguates
    // from a colliding low-id root). Value is `parent_name:old_comp_name`.
    let (parent_name, old_comp) = text
        .lines()
        .filter_map(split_pack_line)
        .find(|(k, v)| *k == packed && v.contains(':'))
        .and_then(|(_, v)| v.split_once(':').map(|(p, c)| (p.to_string(), c.to_string())))
        .ok_or_else(|| not_found(format!("component {parent_id}:com_{child} not in {}", pack_path.display())))?;
    if old_comp == new_name {
        return Ok(report);
    }

    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let rewritten = match split_pack_line(line) {
            Some((k, v)) if k == packed && v == format!("{parent_name}:{old_comp}") => {
                report.pack_lines_changed += 1;
                Some(format!("{k}={parent_name}:{new_name}"))
            }
            _ => None,
        };
        out.push_str(rewritten.as_deref().unwrap_or(line));
        out.push('\n');
    }
    fs::write(&pack_path, out)?;

    // `.if` section header: `[com_{child}]` or `[com_{child} <old>]` ->
    // `[com_{child} {new_name}]`.
    let if_dir = content_dir.join("interfaces");
    if let Some(path) = find_if_file(&if_dir, parent_id)? {
        let body = fs::read_to_string(&path)?;
        let mut changed = false;
        let mut nb = String::with_capacity(body.len());
        for line in body.lines() {
            let t = line.trim();
            if t.starts_with('[') && t.ends_with(']') {
                let inside = &t[1..t.len() - 1];
                if inside.split_whitespace().next() == Some(&format!("com_{child}")) {
                    nb.push_str(&format!("[com_{child} {new_name}]"));
                    nb.push('\n');
                    changed = true;
                    continue;
                }
            }
            nb.push_str(line);
            nb.push('\n');
        }
        if changed {
            fs::write(&path, nb)?;
            report.if_header_changed = true;
        }
    }

    // References: `parent_name:old_comp` -> `parent_name:new_name`.
    report.refs_changed += rename_refs(
        content_dir,
        &format!("{parent_name}:{old_comp}"),
        &format!("{parent_name}:{new_name}"),
    )?;
    Ok(report)
}

// ── helpers ───────────────────────────────────────────────────────────────

/// Parse one `interface.pack` line to `(key, value)`, skipping comments/blanks.
fn split_pack_line(raw: &str) -> Option<(u32, &str)> {
    let line = raw.split("//").next().unwrap_or("").trim();
    if line.is_empty() {
        return None;
    }
    let (k, v) = line.split_once('=')?;
    let key = k.trim().parse::<u32>().ok()?;
    Some((key, v.trim()))
}

/// `{id}.if`, else the first `{id}_*.if` (a renamed file).
fn find_if_file(dir: &Path, id: u32) -> io::Result<Option<PathBuf>> {
    let bare = dir.join(format!("{id}.if"));
    if bare.exists() {
        return Ok(Some(bare));
    }
    let prefix = format!("{id}_");
    if dir.is_dir() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with(&prefix) && name.ends_with(".if") {
                return Ok(Some(entry.path()));
            }
        }
    }
    Ok(None)
}

fn file_label(p: &Path) -> String {
    p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Word-boundary replace of symbol `old` with `new` across `.rs2` (under
/// `scripts/`) and `.cs2` (under `clientscripts/`). `old` may be a component
/// symbol `parent:child`; the match is bounded by non-identifier chars on both
/// sides (`:` is a boundary, so `if_5` never matches inside `if_549`).
fn rename_refs(content_dir: &Path, old: &str, new: &str) -> io::Result<usize> {
    let mut total = 0;
    for (sub, ext) in [("scripts", "rs2"), ("clientscripts", "cs2")] {
        let dir = content_dir.join(sub);
        if !dir.is_dir() {
            continue;
        }
        for path in walk_ext(&dir, ext)? {
            let text = fs::read_to_string(&path)?;
            let (replaced, n) = replace_symbol(&text, old, new);
            if n > 0 {
                fs::write(&path, replaced)?;
                total += n;
            }
        }
    }
    Ok(total)
}

/// Recursively collect files with `ext` under `dir`.
fn walk_ext(dir: &Path, ext: &str) -> io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            out.extend(walk_ext(&path, ext)?);
        } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case(ext)) {
            out.push(path);
        }
    }
    Ok(out)
}

/// Replace whole-symbol occurrences of `old` with `new`, returning the new
/// text and the count. A match must be bounded by a non-identifier byte (or
/// string edge) on both sides — an identifier byte is `[A-Za-z0-9_]`. (`:` and
/// every other punctuation are boundaries, so `parent:child` symbols work.)
fn replace_symbol(text: &str, old: &str, new: &str) -> (String, usize) {
    if old.is_empty() {
        return (text.to_string(), 0);
    }
    let bytes = text.as_bytes();
    let ob = old.as_bytes();
    let is_ident_byte = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let mut count = 0;
    while i < bytes.len() {
        if bytes[i..].starts_with(ob) {
            let before_ok = i == 0 || !is_ident_byte(bytes[i - 1]);
            let after = i + ob.len();
            let after_ok = after >= bytes.len() || !is_ident_byte(bytes[after]);
            if before_ok && after_ok {
                out.push_str(new);
                i = after;
                count += 1;
                continue;
            }
        }
        // Push one UTF-8 char's worth so we never split a multibyte sequence.
        let ch = text[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    (out, count)
}

fn require_ident(name: &str) -> io::Result<()> {
    let ok = !name.is_empty()
        && name.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if ok {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid identifier: {name:?}"),
        ))
    }
}

fn not_found(msg: String) -> io::Error {
    io::Error::new(io::ErrorKind::NotFound, msg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stem_is_hybrid() {
        assert_eq!(if_stem(549, "if_549"), "549");
        assert_eq!(if_stem(549, "welcome"), "549_welcome");
        assert_eq!(if_stem(0, "if_0"), "0");
    }

    #[test]
    fn symbol_replace_is_word_bounded() {
        let (out, n) = replace_symbol("if_opentop(if_549); if_5490; if_549:com_2", "if_549", "welcome");
        assert_eq!(n, 2);
        assert_eq!(out, "if_opentop(welcome); if_5490; welcome:com_2");
    }

    #[test]
    fn component_symbol_replace() {
        let (out, n) = replace_symbol("if_opensub(if_549:com_2, ...)", "if_549:com_2", "if_549:close");
        assert_eq!(n, 1);
        assert_eq!(out, "if_opensub(if_549:close, ...)");
    }
}
