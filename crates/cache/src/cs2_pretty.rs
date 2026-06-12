//! CS2 pretty-printer. Folds raw bytecode into "logical lines" that read closer to
//! source-level RuneScript than to bytecode.
//!
//! Three folds are applied in a single forward pass:
//!
//! 1. **Push compaction** — every `push_constant_int` / `push_int_local` / `push_varp`
//!    etc. becomes the single mnemonic `push` with a typed sigil operand
//!    (`%il0`, `@varp[286]`, `123`, `"text"`). The opcode-specific name is preserved
//!    only as the operand kind.
//!
//! 2. **`push_const_int N` + conditional branch → `if cmp N → label`**
//!    The push of the comparand and the branch collapse into one line. The "other"
//!    value being compared remains on the stack from earlier ops — that part of the
//!    bytecode isn't hidden.
//!
//! 3. **conditional branch + unconditional branch → `if cmp N → A else → B`**
//!    Java-emitted CS2 routinely follows a cond-branch with an unconditional jump to
//!    the else-arm. We surface that explicitly.
//!
//! Stack-depth tracking is best-effort: when an opcode's `stack_delta` is `None` we
//! emit `?` for the column and resume the next time we hit a known op. Branch targets
//! that are between the two halves of a fold mean we don't fold — the source PC of
//! the would-be-hidden op gets its own label so we never lose a jump target.

use crate::cs2::{ClientScript, OP_PUSH_CONST_STRING};
use crate::cs2_opcodes::{
    branch_keyword, is_branch, is_conditional_branch, is_unconditional_branch, mnemonic,
    opcode_name, operand_kind, stack_delta, OperandKind,
};
use std::collections::BTreeMap;

/// One rendered line of pretty-printed disassembly. May cover 1..=3 source PCs
/// depending on whether peephole folds applied. The first entry of `addrs` is the
/// "primary" pc — labels are emitted for that.
#[derive(Debug, Clone)]
pub struct Line {
    pub addrs: Vec<usize>,
    /// Mnemonic column (`push`, `if eq`, `branch`, `return`, etc).
    pub mnemonic: String,
    /// Operand column (`%il0`, `@varp[286]`, `→label_07`, `123`, `"text"`).
    pub operand: String,
    /// Int-stack depth AFTER this line. `None` when arity is unknown for any op
    /// covered by this line.
    pub int_depth: Option<i32>,
    /// String-stack depth AFTER this line. `None` when arity unknown.
    pub str_depth: Option<i32>,
    /// Optional comment text (resolved obj/npc/loc name, gosub script name, etc.).
    /// Rendered to the right of the operand in a dim colour.
    pub annotation: Option<String>,
}

/// Supplies friendly names for config-typed operands. The printer queries this when it
/// sees `push_constant_int N` followed by an op that consumes a known config type
/// (e.g. `oc_name` → obj id). Implementors that don't have cache access return `None`
/// for everything (see [`NullResolver`]).
pub trait NameResolver {
    fn obj_name(&mut self, id: i32) -> Option<String>;
    fn npc_name(&mut self, id: i32) -> Option<String>;
    fn loc_name(&mut self, id: i32) -> Option<String>;
    fn seq_name(&mut self, id: i32) -> Option<String>;
    /// Name of the script that this gosub targets — usually `None` for OSRS caches
    /// (the field is debug-only) but useful for hand-named scripts and tests.
    fn script_name(&mut self, id: i32) -> Option<String>;
}

/// No-op resolver — every lookup returns `None`. Use in contexts without cache access.
pub struct NullResolver;

impl NameResolver for NullResolver {
    fn obj_name(&mut self, _: i32) -> Option<String> { None }
    fn npc_name(&mut self, _: i32) -> Option<String> { None }
    fn loc_name(&mut self, _: i32) -> Option<String> { None }
    fn seq_name(&mut self, _: i32) -> Option<String> { None }
    fn script_name(&mut self, _: i32) -> Option<String> { None }
}

