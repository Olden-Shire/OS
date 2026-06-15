//! Config record ↔ readable text codec.
//!
//! Each config type (obj, loc, npc, …) is a stream of `opcode + operands`
//! terminated by opcode 0. This module turns that stream into a readable
//! `key = value` text file and back, BYTE-EXACTLY, driven from a per-type
//! opcode SCHEMA rather than the lossy typed decoders: every opcode is
//! emitted as its own line in stream order with its raw operands, so
//! re-encoding the lines in order reproduces the original bytes by
//! construction.
//!
//! `decode` self-verifies (re-encode → compare) and returns `None` on any
//! mismatch or unknown opcode, so the unpack pipeline falls back to
//! `.dat` per-record and nothing is ever lost. Opcodes whose wire layout
//! this model can't express (e.g. the `n+1` multiloc lists) are simply
//! left out of a schema; records that use them fall back to `.dat`.

use std::collections::{BTreeMap, HashMap};

use io::Packet;

/// Resolves a model id ↔ its `model.pack` name for [`Operand::Model`] operands,
/// so config text can reference models by name (`model = obj_cannonball`) instead
/// of a raw id. Built from the model pack file; entries whose name is still the
/// numeric stub (`995=995`) are ignored, so unnamed models render as their id and
/// round-trip unchanged. The default (empty) resolver makes every `Model` operand
/// behave exactly like a raw `U16`.
/// One id↔name table built from a `*.pack` file (numeric stubs filtered out, so
/// unnamed ids render as their number and round-trip unchanged).
#[derive(Default)]
struct PackRefs {
    by_id: HashMap<u32, String>,
    by_name: HashMap<String, u32>,
}

impl PackRefs {
    fn from_pack(map: &BTreeMap<u32, String>) -> Self {
        let mut by_id = HashMap::new();
        let mut by_name = HashMap::new();
        for (&id, name) in map {
            if name.parse::<u32>().is_ok() {
                continue; // bare-id stub, not a real name
            }
            by_id.insert(id, name.clone());
            by_name.insert(name.clone(), id);
        }
        Self { by_id, by_name }
    }
    fn name_of(&self, id: i32) -> Option<&str> {
        u32::try_from(id).ok().and_then(|i| self.by_id.get(&i)).map(String::as_str)
    }
    fn id_of(&self, name: &str) -> Option<i32> {
        self.by_name.get(name).map(|&i| i as i32)
    }
}

/// Resolves config id references to/from `*.pack` names so config text can use
/// names (`model = obj_cannonball`, `readyanim = seq_447`) instead of raw ids:
/// [`Operand::Model`]/[`Operand::ModelList`] go through `model.pack`,
/// [`Operand::Seq`] through `seq.pack`. The default (empty) resolver makes every
/// ref behave like a raw number.
#[derive(Default)]
pub struct ConfigRefs {
    model: PackRefs,
    seq: PackRefs,
}

impl ConfigRefs {
    /// Build from a `model.pack` id→stem map (seqs unnamed). Back-compat for
    /// callers that only resolve models.
    #[must_use]
    pub fn from_pack(model: &BTreeMap<u32, String>) -> Self {
        Self { model: PackRefs::from_pack(model), seq: PackRefs::default() }
    }

    /// Build from both `model.pack` and `seq.pack` id→stem maps.
    #[must_use]
    pub fn from_packs(model: &BTreeMap<u32, String>, seq: &BTreeMap<u32, String>) -> Self {
        Self { model: PackRefs::from_pack(model), seq: PackRefs::from_pack(seq) }
    }
}

/// A scalar wire primitive.
#[derive(Clone, Copy)]
pub enum Prim {
    /// unsigned byte (g1 / p1)
    U8,
    /// unsigned short (g2 / p2)
    U16,
    /// signed short (g2b / p2)
    I16,
    /// signed int (g4 / p4)
    I32,
    /// signed byte (g1b / p1)
    I8,
    /// unsigned 3-byte (g3 / p3) — colours
    U24,
}

impl Prim {
    fn read(self, p: &mut Packet) -> i32 {
        match self {
            Prim::U8 => p.g1(),
            Prim::U16 => p.g2(),
            Prim::I16 => i32::from(p.g2b()),
            Prim::I32 => p.g4(),
            Prim::I8 => i32::from(p.g1b()),
            Prim::U24 => p.g3(),
        }
    }
    fn write(self, p: &mut Packet, v: i32) {
        match self {
            Prim::U8 | Prim::I8 => p.p1(v),
            Prim::U16 | Prim::I16 => p.p2(v),
            Prim::I32 => p.p4(v),
            Prim::U24 => p.p3(v),
        }
    }
}

