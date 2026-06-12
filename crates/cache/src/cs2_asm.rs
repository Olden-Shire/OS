//! Byte-exact CS2 assembly text format (`.cs2`) — disassemble a decoded
//! [`ClientScript`] to editable text and assemble it back, reproducing the original
//! bytecode bit-for-bit.
//!
//! Unlike [`crate::cs2_pretty`] (a lossy *viewer* that folds pushes/branches), this is a
//! faithful, round-trippable encoding: one text line per bytecode instruction, branch
//! targets expressed as labels, and operands rendered **symbolically** where a name table
//! is supplied (script ids via `script.pack`, varp ids via `varp.pack`, varbit ids via
//! `varbit.pack`). Names are bijective with ids, so `assemble(disassemble(s)) == s` and
//! hence `ClientScript::encode` reproduces the original group bytes. Anything without a
//! name renders numerically and still round-trips.
//!
//! ## Format
//!
//! ```text
//! .name "optional"          ; only emitted when the script carries an internal name
//! .int_args 1
//! .str_args 0
//! .int_locals 3
//! .str_locals 0
//!
//!   push_constant_int 42
//!   push_varp world_var      ; varp.pack name (or the bare id when unnamed)
//! label_00:
//!   push_int_local 0
//!   branch_equals label_01   ; branch operands are labels, never raw offsets
//!   gosub login_handler      ; script.pack name
//!   return
//! ```
//!
//! Lines beginning with `;` or `//` are comments. Branch targets outside the script
//! (rare/malformed) fall back to a raw relative offset written as `*N`.

use std::collections::{BTreeMap, HashMap};

use crate::cs2::{ClientScript, OP_PUSH_CONST_STRING};
use crate::cs2_opcodes::{
    is_branch, opcode_by_name, opcode_name, operand_kind, OperandKind,
};

/// Symbol scopes whose operands are rendered/parsed by name. Each maps 1:1 to the
/// `.pack` file that governs that identifier's id↔name binding.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Scope {
    Script,
    Varp,
    Varbit,
}

/// Which name scope (if any) an opcode's operand belongs to. Limited to **direct**
/// operands that are bijective with a name table — config ids pushed as plain
/// `push_constant_int` and only contextually typed (oc_*/npc/loc) stay numeric.
fn scope_for_op(op: u16) -> Option<Scope> {
    match op {
        40 => Some(Scope::Script),       // gosub_with_params
        1 | 2 => Some(Scope::Varp),       // push_varp / pop_varp
        25 | 27 => Some(Scope::Varbit),   // push_varbit / pop_varbit
        _ => None,
    }
}

/// Bidirectional id↔name table for one scope.
#[derive(Default)]
struct Bimap {
    fwd: HashMap<i32, String>,
    rev: HashMap<String, i32>,
}

impl Bimap {
    fn fill(&mut self, m: &BTreeMap<u32, String>) {
        for (&id, name) in m {
            self.fwd.insert(id as i32, name.clone());
            self.rev.insert(name.clone(), id as i32);
        }
    }
}

/// Name tables consulted when rendering/parsing symbolic operands. Build from the same
/// `.pack` files on both the unpack (disassemble) and pack (assemble) sides so the two
/// directions are exact inverses. Default (empty) renders everything numerically.
#[derive(Default)]
pub struct NameMaps {
    script: Bimap,
    varp: Bimap,
    varbit: Bimap,
}

impl NameMaps {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Populate the script (archive-12) id→name table from a `script.pack`-shaped map.
    pub fn set_scripts(&mut self, m: &BTreeMap<u32, String>) {
        self.script.fill(m);
    }

    /// Populate the varp id→name table from a `varp.pack`-shaped map.
    pub fn set_varps(&mut self, m: &BTreeMap<u32, String>) {
        self.varp.fill(m);
    }

    /// Populate the varbit id→name table from a `varbit.pack`-shaped map.
    pub fn set_varbits(&mut self, m: &BTreeMap<u32, String>) {
        self.varbit.fill(m);
    }