/// Pre-pass result — labels keyed by source pc, value is the label number assigned in
/// address-ascending order (so `label_00` is the first target encountered top-to-bottom).
pub fn compute_labels(script: &ClientScript) -> BTreeMap<usize, u32> {
    let mut targets: BTreeMap<usize, u32> = BTreeMap::new();
    for (pc, &op) in script.instructions.iter().enumerate() {
        if !is_branch(op) {
            continue;
        }
        let target = (pc as i64 + 1 + script.int_operands[pc] as i64) as usize;
        if target < script.instructions.len() {
            targets.insert(target, 0);
        }
    }
    for (i, (_, slot)) in targets.iter_mut().enumerate() {
        *slot = i as u32;
    }
    targets
}

/// Main pretty-printer entry point. Returns one [`Line`] per logical step.
///
/// Use [`pretty_with`] when you have a [`NameResolver`] hooked into the cache for
/// friendly config-id annotations; this convenience wrapper passes a [`NullResolver`].
#[must_use]
pub fn pretty(script: &ClientScript, labels: &BTreeMap<usize, u32>) -> Vec<Line> {
    pretty_with(script, labels, &mut NullResolver)
}

/// Pretty-print with a custom resolver. The resolver is called for:
/// - Every `gosub_with_params` — annotation is the target script's name if known.
/// - Every `push_constant_int N` whose next non-folded op consumes a config type
///   (`oc_*` → obj id, `inv_get*`/`inv_total` second push → obj id, …). The push line
///   gets the resolved name as its annotation.
pub fn pretty_with(
    script: &ClientScript,
    labels: &BTreeMap<usize, u32>,
    resolver: &mut dyn NameResolver,
) -> Vec<Line> {
    let mut out = Vec::with_capacity(script.instructions.len());
    let mut int_depth: Option<i32> = Some(i32::from(script.int_arg_count));
    let mut str_depth: Option<i32> = Some(i32::from(script.string_arg_count));
    let mut pc = 0usize;
    let n = script.instructions.len();

    while pc < n {
        // Reset stack-depth tracking at any label boundary — converging control flow
        // can reach a label with any depth, so reporting it as `?` is more accurate
        // than the value carried over from the textually-preceding line.
        if labels.contains_key(&pc) {
            int_depth = None;
            str_depth = None;
        }

        let op = script.instructions[pc];

        // --- Fold #1+#2: push_const_int N + cond branch → "if cmp N → label" ---
        // Skip folding when something else jumps to the cond branch (because the push
        // is part of a separate logical line that would lose its address).
        if op == 0 && pc + 1 < n {
            let next = script.instructions[pc + 1];
            if is_conditional_branch(next) && !labels.contains_key(&(pc + 1)) {
                // Try a 3-way fold: cond+uncond → "if cmp N → A else → B".
                let elsefold_ok = pc + 2 < n
                    && is_unconditional_branch(script.instructions[pc + 2])
                    && !labels.contains_key(&(pc + 2));
                let cmp_val = script.int_operands[pc];
                let cmp_op = next;
                let cmp_target = branch_target(script, pc + 1);
                let kw = branch_keyword(cmp_op);

                if elsefold_ok {
                    let else_target = branch_target(script, pc + 2);
                    let mn = format!("if {kw}");
                    let opnd = format!(
                        "{cmp_val}  → {}  else → {}",
                        label_or_addr(labels, cmp_target),
                        label_or_addr(labels, else_target),
                    );
                    advance_depth(&mut int_depth, &mut str_depth, op);
                    advance_depth(&mut int_depth, &mut str_depth, cmp_op);
                    advance_depth(&mut int_depth, &mut str_depth, script.instructions[pc + 2]);
                    out.push(Line {
                        addrs: vec![pc, pc + 1, pc + 2],
                        mnemonic: mn,
                        operand: opnd,
                        int_depth,
                        str_depth,
                        annotation: None,
                    });
                    pc += 3;
                    continue;
                } else {
                    let mn = format!("if {kw}");
                    let opnd =
                        format!("{cmp_val}  → {}", label_or_addr(labels, cmp_target));
                    advance_depth(&mut int_depth, &mut str_depth, op);
                    advance_depth(&mut int_depth, &mut str_depth, cmp_op);
                    out.push(Line {
                        addrs: vec![pc, pc + 1],
                        mnemonic: mn,
                        operand: opnd,
                        int_depth,
                        str_depth,
                        annotation: None,
                    });
                    pc += 2;
                    continue;
                }
            }
        }

        // --- No fold: emit a single line ---
        let line = emit_one(script, labels, pc, op, &mut int_depth, &mut str_depth, resolver);
        out.push(line);
        pc += 1;
    }

    // Second pass: annotate `push_constant_int N` whose next line consumes a known
    // config-typed value. We do this post-hoc so the folding logic above stays simple
    // — folds emit `Line`s with `annotation: None` and we backfill them here.
    annotate_config_pushes(script, &mut out, resolver);

    out
}