/// One operand of an opcode, in stream order.
#[derive(Clone, Copy)]
pub enum Operand {
    /// a scalar
    Num(Prim),
    /// a `U16` model id resolved through `model.pack`: rendered as its name when
    /// one exists, else the raw id; parsed back from either form on encode.
    Model,
    /// a `U16` seq (animation) id resolved through `seq.pack`, same as `Model`.
    Seq,
    /// a `U8` count then `count` × `U16` model ids, each resolved through
    /// `model.pack`; rendered space-separated (`models = a b c`).
    ModelList,
    /// length-prefixed cp1252 string (gjstr / pjstr) — must be the LAST operand
    Str,
    /// `count` (a scalar) then `count` rows of `cols`. `col_major` reads all
    /// of column 0, then all of column 1, … (seq frame tables); otherwise
    /// rows are interleaved (recolour pairs, model+shape lists).
    Counted { count: Prim, cols: &'static [Prim], col_major: bool },
    /// Like `Counted` row-major but reads `count + 1` rows (the loc
    /// multiloc / npc multivar lists, whose g1 count is one less than the
    /// element count). Single-column only.
    CountedP1 { count: Prim, col: Prim },
    /// A `U8` count then `count` × `U16` model ids (model.pack resolved),
    /// rendered as INDEXED separate lines `<stem>1 = a`, `<stem>2 = b`, …
    /// (content-old style — `model1`/`head1`). Must be an opcode's sole
    /// operand; on encode the contiguous `<stem>N` run is grouped back.
    ModelsIndexed { stem: &'static str },
    /// A `U8` count then `count` × (`U16` src, `U16` dst) recolour/retexture
    /// pairs, rendered as indexed lines `<stem>1s = src`, `<stem>1d = dst`, …
    /// (content-old `recol1s`/`recol1d`). Must be an opcode's sole operand.
    PairsIndexed { stem: &'static str },
    /// A fixed-arity run of `U16` seq ids (seq.pack resolved), one per label,
    /// rendered as separate lines `<labels[0]> = a`, `<labels[1]> = b`, …
    /// (the npc 4-direction walk: `walkanim`/`walkanim_b`/`walkanim_r`/
    /// `walkanim_l`). Must be an opcode's sole operand; on encode the labels
    /// are matched contiguously in order.
    LabeledSeqs { labels: &'static [&'static str] },
}

/// The npc 4-direction walk-anim labels (op-17), in wire order: forward, back,
/// right, left (Engine-TS `NpcType`). All suffixed (`_f` for forward) so the set
/// is distinct from the single op-14 `walkanim`.
const WALK_DIRS: &[&str] = &["walkanim_f", "walkanim_b", "walkanim_r", "walkanim_l"];

/// Which half of an indexed line a key denotes, for [`encode`] grouping.
#[derive(Clone, Copy, PartialEq)]
enum IndexKind {
    /// a `<stem>N` model line
    Model,
    /// a `<stem>Ns` recolour-source line
    PairS,
    /// a `<stem>Nd` recolour-dest line
    PairD,
}

/// If `key` is an indexed line for some schema opcode (`model3`, `recol2s`),
/// return that opcode, which half it is, and its 1-based index.
fn indexed_match(schema: Schema, key: &str) -> Option<(&'static OpDef, IndexKind, u32)> {
    fn digits(s: &str) -> Option<u32> {
        if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) { s.parse().ok() } else { None }
    }
    for def in schema {
        match def.operands {
            [Operand::ModelsIndexed { stem }] => {
                if let Some(rest) = key.strip_prefix(stem)
                    && let Some(idx) = digits(rest)
                {
                    return Some((def, IndexKind::Model, idx));
                }
            }
            [Operand::PairsIndexed { stem }] => {
                if let Some(rest) = key.strip_prefix(stem) {
                    if let Some(idx) = rest.strip_suffix('s').and_then(digits) {
                        return Some((def, IndexKind::PairS, idx));
                    }
                    if let Some(idx) = rest.strip_suffix('d').and_then(digits) {
                        return Some((def, IndexKind::PairD, idx));
                    }
                }
            }
            _ => {}
        }
    }
    None
}

/// If a `LabeledSeqs` opcode's labels match `entries[at..]` exactly and in
/// order, return that opcode (the whole `walkanim_f`/`walkanim_b`/… set must be
/// present contiguously for it to encode as the 4-direction op).
fn labeled_match_at(schema: Schema, entries: &[(&str, &str)], at: usize) -> Option<&'static OpDef> {
    for def in schema {
        if let [Operand::LabeledSeqs { labels }] = def.operands
            && at + labels.len() <= entries.len()
            && labels.iter().enumerate().all(|(k, &lab)| entries[at + k].0 == lab)
        {
            return Some(def);
        }
    }
    None
}

/// A single opcode's schema: wire code, readable field name, operands.
pub struct OpDef {
    pub code: u8,
    pub name: &'static str,
    pub operands: &'static [Operand],
}

pub type Schema = &'static [OpDef];