    fn bimap(&self, scope: Scope) -> &Bimap {
        match scope {
            Scope::Script => &self.script,
            Scope::Varp => &self.varp,
            Scope::Varbit => &self.varbit,
        }
    }

    fn id_to_name(&self, scope: Scope, id: i32) -> Option<&str> {
        self.bimap(scope).fwd.get(&id).map(String::as_str)
    }

    fn name_to_id(&self, scope: Scope, name: &str) -> Option<i32> {
        self.bimap(scope).rev.get(name).copied()
    }

    /// Friendly name for a script id (`script.pack`), used by both the asm and the
    /// structured-source renderers.
    #[must_use]
    pub fn script_name(&self, id: i32) -> Option<&str> {
        self.id_to_name(Scope::Script, id)
    }

    /// Reverse of [`Self::script_name`].
    #[must_use]
    pub fn script_id(&self, name: &str) -> Option<i32> {
        self.name_to_id(Scope::Script, name)
    }
}

// ───── disassemble ─────────────────────────────────────────────────────────────

/// Render a decoded script to `.cs2` assembly text. Pass an empty [`NameMaps`] for a
/// purely numeric listing.
#[must_use]
pub fn disassemble(script: &ClientScript, names: &NameMaps) -> String {
    let n = script.instructions.len();
    let labels = compute_labels(script);

    let mut out = String::new();
    if let Some(name) = &script.name {
        out.push_str(&format!(".name {}\n", quote(name)));
    }
    out.push_str(&format!(".int_args {}\n", script.int_arg_count));
    out.push_str(&format!(".str_args {}\n", script.string_arg_count));
    out.push_str(&format!(".int_locals {}\n", script.int_local_count));
    out.push_str(&format!(".str_locals {}\n", script.string_local_count));
    out.push('\n');

    for pc in 0..n {
        if let Some(&k) = labels.get(&pc) {
            out.push_str(&format!("label_{k:02}:\n"));
        }
        let op = script.instructions[pc];
        let mnemonic =
            opcode_name(op).map_or_else(|| format!("op_{op}"), str::to_owned);
        let operand = render_operand(script, &labels, pc, op, names);
        if operand.is_empty() {
            out.push_str(&format!("  {mnemonic}\n"));
        } else {
            out.push_str(&format!("  {mnemonic} {operand}\n"));
        }
    }
    // A branch may target one-past-the-end; emit that label so assemble can resolve it.
    if let Some(&k) = labels.get(&n) {
        out.push_str(&format!("label_{k:02}:\n"));
    }
    out
}

/// Branch targets → ascending label numbers. Includes a target of exactly `n`
/// (one-past-end) so end-of-script jumps are representable.
fn compute_labels(script: &ClientScript) -> BTreeMap<usize, u32> {
    let n = script.instructions.len();
    let mut targets: BTreeMap<usize, u32> = BTreeMap::new();
    for (pc, &op) in script.instructions.iter().enumerate() {
        if !is_branch(op) {
            continue;
        }
        let target = pc as i64 + 1 + script.int_operands[pc] as i64;
        if (0..=n as i64).contains(&target) {
            targets.insert(target as usize, 0);
        }
    }
    for (i, slot) in targets.values_mut().enumerate() {
        *slot = i as u32;
    }
    targets
}

fn render_operand(
    script: &ClientScript,
    labels: &BTreeMap<usize, u32>,
    pc: usize,
    op: u16,
    names: &NameMaps,
) -> String {
    if op == OP_PUSH_CONST_STRING {
        return quote(&script.string_operands[pc]);
    }
    let value = script.int_operands[pc];

    if is_branch(op) {
        let target = pc as i64 + 1 + value as i64;
        return labels
            .get(&(target as usize))
            .filter(|_| target >= 0)
            .map_or_else(|| format!("*{value}"), |k| format!("label_{k:02}"));
    }

    if let Some(scope) = scope_for_op(op) {
        if let Some(name) = names.id_to_name(scope, value) {
            return name.to_owned();
        }
        return value.to_string();
    }

    // Filler / secondary-flag operands are omitted when zero to keep the listing clean;
    // everything else is a literal int.
    match operand_kind(op) {
        OperandKind::Filler | OperandKind::SecondaryFlag if value == 0 => String::new(),
        _ => value.to_string(),
    }
}