/// Walk the rendered lines and add config-name annotations to `push N` lines whose
/// immediate next line is an `oc_*` op (or similar predictable consumer). Limited to
/// the cases where the operand's type is unambiguous from the consumer.
fn annotate_config_pushes(
    script: &ClientScript,
    lines: &mut [Line],
    resolver: &mut dyn NameResolver,
) {
    for i in 0..lines.len() {
        let line = &lines[i];
        // Only consider single-op `push N` lines (folded if/else lines already render
        // the literal in context).
        if line.addrs.len() != 1 {
            continue;
        }
        let pc = line.addrs[0];
        if script.instructions[pc] != 0 {
            continue; // not push_constant_int
        }
        let n = script.int_operands[pc];
        // What does the NEXT logical line consume?
        let Some(next) = lines.get(i + 1) else { continue };
        let next_pc = next.addrs[0];
        let next_op = script.instructions[next_pc];
        let annotation = match next_op {
            // oc_* ops consume one obj id on top of stack.
            4200..=4207 => resolver.obj_name(n),
            // inv_getobj / inv_getnum / inv_total / invother_* take inv id then obj id.
            3303 => resolver.obj_name(n), // inv_total
            _ => None,
        };
        if annotation.is_some() {
            lines[i].annotation = annotation;
        }
    }
    // gosub_with_params annotation — script name (if known).
    for line in lines.iter_mut() {
        if line.addrs.len() != 1 {
            continue;
        }
        let pc = line.addrs[0];
        if script.instructions[pc] == 40 {
            if let Some(n) = resolver.script_name(script.int_operands[pc]) {
                line.annotation = Some(n);
            }
        }
    }
}

/// Emit one source-pc as a single line, applying the operand-sigil formatting from
/// the opcode table. Depth tracking is advanced in place. Annotations are filled in by
/// the post-pass [`annotate_config_pushes`] — `resolver` is unused here today, kept
/// in the signature so a future per-op annotation (e.g. mid-stream npc lookups) can
/// hook in without further plumbing.
fn emit_one(
    script: &ClientScript,
    labels: &BTreeMap<usize, u32>,
    pc: usize,
    op: u16,
    int_depth: &mut Option<i32>,
    str_depth: &mut Option<i32>,
    _resolver: &mut dyn NameResolver,
) -> Line {
    let mn = mnemonic_with_branch_suffix(op);
    let opnd = format_operand(script, labels, pc, op);
    advance_depth(int_depth, str_depth, op);
    Line {
        addrs: vec![pc],
        mnemonic: mn,
        operand: opnd,
        int_depth: *int_depth,
        str_depth: *str_depth,
        annotation: None,
    }
}

/// For unfolded conditional branches, render `if cmp` instead of bare `if`. The
/// opcode table maps all branch_* ops to mnemonic `if`; this glues the keyword on.
fn mnemonic_with_branch_suffix(op: u16) -> String {
    if is_conditional_branch(op) {
        format!("if {}", branch_keyword(op))
    } else {
        mnemonic(op).to_owned()
    }
}