/// Server-side npc config keys (Engine-TS `NpcType` opcodes 200+) that the 2007
/// CLIENT cache does not carry. They may appear in a `.npc` SOURCE file (authored
/// content like `wanderrange = 2`), but must NOT be written into the cache — the
/// server reads them separately. `encode` skips any line with one of these keys
/// instead of failing, so the repacked cache stays CRC-identical to vanilla.
pub const NPC_SERVER_KEYS: &[&str] = &[
    // examine text — server-authored from rev1 on (dropped from the client cache
    // config, where it was npc opcode 3 in the 2004 engine).
    "desc",
    "wanderrange",
    "maxrange",
    "huntrange",
    "timer",
    "respawnrate",
    "moverestrict",
    "attackrange",
    "blockwalk",
    "huntmode",
    "defaultmode",
    "members",
    "patrol",
    "givechase",
    "regenrate",
    "params",
    "param",
    "category",
    "debugname",
    // Combat base stats — cache opcodes 74-79 in the 2004 engine, server-side
    // from rev1 on (dropped from the client cache).
    "hitpoints",
    "attack",
    "strength",
    "defence",
    "ranged",
    "magic",
];

/// Is `key` a server-only config property (not a client-cache opcode)?
#[must_use]
pub fn is_server_only_key(key: &str) -> bool {
    NPC_SERVER_KEYS.contains(&key)
}

/// Decode a config record to readable text iff it re-encodes BYTE-EXACTLY
/// (else `None` → caller keeps `.dat`). First line is a `// <kind> <id>`
/// context comment, ignored on encode.
pub fn decode(schema: Schema, kind: &str, id: u32, bytes: &[u8], refs: &ConfigRefs) -> Option<String> {
    let lines = decode_lines(schema, bytes, refs)?;
    let mut text = format!("// {kind} {id}\n");
    text.push_str(&lines.join("\n"));
    if !lines.is_empty() {
        text.push('\n');
    }
    match encode(schema, &text, refs) {
        Some(re) if re == bytes => Some(text),
        _ => None,
    }
}

fn decode_lines(schema: Schema, bytes: &[u8], refs: &ConfigRefs) -> Option<Vec<String>> {
    let mut p = Packet::from_vec(bytes.to_vec());
    let mut lines = Vec::new();
    loop {
        if p.pos as usize >= bytes.len() {
            return None; // no terminating 0
        }
        let code = p.g1() as u8;
        if code == 0 {
            if p.pos as usize != bytes.len() {
                return None; // trailing bytes — we'd lose data
            }
            return Some(lines);
        }
        let def = schema.iter().find(|d| d.code == code)?;

        // Indexed multi-line opcodes (models/heads, recol/retex pairs) expand
        // to one `<stem>N …` line per element instead of a single joined line.
        match def.operands {
            [Operand::ModelsIndexed { stem }] => {
                let n = p.g1() as usize;
                if n == 0 {
                    return None; // empty list can't round-trip from zero lines
                }
                for idx in 1..=n {
                    let id = p.g2();
                    let name = refs.model.name_of(id).map_or_else(|| id.to_string(), str::to_string);
                    lines.push(format!("{stem}{idx} = {name}"));
                }
                continue;
            }
            [Operand::PairsIndexed { stem }] => {
                let n = p.g1() as usize;
                if n == 0 {
                    return None;
                }
                for idx in 1..=n {
                    let s = p.g2();
                    let d = p.g2();
                    lines.push(format!("{stem}{idx}s = {s}"));
                    lines.push(format!("{stem}{idx}d = {d}"));
                }
                continue;
            }
            [Operand::LabeledSeqs { labels }] => {
                for label in *labels {
                    let id = p.g2();
                    let name = refs.seq.name_of(id).map_or_else(|| id.to_string(), str::to_string);
                    lines.push(format!("{label} = {name}"));
                }
                continue;
            }
            _ => {}
        }

        let mut parts: Vec<String> = Vec::new();
        for op in def.operands {
            match op {
                Operand::Num(prim) => parts.push(prim.read(&mut p).to_string()),
                Operand::Model => {
                    let id = p.g2();
                    parts.push(refs.model.name_of(id).map_or_else(|| id.to_string(), str::to_string));
                }
                Operand::Seq => {
                    let id = p.g2();
                    parts.push(refs.seq.name_of(id).map_or_else(|| id.to_string(), str::to_string));
                }
                Operand::ModelList => {
                    let n = p.g1() as usize;
                    let vals: Vec<String> = (0..n)
                        .map(|_| {
                            let id = p.g2();
                            refs.model.name_of(id).map_or_else(|| id.to_string(), str::to_string)
                        })
                        .collect();
                    parts.push(vals.join(" "));
                }
                Operand::Str => parts.push(escape_str(&p.gjstr())),
                Operand::Counted { count, cols, col_major } => {
                    let n = count.read(&mut p) as usize;
                    let mut rows = vec![Vec::with_capacity(cols.len()); n];
                    if *col_major {
                        for &col in *cols {
                            for row in rows.iter_mut() {
                                row.push(col.read(&mut p));
                            }
                        }
                    } else {
                        for row in rows.iter_mut() {
                            for &col in *cols {
                                row.push(col.read(&mut p));
                            }
                        }
                    }
                    let s = rows
                        .iter()
                        .map(|r| r.iter().map(i32::to_string).collect::<Vec<_>>().join("/"))
                        .collect::<Vec<_>>()
                        .join(" ");
                    parts.push(s);
                }
                Operand::CountedP1 { count, col } => {
                    let n = count.read(&mut p) as usize + 1;
                    let vals: Vec<String> =
                        (0..n).map(|_| col.read(&mut p).to_string()).collect();
                    parts.push(vals.join(" "));
                }
                // Indexed / labeled operands are an opcode's sole operand and
                // handled before this loop; reaching here means a bad schema.
                Operand::ModelsIndexed { .. }
                | Operand::PairsIndexed { .. }
                | Operand::LabeledSeqs { .. } => return None,
            }
        }
        // No-operand opcodes are presence flags (members, stackable): the byte
        // stream just carries the opcode, so render `name = true` rather than a
        // bare `name = `.
        if def.operands.is_empty() {
            lines.push(format!("{} = true", def.name));
        } else {
            lines.push(format!("{} = {}", def.name, parts.join(", ")));
        }
    }
}