// ───── assemble ────────────────────────────────────────────────────────────────

type AsmResult<T> = Result<T, String>;

/// Parse `.cs2` assembly text back into a [`ClientScript`]. Errors (unknown mnemonic,
/// unresolved label/name, malformed operand) are returned as messages.
pub fn assemble(text: &str, names: &NameMaps) -> AsmResult<ClientScript> {
    // Header defaults.
    let mut name: Option<String> = None;
    let mut int_args = 0u16;
    let mut str_args = 0u16;
    let mut int_locals = 0u16;
    let mut str_locals = 0u16;

    // First pass: collect instructions (op + raw operand text) and label→index map.
    struct Raw {
        op: u16,
        operand: String,
    }
    let mut raw: Vec<Raw> = Vec::new();
    let mut label_index: HashMap<String, usize> = HashMap::new();
    let mut pending_labels: Vec<String> = Vec::new();

    for (lineno, raw_line) in text.lines().enumerate() {
        let line = strip_comment(raw_line).trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix('.') {
            let (key, val) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
            let val = val.trim();
            match key {
                "name" => name = Some(unquote(val).map_err(|e| at(lineno, &e))?),
                "int_args" => int_args = parse_u16(val).map_err(|e| at(lineno, &e))?,
                "str_args" => str_args = parse_u16(val).map_err(|e| at(lineno, &e))?,
                "int_locals" => int_locals = parse_u16(val).map_err(|e| at(lineno, &e))?,
                "str_locals" => str_locals = parse_u16(val).map_err(|e| at(lineno, &e))?,
                other => return Err(at(lineno, &format!("unknown directive .{other}"))),
            }
            continue;
        }
        if let Some(label) = line.strip_suffix(':') {
            pending_labels.push(label.trim().to_owned());
            continue;
        }
        // Instruction: mnemonic + optional operand (operand keeps internal spaces, e.g.
        // a quoted string).
        let (mnemonic, operand) = match line.split_once(char::is_whitespace) {
            Some((m, rest)) => (m, rest.trim().to_owned()),
            None => (line, String::new()),
        };
        let op = resolve_mnemonic(mnemonic).ok_or_else(|| at(lineno, &format!("unknown mnemonic {mnemonic}")))?;
        for label in pending_labels.drain(..) {
            label_index.insert(label, raw.len());
        }
        raw.push(Raw { op, operand });
    }
    // Labels declared after the final instruction resolve to one-past-end.
    for label in pending_labels.drain(..) {
        label_index.insert(label, raw.len());
    }

    // Second pass: resolve operands to concrete int/string values.
    let n = raw.len();
    let mut instructions = Vec::with_capacity(n);
    let mut int_operands = vec![0i32; n];
    let mut string_operands = vec![String::new(); n];

    for (i, r) in raw.iter().enumerate() {
        let op = r.op;
        instructions.push(op);
        if op == OP_PUSH_CONST_STRING {
            string_operands[i] = unquote(&r.operand).map_err(|e| format!("instruction {i}: {e}"))?;
            continue;
        }
        if is_branch(op) {
            int_operands[i] = resolve_branch(&r.operand, i, &label_index)
                .map_err(|e| format!("instruction {i}: {e}"))?;
            continue;
        }
        if let Some(scope) = scope_for_op(op) {
            int_operands[i] = resolve_named(scope, &r.operand, names)
                .map_err(|e| format!("instruction {i}: {e}"))?;
            continue;
        }
        // Plain int operand; absent operand means 0 (the omitted filler case).
        int_operands[i] = if r.operand.is_empty() {
            0
        } else {
            parse_i32(&r.operand).map_err(|e| format!("instruction {i}: {e}"))?
        };
    }

    Ok(ClientScript {
        name,
        instructions,
        int_operands,
        string_operands,
        int_local_count: int_locals,
        string_local_count: str_locals,
        int_arg_count: int_args,
        string_arg_count: str_args,
    })
}