/// Format the operand by `OperandKind`. Branches resolve to label refs, gosubs to
/// `→script #N`, strings get debug-quoted, filler is empty.
fn format_operand(
    script: &ClientScript,
    labels: &BTreeMap<usize, u32>,
    pc: usize,
    op: u16,
) -> String {
    if op == OP_PUSH_CONST_STRING {
        return format!("{:?}", script.string_operands[pc]);
    }
    let operand = script.int_operands[pc];
    match operand_kind(op) {
        OperandKind::Filler => String::new(),
        OperandKind::Int => operand.to_string(),
        OperandKind::String => format!("{:?}", script.string_operands[pc]),
        OperandKind::VarpId => format!("@varp[{operand}]"),
        OperandKind::VarbitId => format!("@varbit[{operand}]"),
        OperandKind::VarcIntId => format!("@varc_int[{operand}]"),
        OperandKind::VarcStrId => format!("@varc_str[{operand}]"),
        OperandKind::LocalInt => format!("%il{operand}"),
        OperandKind::LocalStr => format!("%sl{operand}"),
        OperandKind::BranchOffset => {
            let target = (pc as i64 + 1 + operand as i64) as usize;
            format!("→ {}", label_or_addr(labels, target))
        }
        OperandKind::ScriptId => format!("→script #{operand}"),
        OperandKind::ArraySlot => {
            // 1-byte operand = array id (most callers pass index on stack).
            format!("array[{}]", operand & 0xFF)
        }
        OperandKind::ArrayDef => {
            let id = (operand >> 16) & 0xFFFF;
            let typ = operand & 0xFFFF;
            format!("array[{id}] type={typ}")
        }
        OperandKind::JoinCount => format!("n={operand}"),
        OperandKind::SecondaryFlag => {
            // 0 = primary active component, 1 = secondary. Hide the common 0 case so
            // we don't drown the listing in a column of zeros.
            if operand == 0 {
                String::new()
            } else {
                "[secondary]".to_owned()
            }
        }
    }
}

fn label_or_addr(labels: &BTreeMap<usize, u32>, target: usize) -> String {
    labels
        .get(&target)
        .map_or_else(|| format!("{target:04}"), |n| format!("label_{n:02}"))
}

fn branch_target(script: &ClientScript, pc: usize) -> usize {
    (pc as i64 + 1 + script.int_operands[pc] as i64) as usize
}

fn advance_depth(int_depth: &mut Option<i32>, str_depth: &mut Option<i32>, op: u16) {
    match stack_delta(op) {
        Some((id, sd)) => {
            if let Some(d) = int_depth {
                *d += id;
            }
            if let Some(d) = str_depth {
                *d += sd;
            }
        }
        None => {
            *int_depth = None;
            *str_depth = None;
        }
    }
}