/// Re-encode readable text to the exact config byte stream. Lines run in
/// order (opcode order preserved); blank/`//` lines skipped. `None` on any
/// unparseable line.
pub fn encode(schema: Schema, text: &str, refs: &ConfigRefs) -> Option<Vec<u8>> {
    let mut p = Packet::from_vec(Vec::new());
    // Significant `key = value` lines in order (comments/blank dropped); a
    // line without `=` is malformed and fails the whole round-trip.
    let mut entries: Vec<(&str, &str)> = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        let (k, v) = line.split_once('=')?;
        entries.push((k.trim(), v.trim()));
    }

    let mut i = 0;
    while i < entries.len() {
        let (key, val) = entries[i];

        // Indexed multi-line opcodes (model1.., recol1s/recol1d..): gather the
        // contiguous run of same-opcode lines and emit a single opcode.
        if let Some((def, _, _)) = indexed_match(schema, key) {
            let mut models: Vec<(u32, &str)> = Vec::new();
            let mut src: BTreeMap<u32, i32> = BTreeMap::new();
            let mut dst: BTreeMap<u32, i32> = BTreeMap::new();
            while i < entries.len() {
                let Some((d2, kind, idx)) = indexed_match(schema, entries[i].0) else { break };
                if d2.code != def.code {
                    break;
                }
                match kind {
                    IndexKind::Model => models.push((idx, entries[i].1)),
                    IndexKind::PairS => {
                        src.insert(idx, entries[i].1.parse().ok()?);
                    }
                    IndexKind::PairD => {
                        dst.insert(idx, entries[i].1.parse().ok()?);
                    }
                }
                i += 1;
            }
            p.p1(i32::from(def.code));
            match def.operands {
                [Operand::ModelsIndexed { .. }] => {
                    models.sort_by_key(|(idx, _)| *idx);
                    p.p1(models.len() as i32);
                    for (_, name) in models {
                        let id = match name.parse::<i32>() {
                            Ok(n) => n,
                            Err(_) => refs.model.id_of(name)?,
                        };
                        p.p2(id);
                    }
                }
                [Operand::PairsIndexed { .. }] => {
                    if src.len() != dst.len() {
                        return None; // every index needs both a src and a dst
                    }
                    p.p1(src.len() as i32);
                    for (idx, s) in &src {
                        p.p2(*s);
                        p.p2(*dst.get(idx)?);
                    }
                }
                _ => return None,
            }
            continue;
        }

        // Labeled seq group (npc 4-direction `walkanim_f`/`walkanim_b`/…),
        // emitted only when the whole set is present contiguously.
        if let Some(def) = labeled_match_at(schema, &entries, i) {
            let [Operand::LabeledSeqs { labels }] = def.operands else { return None };
            p.p1(i32::from(def.code));
            for k in 0..labels.len() {
                let f = entries[i + k].1;
                let id = match f.parse::<i32>() {
                    Ok(n) => n,
                    Err(_) => refs.seq.id_of(f)?,
                };
                p.p2(id);
            }
            i += labels.len();
            continue;
        }

        // Normal single-line opcode.
        let Some(def) = schema.iter().find(|d| d.name == key) else {
            // Server-side property (e.g. wanderrange): kept in the .npc source,
            // not emitted into the client cache. Unknown non-server keys are a
            // real error (typo / unsupported opcode) → fail the round-trip.
            if is_server_only_key(key) {
                i += 1;
                continue;
            }
            return None;
        };
        p.p1(i32::from(def.code));

        // A trailing Str operand consumes the remainder of the line, so we
        // split the value into (operand_count) comma fields where the last
        // field (if Str) is whatever's left after the prior commas.
        let has_trailing_str = matches!(def.operands.last(), Some(Operand::Str));
        let nfields = def.operands.len();
        let fields: Vec<&str> = split_fields(val, nfields, has_trailing_str);
        // No-operand flags carry a decorative value (`members = true`, or the
        // legacy bare `members = `) we ignore — only the opcode is emitted.
        // Everything else must supply exactly its operand count.
        if nfields != 0 && fields.len() != nfields {
            return None;
        }

        for (op, field) in def.operands.iter().zip(fields.iter()) {
            match op {
                Operand::Num(prim) => prim.write(&mut p, field.trim().parse().ok()?),
                Operand::Model => {
                    let f = field.trim();
                    let id = match f.parse::<i32>() {
                        Ok(n) => n,
                        Err(_) => refs.model.id_of(f)?,
                    };
                    p.p2(id);
                }
                Operand::Seq => {
                    let f = field.trim();
                    let id = match f.parse::<i32>() {
                        Ok(n) => n,
                        Err(_) => refs.seq.id_of(f)?,
                    };
                    p.p2(id);
                }
                Operand::ModelList => {
                    let f = field.trim();
                    let ids: Vec<i32> = if f.is_empty() {
                        Vec::new()
                    } else {
                        f.split_whitespace()
                            .map(|t| match t.parse::<i32>() {
                                Ok(n) => Some(n),
                                Err(_) => refs.model.id_of(t),
                            })
                            .collect::<Option<Vec<_>>>()?
                    };
                    p.p1(ids.len() as i32);
                    for id in ids {
                        p.p2(id);
                    }
                }
                Operand::Str => p.pjstr(&unescape_str(field)),
                Operand::Counted { count, cols, col_major } => {
                    let f = field.trim();
                    let rows: Vec<Vec<i32>> = if f.is_empty() {
                        Vec::new()
                    } else {
                        f.split_whitespace()
                            .map(|row| {
                                row.split('/').map(|c| c.parse::<i32>()).collect::<Result<Vec<_>, _>>()
                            })
                            .collect::<Result<Vec<_>, _>>()
                            .ok()?
                    };
                    // Every row must have exactly `cols.len()` columns.
                    if rows.iter().any(|r| r.len() != cols.len()) {
                        return None;
                    }
                    count.write(&mut p, rows.len() as i32);
                    if *col_major {
                        for ci in 0..cols.len() {
                            for row in &rows {
                                cols[ci].write(&mut p, row[ci]);
                            }
                        }
                    } else {
                        for row in &rows {
                            for (ci, &c) in row.iter().enumerate() {
                                cols[ci].write(&mut p, c);
                            }
                        }
                    }
                }
                Operand::CountedP1 { count, col } => {
                    let f = field.trim();
                    let vals: Vec<i32> = if f.is_empty() {
                        Vec::new()
                    } else {
                        f.split_whitespace()
                            .map(|v| v.parse::<i32>())
                            .collect::<Result<Vec<_>, _>>()
                            .ok()?
                    };
                    if vals.is_empty() {
                        return None; // n+1 ≥ 1 always
                    }
                    count.write(&mut p, vals.len() as i32 - 1);
                    for v in vals {
                        col.write(&mut p, v);
                    }
                }
                // Sole-operand indexed/labeled opcodes are emitted by the
                // grouping branches above; never reached through this loop.
                Operand::ModelsIndexed { .. }
                | Operand::PairsIndexed { .. }
                | Operand::LabeledSeqs { .. } => return None,
            }
        }
        i += 1;
    }
    p.p1(0);
    Some(p.data)
}