fn resolve_mnemonic(mnemonic: &str) -> Option<u16> {
    if let Some(rest) = mnemonic.strip_prefix("op_") {
        return rest.parse().ok();
    }
    opcode_by_name(mnemonic)
}

fn resolve_branch(operand: &str, index: usize, labels: &HashMap<String, usize>) -> AsmResult<i32> {
    if let Some(raw) = operand.strip_prefix('*') {
        return parse_i32(raw);
    }
    let target = *labels
        .get(operand)
        .ok_or_else(|| format!("unknown branch label {operand}"))?;
    // Inverse of disassemble: original operand = target - (index + 1).
    Ok(target as i32 - index as i32 - 1)
}

fn resolve_named(scope: Scope, operand: &str, names: &NameMaps) -> AsmResult<i32> {
    if operand.is_empty() {
        return Err("missing operand for named opcode".to_owned());
    }
    // A bare integer is always accepted; otherwise resolve through the name table.
    if let Ok(v) = operand.parse::<i32>() {
        return Ok(v);
    }
    names
        .name_to_id(scope, operand)
        .ok_or_else(|| format!("unresolved name {operand}"))
}

// ───── small helpers ───────────────────────────────────────────────────────────

fn strip_comment(line: &str) -> &str {
    // `;` and `//` start a comment. A `"` opens a string literal, so don't strip inside
    // one (cs2 strings can contain `;`/`/`).
    let bytes = line.as_bytes();
    let mut in_str = false;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => in_str = !in_str,
            b'\\' if in_str => i += 1, // skip escaped char
            b';' if !in_str => return &line[..i],
            b'/' if !in_str && i + 1 < bytes.len() && bytes[i + 1] == b'/' => return &line[..i],
            _ => {}
        }
        i += 1;
    }
    line
}

pub(crate) fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

fn unquote(s: &str) -> AsmResult<String> {
    let s = s.trim();
    let inner = s
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .ok_or_else(|| format!("expected a quoted string, got {s}"))?;
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('\\') => out.push('\\'),
            Some('"') => out.push('"'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some(other) => return Err(format!("invalid escape \\{other}")),
            None => return Err("trailing backslash in string".to_owned()),
        }
    }
    Ok(out)
}

fn parse_i32(s: &str) -> AsmResult<i32> {
    s.trim().parse().map_err(|_| format!("invalid integer {s}"))
}

fn parse_u16(s: &str) -> AsmResult<u16> {
    s.trim().parse().map_err(|_| format!("invalid count {s}"))
}