/// Look up the original Java-style opcode name for cross-reference with ScriptRunner.
/// Re-exported here so `cs2_view` doesn't have to import both modules.
#[must_use]
pub fn original_name(op: u16) -> Option<&'static str> {
    opcode_name(op)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal script from raw (op, operand) pairs for testing.
    fn script_from(pairs: &[(u16, i32)], strings: &[(usize, &str)]) -> ClientScript {
        let mut int_operands = vec![0i32; pairs.len()];
        let mut string_operands = vec![String::new(); pairs.len()];
        let mut instructions = Vec::with_capacity(pairs.len());
        for (i, &(op, n)) in pairs.iter().enumerate() {
            instructions.push(op);
            int_operands[i] = n;
        }
        for (i, s) in strings {
            string_operands[*i] = s.to_string();
        }
        ClientScript {
            name: None,
            instructions,
            int_operands,
            string_operands,
            int_local_count: 0,
            string_local_count: 0,
            int_arg_count: 0,
            string_arg_count: 0,
        }
    }

    #[test]
    fn folds_push_and_cond_branch() {
        // push 1; branch_equals → label_00 (offset +1, so target is pc=3)
        // followed by an unrelated op so we have a label target.
        let s = script_from(
            &[
                (0, 1),   // push 1
                (8, 1),   // branch_equals offset +1 → pc 3
                (21, 0),  // return (filler)
                (21, 0),  // return (label target)
            ],
            &[],
        );
        let labels = compute_labels(&s);
        let lines = pretty(&s, &labels);

        // Expect: one folded line for pc 0+1, then two `return` lines.
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].addrs, vec![0, 1]);
        assert_eq!(lines[0].mnemonic, "if eq");
        assert!(lines[0].operand.contains("1"));
        assert!(lines[0].operand.contains("label_"));
    }

    #[test]
    fn folds_push_cond_uncond_into_if_else() {
        // push 1; branch_equals → pc 4 (offset +2); branch → pc 5 (offset +1).
        // Need 5 entries so both targets exist.
        let s = script_from(
            &[
                (0, 1),   // push 1
                (8, 2),   // branch_equals +2 → pc 4
                (6, 1),   // branch +1 → pc 5
                (21, 0),  // padding instruction
                (21, 0),  // pc 4 target
                (21, 0),  // pc 5 target
            ],
            &[],
        );
        let labels = compute_labels(&s);
        let lines = pretty(&s, &labels);

        assert_eq!(lines[0].addrs, vec![0, 1, 2]);
        assert_eq!(lines[0].mnemonic, "if eq");
        assert!(lines[0].operand.contains("else"));
    }

    #[test]
    fn push_uses_typed_sigil_per_operand_kind() {
        let s = script_from(
            &[
                (0, 42),    // push_constant_int → "42"
                (1, 286),   // push_varp        → "@varp[286]"
                (33, 1),    // push_int_local   → "%il1"
                (35, 0),    // push_string_local→ "%sl0"
            ],
            &[],
        );
        let labels = compute_labels(&s);
        let lines = pretty(&s, &labels);
        assert_eq!(lines[0].operand, "42");
        assert_eq!(lines[1].operand, "@varp[286]");
        assert_eq!(lines[2].operand, "%il1");
        assert_eq!(lines[3].operand, "%sl0");
        // All folded into a single mnemonic.
        assert!(lines.iter().all(|l| l.mnemonic == "push"));
    }

    #[test]
    fn filler_operand_renders_empty() {
        let s = script_from(&[(21, 0), (4000, 0)], &[]);
        let labels = compute_labels(&s);
        let lines = pretty(&s, &labels);
        assert_eq!(lines[0].operand, "");
        assert_eq!(lines[1].operand, "");
        assert_eq!(lines[0].mnemonic, "return");
        assert_eq!(lines[1].mnemonic, "add");
    }

    #[test]
    fn stack_depth_tracks_arity() {
        // push 1; push 2; add → depth should go 1, 2, 1.
        let s = script_from(&[(0, 1), (0, 2), (4000, 0)], &[]);
        let labels = compute_labels(&s);
        let lines = pretty(&s, &labels);
        assert_eq!(lines[0].int_depth, Some(1));
        assert_eq!(lines[1].int_depth, Some(2));
        assert_eq!(lines[2].int_depth, Some(1));
    }

    #[test]
    fn folding_disabled_when_push_jumped_into() {
        // push 1; branch_eq +0 → pc 2 (the branch itself) — would normally fold, but
        // we still try since the LABEL is on the BRANCH, not the push. Actually the
        // skip-guard is on the cond branch, not the push. Make a clearer case:
        // op0:push, op1:cond_branch (target pc=4), op2:return, op3:branch jumps to op1.
        // Folding op0+op1 would hide op1, which op3 jumps to → must not fold.
        let s = script_from(
            &[
                (0, 1),    // pc 0 push
                (8, 2),    // pc 1 branch_eq +2 → pc 4 (label target #1)
                (21, 0),   // pc 2 return
                (6, -3),   // pc 3 branch -3 → pc 1 (back-edge into the branch itself)
                (21, 0),   // pc 4
            ],
            &[],
        );
        let labels = compute_labels(&s);
        let lines = pretty(&s, &labels);
        // Should NOT fold pc 0+1 because pc 1 is a label target.
        assert!(lines.iter().any(|l| l.addrs == vec![0]));
        assert!(lines.iter().any(|l| l.addrs == vec![1]));
    }
}