/// Split `val` into `n` comma-separated fields. When `trailing_str`, the
/// last field keeps every remaining comma (strings may contain commas).
fn split_fields(val: &str, n: usize, trailing_str: bool) -> Vec<&str> {
    if n == 0 {
        return Vec::new();
    }
    if trailing_str && n >= 1 {
        // split into at most n parts: the first n-1 on commas, rest verbatim.
        let mut out: Vec<&str> = val.splitn(n, ',').collect();
        // splitn keeps the tail intact in the final element.
        for f in out.iter_mut().take(n - 1) {
            *f = f.trim();
        }
        return out;
    }
    val.split(',').map(str::trim).collect()
}

fn escape_str(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\n', "\\n").replace('\r', "\\r")
}

fn unescape_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

// ── schema building blocks ──────────────────────────────────────────────
use Operand::{Counted, CountedP1, Num, Str};
use Prim::{I16, I32, I8, U16, U24, U8};

const fn n(p: Prim) -> Operand {
    Num(p)
}
/// recolour / retexture: g1 count, count × (g2 src, g2 dst).
const PAIRS: Operand = Counted { count: U8, cols: &[U16, U16], col_major: false };
/// g1 count, count × g2 (model / head lists).
const U16_LIST: Operand = Counted { count: U8, cols: &[U16], col_major: false };