fn at(lineno: usize, msg: &str) -> String {
    format!("line {}: {msg}", lineno + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn script(
        pairs: &[(u16, i32)],
        strings: &[(usize, &str)],
        counts: (u16, u16, u16, u16),
    ) -> ClientScript {
        let mut instructions = Vec::with_capacity(pairs.len());
        let mut int_operands = vec![0i32; pairs.len()];
        let mut string_operands = vec![String::new(); pairs.len()];
        for (i, &(op, v)) in pairs.iter().enumerate() {
            instructions.push(op);
            int_operands[i] = v;
        }
        for &(i, s) in strings {
            string_operands[i] = s.to_owned();
        }
        ClientScript {
            name: None,
            instructions,
            int_operands,
            string_operands,
            int_arg_count: counts.0,
            string_arg_count: counts.1,
            int_local_count: counts.2,
            string_local_count: counts.3,
        }
    }

    /// Disassemble → assemble → encode must reproduce the original `encode` bytes.
    fn assert_round_trip(s: &ClientScript, names: &NameMaps) {
        let text = disassemble(s, names);
        let back = assemble(&text, names).unwrap_or_else(|e| panic!("assemble failed: {e}\n{text}"));
        assert_eq!(
            back.encode(),
            s.encode(),
            "byte mismatch.\n--- disassembly ---\n{text}"
        );
    }

    #[test]
    fn numeric_round_trip_all_operand_widths() {
        let s = script(
            &[
                (0, 123_456), // push_constant_int (4-byte)
                (3, 0),       // push_constant_string
                (100, 1),     // cc_create (1-byte secondary)
                (4000, 0),    // add (filler, omitted)
                (21, 0),      // return
            ],
            &[(1, "hello world")],
            (1, 0, 2, 0),
        );
        assert_round_trip(&s, &NameMaps::new());
    }

    #[test]
    fn branch_labels_round_trip() {
        // push 1; branch_equals +1 → pc 3; return; return (target)
        let s = script(&[(0, 1), (8, 1), (21, 0), (21, 0)], &[], (0, 0, 0, 0));
        let text = disassemble(&s, &NameMaps::new());
        assert!(text.contains("branch_equals label_00"), "{text}");
        assert_round_trip(&s, &NameMaps::new());
    }

    #[test]
    fn branch_to_end_round_trips() {
        // branch_equals targeting one-past-the-end (offset +1 at pc 1 of len 2 → pc 3? )
        // Use offset 0 at pc 1 → target pc 2 == n.
        let s = script(&[(0, 1), (8, 0)], &[], (0, 0, 0, 0));
        assert_round_trip(&s, &NameMaps::new());
    }

    #[test]
    fn symbolic_names_round_trip_and_render() {
        let mut names = NameMaps::new();
        names.set_scripts(&BTreeMap::from([(42u32, "login_handler".to_owned())]));
        names.set_varps(&BTreeMap::from([(286u32, "world_var".to_owned())]));
        // push_varp 286; pop_varp 286; gosub 42
        let s = script(&[(1, 286), (2, 286), (40, 42)], &[], (0, 0, 0, 0));
        let text = disassemble(&s, &names);
        assert!(text.contains("push_varp world_var"), "{text}");
        assert!(text.contains("gosub_with_params login_handler"), "{text}");
        assert_round_trip(&s, &names);
    }

    #[test]
    fn unnamed_symbolic_operands_fall_back_to_numeric() {
        let s = script(&[(1, 286), (40, 42)], &[], (0, 0, 0, 0));
        let text = disassemble(&s, &NameMaps::new());
        assert!(text.contains("push_varp 286"), "{text}");
        assert!(text.contains("gosub_with_params 42"), "{text}");
        assert_round_trip(&s, &NameMaps::new());
    }

    #[test]
    fn name_directive_round_trips() {
        let mut s = script(&[(21, 0)], &[], (0, 0, 0, 0));
        s.name = Some("debug_script".to_owned());
        let text = disassemble(&s, &NameMaps::new());
        assert!(text.contains(".name \"debug_script\""), "{text}");
        assert_round_trip(&s, &NameMaps::new());
    }

    #[test]
    fn comments_and_blank_lines_are_ignored() {
        let text = "\
; a comment
.int_args 0
.str_args 0
.int_locals 0
.str_locals 0

  push_constant_int 7  ; inline comment
  return
";
        let s = assemble(text, &NameMaps::new()).unwrap();
        assert_eq!(s.instructions, vec![0, 21]);
        assert_eq!(s.int_operands[0], 7);
    }

    #[test]
    fn string_with_semicolon_is_preserved() {
        let s = script(&[(3, 0)], &[(0, "a; b // c")], (0, 0, 0, 0));
        assert_round_trip(&s, &NameMaps::new());
    }
}