// ── obj (group 10) ──────────────────────────────────────────────────────
pub const OBJ: Schema = &[
    OpDef { code: 1, name: "model", operands: &[Operand::Model] },
    OpDef { code: 2, name: "name", operands: &[Str] },
    OpDef { code: 4, name: "zoom2d", operands: &[n(U16)] },
    OpDef { code: 5, name: "xan2d", operands: &[n(U16)] },
    OpDef { code: 6, name: "yan2d", operands: &[n(U16)] },
    OpDef { code: 7, name: "xof2d", operands: &[n(I16)] },
    OpDef { code: 8, name: "yof2d", operands: &[n(I16)] },
    OpDef { code: 11, name: "stackable", operands: &[] },
    OpDef { code: 12, name: "cost", operands: &[n(I32)] },
    OpDef { code: 16, name: "members", operands: &[] },
    OpDef { code: 23, name: "manwear", operands: &[Operand::Model, n(U8)] },
    OpDef { code: 24, name: "manwear2", operands: &[n(U16)] },
    OpDef { code: 25, name: "womanwear", operands: &[Operand::Model, n(U8)] },
    OpDef { code: 26, name: "womanwear2", operands: &[n(U16)] },
    OpDef { code: 30, name: "op1", operands: &[Str] },
    OpDef { code: 31, name: "op2", operands: &[Str] },
    OpDef { code: 32, name: "op3", operands: &[Str] },
    OpDef { code: 33, name: "op4", operands: &[Str] },
    OpDef { code: 34, name: "op5", operands: &[Str] },
    OpDef { code: 35, name: "iop1", operands: &[Str] },
    OpDef { code: 36, name: "iop2", operands: &[Str] },
    OpDef { code: 37, name: "iop3", operands: &[Str] },
    OpDef { code: 38, name: "iop4", operands: &[Str] },
    OpDef { code: 39, name: "iop5", operands: &[Str] },
    OpDef { code: 40, name: "recol", operands: &[Operand::PairsIndexed { stem: "recol" }] },
    OpDef { code: 41, name: "retex", operands: &[Operand::PairsIndexed { stem: "retex" }] },
    OpDef { code: 78, name: "manwear3", operands: &[n(U16)] },
    OpDef { code: 79, name: "womanwear3", operands: &[n(U16)] },
    OpDef { code: 90, name: "manhead", operands: &[n(U16)] },
    OpDef { code: 91, name: "womanhead", operands: &[n(U16)] },
    OpDef { code: 92, name: "manhead2", operands: &[n(U16)] },
    OpDef { code: 93, name: "womanhead2", operands: &[n(U16)] },
    OpDef { code: 95, name: "zan2d", operands: &[n(U16)] },
    OpDef { code: 97, name: "certlink", operands: &[n(U16)] },
    OpDef { code: 98, name: "certtemplate", operands: &[n(U16)] },
    OpDef { code: 100, name: "count1", operands: &[n(U16), n(U16)] },
    OpDef { code: 101, name: "count2", operands: &[n(U16), n(U16)] },
    OpDef { code: 102, name: "count3", operands: &[n(U16), n(U16)] },
    OpDef { code: 103, name: "count4", operands: &[n(U16), n(U16)] },
    OpDef { code: 104, name: "count5", operands: &[n(U16), n(U16)] },
    OpDef { code: 105, name: "count6", operands: &[n(U16), n(U16)] },
    OpDef { code: 106, name: "count7", operands: &[n(U16), n(U16)] },
    OpDef { code: 107, name: "count8", operands: &[n(U16), n(U16)] },
    OpDef { code: 108, name: "count9", operands: &[n(U16), n(U16)] },
    OpDef { code: 109, name: "count10", operands: &[n(U16), n(U16)] },
    OpDef { code: 110, name: "resizex", operands: &[n(U16)] },
    OpDef { code: 111, name: "resizey", operands: &[n(U16)] },
    OpDef { code: 112, name: "resizez", operands: &[n(U16)] },
    OpDef { code: 113, name: "ambient", operands: &[n(I8)] },
    OpDef { code: 114, name: "contrast", operands: &[n(I8)] },
    OpDef { code: 115, name: "team", operands: &[n(U8)] },
];

// ── loc (group 6) ─ opcodes 77/79 (multiloc/bgsound n+1 lists) omitted ──
pub const LOC: Schema = &[
    OpDef { code: 1, name: "models", operands: &[Counted { count: U8, cols: &[U16, U8], col_major: false }] },
    OpDef { code: 2, name: "name", operands: &[Str] },
    OpDef { code: 5, name: "models_only", operands: &[U16_LIST] },
    OpDef { code: 14, name: "width", operands: &[n(U8)] },
    OpDef { code: 15, name: "length", operands: &[n(U8)] },
    OpDef { code: 17, name: "nonsolid", operands: &[] },
    OpDef { code: 18, name: "nonblockrange", operands: &[] },
    OpDef { code: 19, name: "active", operands: &[n(U8)] },
    OpDef { code: 21, name: "hillskew", operands: &[] },
    OpDef { code: 22, name: "sharelight", operands: &[] },
    OpDef { code: 23, name: "occlude", operands: &[] },
    OpDef { code: 24, name: "anim", operands: &[n(U16)] },
    OpDef { code: 27, name: "blockwalk", operands: &[] },
    OpDef { code: 28, name: "wallwidth", operands: &[n(U8)] },
    OpDef { code: 29, name: "ambient", operands: &[n(I8)] },
    OpDef { code: 30, name: "op1", operands: &[Str] },
    OpDef { code: 31, name: "op2", operands: &[Str] },
    OpDef { code: 32, name: "op3", operands: &[Str] },
    OpDef { code: 33, name: "op4", operands: &[Str] },
    OpDef { code: 34, name: "op5", operands: &[Str] },
    OpDef { code: 39, name: "contrast", operands: &[n(I8)] },
    OpDef { code: 40, name: "recol", operands: &[PAIRS] },
    OpDef { code: 41, name: "retex", operands: &[PAIRS] },
    OpDef { code: 60, name: "mapfunction", operands: &[n(U16)] },
    OpDef { code: 62, name: "mirror", operands: &[] },
    OpDef { code: 64, name: "noshadow", operands: &[] },
    OpDef { code: 65, name: "resizex", operands: &[n(U16)] },
    OpDef { code: 66, name: "resizey", operands: &[n(U16)] },
    OpDef { code: 67, name: "resizez", operands: &[n(U16)] },
    OpDef { code: 68, name: "mapscene", operands: &[n(U16)] },
    OpDef { code: 69, name: "forceapproach", operands: &[n(U8)] },
    OpDef { code: 70, name: "offsetx", operands: &[n(I16)] },
    OpDef { code: 71, name: "offsety", operands: &[n(I16)] },
    OpDef { code: 72, name: "offsetz", operands: &[n(I16)] },
    OpDef { code: 73, name: "forcedecor", operands: &[] },
    OpDef { code: 74, name: "breakroutefinding", operands: &[] },
    OpDef { code: 75, name: "raiseobject", operands: &[n(U8)] },
    OpDef { code: 77, name: "multiloc", operands: &[n(U16), n(U16), CountedP1 { count: U8, col: U16 }] },
    OpDef { code: 78, name: "bgsound", operands: &[n(U16), n(U8)] },
    OpDef { code: 79, name: "bgsound_random", operands: &[n(U16), n(U16), n(U8), U16_LIST] },
];

// ── npc (group 9) ─ opcode 106 (multivar n+1 list) omitted ──────────────
pub const NPC: Schema = &[
    // models / heads / recol / retex use content-old's indexed-line form
    // (`model1`, `head1`, `recol1s`/`recol1d`); op17 is the 4-direction walk
    // broken into `walkanim_f`/`walkanim_b`/`walkanim_r`/`walkanim_l` — all
    // suffixed, so the set is distinct from the single op14 `walkanim`.
    OpDef { code: 1, name: "models", operands: &[Operand::ModelsIndexed { stem: "model" }] },
    OpDef { code: 2, name: "name", operands: &[Str] },
    OpDef { code: 12, name: "size", operands: &[n(U8)] },
    OpDef { code: 13, name: "readyanim", operands: &[Operand::Seq] },
    OpDef { code: 14, name: "walkanim", operands: &[Operand::Seq] },
    OpDef { code: 15, name: "turnleftanim", operands: &[Operand::Seq] },
    OpDef { code: 16, name: "turnrightanim", operands: &[Operand::Seq] },
    OpDef { code: 17, name: "walkanim_set", operands: &[Operand::LabeledSeqs { labels: WALK_DIRS }] },
    OpDef { code: 30, name: "op1", operands: &[Str] },
    OpDef { code: 31, name: "op2", operands: &[Str] },
    OpDef { code: 32, name: "op3", operands: &[Str] },
    OpDef { code: 33, name: "op4", operands: &[Str] },
    OpDef { code: 34, name: "op5", operands: &[Str] },
    OpDef { code: 40, name: "recol", operands: &[Operand::PairsIndexed { stem: "recol" }] },
    OpDef { code: 41, name: "retex", operands: &[Operand::PairsIndexed { stem: "retex" }] },
    OpDef { code: 60, name: "headmodels", operands: &[Operand::ModelsIndexed { stem: "head" }] },
    OpDef { code: 93, name: "nominimap", operands: &[] },
    OpDef { code: 95, name: "vislevel", operands: &[n(U16)] },
    OpDef { code: 97, name: "resizeh", operands: &[n(U16)] },
    OpDef { code: 98, name: "resizev", operands: &[n(U16)] },
    OpDef { code: 99, name: "alwaysontop", operands: &[] },
    OpDef { code: 100, name: "ambient", operands: &[n(I8)] },
    OpDef { code: 101, name: "contrast", operands: &[n(I8)] },
    OpDef { code: 102, name: "headicon", operands: &[n(U16)] },
    OpDef { code: 103, name: "turnspeed", operands: &[n(U16)] },
    OpDef { code: 106, name: "multivar", operands: &[n(U16), n(U16), CountedP1 { count: U8, col: U16 }] },
    OpDef { code: 107, name: "inactive", operands: &[] },
    OpDef { code: 109, name: "nowalksmoothing", operands: &[] },
];

// ── seq (group 12) ──────────────────────────────────────────────────────
pub const SEQ: Schema = &[
    OpDef { code: 1, name: "frames", operands: &[Counted { count: U16, cols: &[U16, U16, U16], col_major: true }] },
    OpDef { code: 2, name: "loops", operands: &[n(U16)] },
    OpDef { code: 3, name: "walkmerge", operands: &[Counted { count: U8, cols: &[U8], col_major: false }] },
    OpDef { code: 4, name: "reachforward", operands: &[] },
    OpDef { code: 5, name: "priority", operands: &[n(U8)] },
    OpDef { code: 6, name: "replaceheldleft", operands: &[n(U16)] },
    OpDef { code: 7, name: "replaceheldright", operands: &[n(U16)] },
    OpDef { code: 8, name: "maxloops", operands: &[n(U8)] },
    OpDef { code: 9, name: "preanim_move", operands: &[n(U8)] },
    OpDef { code: 10, name: "postanim_move", operands: &[n(U8)] },
    OpDef { code: 11, name: "duplicatebehaviour", operands: &[n(U8)] },
    OpDef { code: 12, name: "iframes", operands: &[Counted { count: U8, cols: &[U16, U16], col_major: true }] },
    OpDef { code: 13, name: "sound", operands: &[Counted { count: U8, cols: &[U24], col_major: false }] },
];

// ── flo (group 4) ───────────────────────────────────────────────────────
pub const FLO: Schema = &[
    OpDef { code: 1, name: "colour", operands: &[n(U24)] },
    OpDef { code: 2, name: "texture", operands: &[n(U8)] },
    OpDef { code: 5, name: "noocclude", operands: &[] },
    OpDef { code: 7, name: "mapcolour", operands: &[n(U24)] },
];

// ── flu (group 1) ───────────────────────────────────────────────────────
pub const FLU: Schema = &[OpDef { code: 1, name: "colour", operands: &[n(U24)] }];

// ── idk (group 3) ───────────────────────────────────────────────────────
pub const IDK: Schema = &[
    OpDef { code: 1, name: "type", operands: &[n(U8)] },
    OpDef { code: 2, name: "models", operands: &[U16_LIST] },
    OpDef { code: 3, name: "disable", operands: &[] },
    OpDef { code: 40, name: "recol", operands: &[PAIRS] },
    OpDef { code: 41, name: "retex", operands: &[PAIRS] },
    OpDef { code: 60, name: "head1", operands: &[n(U16)] },
    OpDef { code: 61, name: "head2", operands: &[n(U16)] },
    OpDef { code: 62, name: "head3", operands: &[n(U16)] },
    OpDef { code: 63, name: "head4", operands: &[n(U16)] },
    OpDef { code: 64, name: "head5", operands: &[n(U16)] },
    OpDef { code: 65, name: "head6", operands: &[n(U16)] },
    OpDef { code: 66, name: "head7", operands: &[n(U16)] },
    OpDef { code: 67, name: "head8", operands: &[n(U16)] },
    OpDef { code: 68, name: "head9", operands: &[n(U16)] },
    OpDef { code: 69, name: "head10", operands: &[n(U16)] },
];

// ── inv (group 5) ───────────────────────────────────────────────────────
pub const INV: Schema = &[OpDef { code: 2, name: "size", operands: &[n(U16)] }];

// ── spot (group 13) ─────────────────────────────────────────────────────
pub const SPOT: Schema = &[
    OpDef { code: 1, name: "model", operands: &[Operand::Model] },
    OpDef { code: 2, name: "anim", operands: &[n(U16)] },
    OpDef { code: 4, name: "resizeh", operands: &[n(U16)] },
    OpDef { code: 5, name: "resizev", operands: &[n(U16)] },
    OpDef { code: 6, name: "angle", operands: &[n(U16)] },
    OpDef { code: 7, name: "ambient", operands: &[n(U8)] },
    OpDef { code: 8, name: "contrast", operands: &[n(U8)] },
    OpDef { code: 40, name: "recol", operands: &[Operand::PairsIndexed { stem: "recol" }] },
    OpDef { code: 41, name: "retex", operands: &[Operand::PairsIndexed { stem: "retex" }] },
];

// ── varbit (group 14) ───────────────────────────────────────────────────
pub const VARBIT: Schema = &[OpDef { code: 1, name: "bits", operands: &[n(U16), n(U8), n(U8)] }];

// ── varp (group 16) ─────────────────────────────────────────────────────
pub const VARP: Schema = &[OpDef { code: 5, name: "clientcode", operands: &[n(U16)] }];

// ── enum (group 8) ─ op5 (string-valued map) omitted; int maps convert ──
pub const ENUM: Schema = &[
    OpDef { code: 1, name: "inputtype", operands: &[n(U8)] },
    OpDef { code: 2, name: "outputtype", operands: &[n(U8)] },
    OpDef { code: 3, name: "default_string", operands: &[Str] },
    OpDef { code: 4, name: "default_int", operands: &[n(I32)] },
    OpDef { code: 6, name: "intmap", operands: &[Counted { count: U16, cols: &[I32, I32], col_major: false }] },
];

/// Schema + on-disk extension for a config archive group id.
pub fn schema_for_group(group_id: u32) -> Option<(Schema, &'static str)> {
    use crate::config::group;
    Some(match group_id {
        group::OBJ => (OBJ, "obj"),
        group::LOC => (LOC, "loc"),
        group::NPC => (NPC, "npc"),
        group::SEQ => (SEQ, "seq"),
        group::FLO => (FLO, "flo"),
        group::FLU => (FLU, "flu"),
        group::IDK => (IDK, "idk"),
        group::INV => (INV, "inv"),
        group::SPOT => (SPOT, "spot"),
        group::VARBIT => (VARBIT, "varbit"),
        group::VARP => (VARP, "varp"),
        group::ENUM => (ENUM, "enum"),
        _ => return None,
    })
}
