//! CS2 opcode metadata — names, mnemonics, operand kinds, exact stack arity.
//!
//! Single source of truth for the disassembler, pretty-printer, and decompiler.
//! Adding a new opcode means filling in all four facets here; the printer / view /
//! lifter code has zero per-opcode knowledge.
//!
//! Sourced from the dispatch chain in `src/main/java/jagex3/client/ScriptRunner.java`
//! (one giant `if (opcode == N) {...}` cascade). Range conventions:
//!
//! - 0..50: stack / locals / branches / gosub / arrays
//! - 100..200: `cc_*` creation
//! - 1000..1424: `cc_set*` mutators on the active component (1400..1424: event-handler
//!   installers)
//! - 1500..1802: `cc_get*` readers (active component)
//! - 2000..2424: `if_set*` — same handlers as 1000..1424 (Java does `opcode -= 1000`)
//!   but the component id is popped from the int stack instead of using the active
//!   component (`ScriptRunner.java:525-531` and the equivalent prologue in each block)
//! - 2500..2802: `if_get*` readers (component id on stack)
//! - 3100..3700: game-world state (`mes`, `anim`, `inv_*`, `stat_*`, `friend_*`, …)
//! - 4000..4099: arithmetic + bitwise
//! - 4100..4199: string ops
//! - 4200..4299: `oc_*` (object/inv-item type accessors)
//! - 5000..5099: chat / social

/// What the bytecode operand at this opcode position represents — controls how the
/// disassembler formats it (sigil, hex, label ref, etc.).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperandKind {
    /// No meaningful operand. The bytecode byte/int exists but is always 0 / ignored
    /// (e.g. arithmetic ops, getters that pull args from the stack). Printer hides it.
    Filler,
    /// Literal integer constant.
    Int,
    /// String literal (stored in `string_operands`, not `int_operands`).
    String,
    /// Player variable index — printed as `@varp[N]`.
    VarpId,
    /// Varbit id — printed as `@varbit[N]`.
    VarbitId,
    /// Client int variable index — printed as `@varc_int[N]`.
    VarcIntId,
    /// Client string variable index — printed as `@varc_str[N]`.
    VarcStrId,
    /// Local int slot — printed as `%il[N]`.
    LocalInt,
    /// Local string slot — printed as `%sl[N]`.
    LocalStr,
    /// Branch offset relative to the next pc. Printer resolves to a label.
    BranchOffset,
    /// Gosub target — another script id. Printer prefixes `→script #N`.
    ScriptId,
    /// Array id for push/pop_array_int — the slot index comes from the stack
    /// (`ScriptRunner.java:384-411`), the operand is just the array number (0..4).
    ArraySlot,
    /// Array definition for op 44: `(arrayId << 16) | typeChar`.
    ArrayDef,
    /// Count of strings to concatenate (join_string).
    JoinCount,
    /// Component-secondary flag byte (0 = activeComponent, 1 = activeComponent2). Most
    /// cc_ ops carry this as their 1-byte operand; the `if_*` mirrors (2000..2424)
    /// still carry the byte but ignore it (component id comes from the stack).
    SecondaryFlag,
}

/// Exact stack effect of an opcode. `Fixed` covers ~95% of the table; the rest depend
/// on an operand, the callee's signature, or a stack value, and are resolved during
/// symbolic execution (see `cs2_decompile`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arity {
    /// Pops `ipop` ints + `spop` strings, pushes `ipush` ints + `spush` strings.
    Fixed { ipop: u8, spop: u8, ipush: u8, spush: u8 },
    /// op 21 `return` — leaves the current frame; whatever remains on the stacks are
    /// the script's return values. Per-script return counts are inferred (all return
    /// sites of a script must leave identical depths).
    Return,
    /// op 37 `join_string` — pops `int_operand` strings, pushes the joined string.
    JoinString,
    /// op 40 `gosub_with_params` — pops the callee's declared int/string args
    /// (`ClientScript` trailer), pushes the callee's inferred returns.
    Gosub,
    /// 1400..1424 / 2400..2424 event-handler installers (`ScriptRunner.java:894-1039`).
    /// Pop order: [component id (int) if `component_from_stack`], descriptor string,
    /// then if the descriptor ends in `'Y'`: an int count + that many ints (transmit
    /// trigger list), then one value per remaining descriptor char (`'s'` → string,
    /// else int) in reverse, finally the handler script id (int). Statically resolvable
    /// only when the descriptor (top of string stack) and the `Y`-count are constants.
    EventHandler { component_from_stack: bool },
    /// op 3408 `enum` — pops 4 ints (inputtype, outputtype, enum id, key); pushes a
    /// string when the popped `outputtype` is `'s'` (115), an int otherwise
    /// (`ScriptRunner.java:1623-1663`). Resolvable when outputtype is a constant push.
    EnumLookup,
}

/// Short snake_case identifier — the historical Java method name. Useful as the
/// "primary" label callers can cross-reference back to ScriptRunner.java.
#[must_use]
pub fn opcode_name(op: u16) -> Option<&'static str> {
    OPCODES.iter().find(|e| e.op == op).map(|e| e.name)
}

/// Reverse of [`opcode_name`] — resolve a canonical snake_case name back to its opcode.
/// Used by the assembler to turn `.cs2` mnemonics into opcodes.
#[must_use]
pub fn opcode_by_name(name: &str) -> Option<u16> {
    OPCODES.iter().find(|e| e.name == name).map(|e| e.op)
}

/// Compact disassembler mnemonic — `push`, `pop`, `if eq`, `return`, etc. Multiple
/// opcodes can share a mnemonic (all push_* → `push`, distinguished by operand kind).
#[must_use]
pub fn mnemonic(op: u16) -> &'static str {
    OPCODES.iter().find(|e| e.op == op).map_or("?", |e| e.mnemonic)
}

/// Operand interpretation for this opcode (controls printer formatting).
#[must_use]
pub fn operand_kind(op: u16) -> OperandKind {
    OPCODES.iter().find(|e| e.op == op).map_or(OperandKind::Int, |e| e.kind)
}

/// Exact stack arity for this opcode, `None` if the opcode is unknown.
#[must_use]
pub fn arity(op: u16) -> Option<Arity> {
    OPCODES.iter().find(|e| e.op == op).map(|e| e.arity)
}

/// Every opcode in the table, ascending. For exporters (e.g. the generator that keeps
/// the Kotlin compiler's `cs2_opcodes.tsv` in sync with this table).
pub fn all_opcodes() -> impl Iterator<Item = u16> {
    OPCODES.iter().map(|e| e.op)
}

/// `(int_delta, str_delta)` — net change in int-stack / string-stack depth.
/// Derived from [`arity`]; `None` for unknown opcodes and the dynamic-arity ops
/// (return / join_string / gosub / event handlers / enum), where the delta depends
/// on context. Kept for the pretty-printer's best-effort depth column.
#[must_use]
pub fn stack_delta(op: u16) -> Option<(i32, i32)> {
    match arity(op)? {
        Arity::Fixed { ipop, spop, ipush, spush } => Some((
            i32::from(ipush) - i32::from(ipop),
            i32::from(spush) - i32::from(spop),
        )),
        _ => None,
    }
}

/// True for branch / jump-relative opcodes — operand is a relative offset.
#[must_use]
pub fn is_branch(op: u16) -> bool {
    matches!(op, 6 | 7 | 8 | 9 | 10 | 31 | 32)
}

/// True for the unconditional branch (op 6). Used by the printer to distinguish
/// `branch` from `if cmp` when folding else-clauses.
#[must_use]
pub fn is_unconditional_branch(op: u16) -> bool {
    op == 6
}

/// True if the operand should be displayed as a script id (gosub).
#[must_use]
pub fn is_gosub(op: u16) -> bool {
    op == 40
}

/// True for the conditional branch family — `branch_equals`, `branch_not`, `branch_lt`,
/// `branch_gt`, `branch_le`, `branch_ge`. The printer folds `push N` + cond-branch into
/// a single `if cmp N → label` line.
#[must_use]
pub fn is_conditional_branch(op: u16) -> bool {
    matches!(op, 7 | 8 | 9 | 10 | 31 | 32)
}

/// `if`-clause keyword for a conditional branch. Returns the operator only (e.g. `"eq"`).
/// Returns `""` for non-conditional ops.
///
/// Comparison order matches the Java: `A op B` where `A` was pushed first
/// (`intStack[isp]`) and `B` second (`intStack[isp + 1]`).
#[must_use]
pub fn branch_keyword(op: u16) -> &'static str {
    match op {
        7 => "neq",
        8 => "eq",
        9 => "lt",
        10 => "gt",
        31 => "le",
        32 => "ge",
        _ => "",
    }
}

/// Table row.
struct Entry {
    op: u16,
    name: &'static str,
    mnemonic: &'static str,
    kind: OperandKind,
    arity: Arity,
}

use OperandKind as OK;

/// Shorthand for [`Arity::Fixed`] — keeps the table rows readable.
const fn fx(ipop: u8, spop: u8, ipush: u8, spush: u8) -> Arity {
    Arity::Fixed { ipop, spop, ipush, spush }
}

/// `cc_seton*` family (active component) — descriptor-driven arity.
const EV_CC: Arity = Arity::EventHandler { component_from_stack: false };
/// `if_seton*` family (component id on stack) — descriptor-driven arity + 1 int pop.
const EV_IF: Arity = Arity::EventHandler { component_from_stack: true };

#[rustfmt::skip]
const OPCODES: &[Entry] = &[
    // --- 0..48: core stack-machine ---
    Entry { op: 0,  name: "push_constant_int",    mnemonic: "push",   kind: OK::Int,          arity: fx(0, 0, 1, 0) },
    Entry { op: 1,  name: "push_varp",            mnemonic: "push",   kind: OK::VarpId,       arity: fx(0, 0, 1, 0) },
    Entry { op: 2,  name: "pop_varp",             mnemonic: "pop",    kind: OK::VarpId,       arity: fx(1, 0, 0, 0) },
    Entry { op: 3,  name: "push_constant_string", mnemonic: "push",   kind: OK::String,       arity: fx(0, 0, 0, 1) },
    Entry { op: 6,  name: "branch",               mnemonic: "branch", kind: OK::BranchOffset, arity: fx(0, 0, 0, 0) },
    Entry { op: 7,  name: "branch_not",           mnemonic: "if",     kind: OK::BranchOffset, arity: fx(2, 0, 0, 0) },
    Entry { op: 8,  name: "branch_equals",        mnemonic: "if",     kind: OK::BranchOffset, arity: fx(2, 0, 0, 0) },
    Entry { op: 9,  name: "branch_less_than",     mnemonic: "if",     kind: OK::BranchOffset, arity: fx(2, 0, 0, 0) },
    Entry { op: 10, name: "branch_greater_than",  mnemonic: "if",     kind: OK::BranchOffset, arity: fx(2, 0, 0, 0) },
    Entry { op: 21, name: "return",               mnemonic: "return", kind: OK::Filler,       arity: Arity::Return },
    Entry { op: 25, name: "push_varbit",          mnemonic: "push",   kind: OK::VarbitId,     arity: fx(0, 0, 1, 0) },
    Entry { op: 27, name: "pop_varbit",           mnemonic: "pop",    kind: OK::VarbitId,     arity: fx(1, 0, 0, 0) },
    Entry { op: 31, name: "branch_le",            mnemonic: "if",     kind: OK::BranchOffset, arity: fx(2, 0, 0, 0) },
    Entry { op: 32, name: "branch_ge",            mnemonic: "if",     kind: OK::BranchOffset, arity: fx(2, 0, 0, 0) },
    Entry { op: 33, name: "push_int_local",       mnemonic: "push",   kind: OK::LocalInt,     arity: fx(0, 0, 1, 0) },
    Entry { op: 34, name: "pop_int_local",        mnemonic: "pop",    kind: OK::LocalInt,     arity: fx(1, 0, 0, 0) },
    Entry { op: 35, name: "push_string_local",    mnemonic: "push",   kind: OK::LocalStr,     arity: fx(0, 0, 0, 1) },
    Entry { op: 36, name: "pop_string_local",     mnemonic: "pop",    kind: OK::LocalStr,     arity: fx(0, 1, 0, 0) },
    Entry { op: 37, name: "join_string",          mnemonic: "join",   kind: OK::JoinCount,    arity: Arity::JoinString },
    Entry { op: 38, name: "pop_int_discard",      mnemonic: "drop",   kind: OK::Filler,       arity: fx(1, 0, 0, 0) },
    Entry { op: 39, name: "pop_string_discard",   mnemonic: "drop_s", kind: OK::Filler,       arity: fx(0, 1, 0, 0) },
    Entry { op: 40, name: "gosub_with_params",    mnemonic: "gosub",  kind: OK::ScriptId,     arity: Arity::Gosub },
    Entry { op: 42, name: "push_varc_int",        mnemonic: "push",   kind: OK::VarcIntId,    arity: fx(0, 0, 1, 0) },
    Entry { op: 43, name: "pop_varc_int",         mnemonic: "pop",    kind: OK::VarcIntId,    arity: fx(1, 0, 0, 0) },
    Entry { op: 44, name: "define_array",         mnemonic: "array_def", kind: OK::ArrayDef,  arity: fx(1, 0, 0, 0) },
    Entry { op: 45, name: "push_array_int",       mnemonic: "push",   kind: OK::ArraySlot,    arity: fx(1, 0, 1, 0) },
    Entry { op: 46, name: "pop_array_int",        mnemonic: "pop",    kind: OK::ArraySlot,    arity: fx(2, 0, 0, 0) },
    Entry { op: 47, name: "push_varc_str",        mnemonic: "push",   kind: OK::VarcStrId,    arity: fx(0, 0, 0, 1) },
    Entry { op: 48, name: "pop_varc_str",         mnemonic: "pop",    kind: OK::VarcStrId,    arity: fx(0, 1, 0, 0) },

    // --- 100..200: cc_ create/find ---
    Entry { op: 100, name: "cc_create",    mnemonic: "cc_create",    kind: OK::SecondaryFlag, arity: fx(3, 0, 0, 0) },
    Entry { op: 101, name: "cc_delete",    mnemonic: "cc_delete",    kind: OK::SecondaryFlag, arity: fx(0, 0, 0, 0) },
    Entry { op: 102, name: "cc_deleteall", mnemonic: "cc_deleteall", kind: OK::SecondaryFlag, arity: fx(1, 0, 0, 0) },
    Entry { op: 200, name: "cc_find",      mnemonic: "cc_find",      kind: OK::SecondaryFlag, arity: fx(2, 0, 1, 0) },

    // --- 1000..1120: cc_set* mutators (active component; Java comments "if/cc_set*") ---
    Entry { op: 1000, name: "cc_setposition", mnemonic: "cc_setposition", kind: OK::SecondaryFlag, arity: fx(2, 0, 0, 0) },
    Entry { op: 1001, name: "cc_setsize",     mnemonic: "cc_setsize",     kind: OK::SecondaryFlag, arity: fx(2, 0, 0, 0) },
    Entry { op: 1003, name: "cc_sethide",     mnemonic: "cc_sethide",     kind: OK::SecondaryFlag, arity: fx(1, 0, 0, 0) },
    Entry { op: 1100, name: "cc_setscrollpos",     mnemonic: "cc_setscrollpos",     kind: OK::SecondaryFlag, arity: fx(2, 0, 0, 0) },
    Entry { op: 1101, name: "cc_setcolour",        mnemonic: "cc_setcolour",        kind: OK::SecondaryFlag, arity: fx(1, 0, 0, 0) },
    Entry { op: 1102, name: "cc_setfill",          mnemonic: "cc_setfill",          kind: OK::SecondaryFlag, arity: fx(1, 0, 0, 0) },
    Entry { op: 1103, name: "cc_settrans",         mnemonic: "cc_settrans",         kind: OK::SecondaryFlag, arity: fx(1, 0, 0, 0) },
    Entry { op: 1104, name: "cc_setlinewid",       mnemonic: "cc_setlinewid",       kind: OK::SecondaryFlag, arity: fx(1, 0, 0, 0) },
    Entry { op: 1105, name: "cc_setgraphic",       mnemonic: "cc_setgraphic",       kind: OK::SecondaryFlag, arity: fx(1, 0, 0, 0) },
    Entry { op: 1106, name: "cc_set2dangle",       mnemonic: "cc_set2dangle",       kind: OK::SecondaryFlag, arity: fx(1, 0, 0, 0) },
    Entry { op: 1107, name: "cc_settiling",        mnemonic: "cc_settiling",        kind: OK::SecondaryFlag, arity: fx(1, 0, 0, 0) },
    Entry { op: 1108, name: "cc_setmodel",         mnemonic: "cc_setmodel",         kind: OK::SecondaryFlag, arity: fx(1, 0, 0, 0) },
    Entry { op: 1109, name: "cc_setmodelangle",    mnemonic: "cc_setmodelangle",    kind: OK::SecondaryFlag, arity: fx(6, 0, 0, 0) },
    Entry { op: 1110, name: "cc_setmodelanim",     mnemonic: "cc_setmodelanim",     kind: OK::SecondaryFlag, arity: fx(1, 0, 0, 0) },
    Entry { op: 1111, name: "cc_setmodelorthog",   mnemonic: "cc_setmodelorthog",   kind: OK::SecondaryFlag, arity: fx(1, 0, 0, 0) },
    Entry { op: 1112, name: "cc_settext",          mnemonic: "cc_settext",          kind: OK::SecondaryFlag, arity: fx(0, 1, 0, 0) },
    Entry { op: 1113, name: "cc_settextfont",      mnemonic: "cc_settextfont",      kind: OK::SecondaryFlag, arity: fx(1, 0, 0, 0) },
    // hAlign, vAlign, lineHeight — three pops (ScriptRunner.java:717-726)
    Entry { op: 1114, name: "cc_settextalign",     mnemonic: "cc_settextalign",     kind: OK::SecondaryFlag, arity: fx(3, 0, 0, 0) },
    Entry { op: 1115, name: "cc_settextshadow",    mnemonic: "cc_settextshadow",    kind: OK::SecondaryFlag, arity: fx(1, 0, 0, 0) },
    Entry { op: 1116, name: "cc_setoutline",       mnemonic: "cc_setoutline",       kind: OK::SecondaryFlag, arity: fx(1, 0, 0, 0) },
    Entry { op: 1117, name: "cc_setgraphicshadow", mnemonic: "cc_setgraphicshadow", kind: OK::SecondaryFlag, arity: fx(1, 0, 0, 0) },
    Entry { op: 1118, name: "cc_setvflip",         mnemonic: "cc_setvflip",         kind: OK::SecondaryFlag, arity: fx(1, 0, 0, 0) },
    Entry { op: 1119, name: "cc_sethflip",         mnemonic: "cc_sethflip",         kind: OK::SecondaryFlag, arity: fx(1, 0, 0, 0) },
    Entry { op: 1120, name: "cc_setscrollsize",    mnemonic: "cc_setscrollsize",    kind: OK::SecondaryFlag, arity: fx(2, 0, 0, 0) },

    // --- 1200..1307: inventory + interaction setters ---
    Entry { op: 1200, name: "cc_setobject",            mnemonic: "cc_setobject",            kind: OK::SecondaryFlag, arity: fx(2, 0, 0, 0) },
    Entry { op: 1201, name: "cc_setnpchead",           mnemonic: "cc_setnpchead",           kind: OK::SecondaryFlag, arity: fx(1, 0, 0, 0) },
    Entry { op: 1202, name: "cc_setplayerhead_self",   mnemonic: "cc_setplayerhead_self",   kind: OK::SecondaryFlag, arity: fx(0, 0, 0, 0) },
    Entry { op: 1300, name: "cc_setop",                mnemonic: "cc_setop",                kind: OK::SecondaryFlag, arity: fx(1, 1, 0, 0) },
    Entry { op: 1301, name: "cc_setdraggable",         mnemonic: "cc_setdraggable",         kind: OK::SecondaryFlag, arity: fx(2, 0, 0, 0) },
    Entry { op: 1302, name: "cc_setdraggablebehavior", mnemonic: "cc_setdraggablebehavior", kind: OK::SecondaryFlag, arity: fx(1, 0, 0, 0) },
    Entry { op: 1303, name: "cc_setdragdeadzone",      mnemonic: "cc_setdragdeadzone",      kind: OK::SecondaryFlag, arity: fx(1, 0, 0, 0) },
    Entry { op: 1304, name: "cc_setdragdeadtime",      mnemonic: "cc_setdragdeadtime",      kind: OK::SecondaryFlag, arity: fx(1, 0, 0, 0) },
    Entry { op: 1305, name: "cc_setopbase",            mnemonic: "cc_setopbase",            kind: OK::SecondaryFlag, arity: fx(0, 1, 0, 0) },
    Entry { op: 1306, name: "cc_settargetverb",        mnemonic: "cc_settargetverb",        kind: OK::SecondaryFlag, arity: fx(0, 1, 0, 0) },
    Entry { op: 1307, name: "cc_clearops",             mnemonic: "cc_clearops",             kind: OK::SecondaryFlag, arity: fx(0, 0, 0, 0) },

    // --- 1400..1424: event-handler installers (descriptor-string-driven arity) ---
    Entry { op: 1400, name: "cc_setonclick",           mnemonic: "cc_setonclick",           kind: OK::SecondaryFlag, arity: EV_CC },
    Entry { op: 1401, name: "cc_setonhold",            mnemonic: "cc_setonhold",            kind: OK::SecondaryFlag, arity: EV_CC },
    Entry { op: 1402, name: "cc_setonrelease",         mnemonic: "cc_setonrelease",         kind: OK::SecondaryFlag, arity: EV_CC },
    Entry { op: 1403, name: "cc_setonmouseover",       mnemonic: "cc_setonmouseover",       kind: OK::SecondaryFlag, arity: EV_CC },
    Entry { op: 1404, name: "cc_setonmouseleave",      mnemonic: "cc_setonmouseleave",      kind: OK::SecondaryFlag, arity: EV_CC },
    Entry { op: 1405, name: "cc_setondrag",            mnemonic: "cc_setondrag",            kind: OK::SecondaryFlag, arity: EV_CC },
    Entry { op: 1406, name: "cc_setontargetleave",     mnemonic: "cc_setontargetleave",     kind: OK::SecondaryFlag, arity: EV_CC },
    Entry { op: 1407, name: "cc_setonvartransmit",     mnemonic: "cc_setonvartransmit",     kind: OK::SecondaryFlag, arity: EV_CC },
    Entry { op: 1408, name: "cc_setontimer",           mnemonic: "cc_setontimer",           kind: OK::SecondaryFlag, arity: EV_CC },
    Entry { op: 1409, name: "cc_setonop",              mnemonic: "cc_setonop",              kind: OK::SecondaryFlag, arity: EV_CC },
    Entry { op: 1410, name: "cc_setondragcomplete",    mnemonic: "cc_setondragcomplete",    kind: OK::SecondaryFlag, arity: EV_CC },
    Entry { op: 1411, name: "cc_setonclickrepeat",     mnemonic: "cc_setonclickrepeat",     kind: OK::SecondaryFlag, arity: EV_CC },
    Entry { op: 1412, name: "cc_setonmouserepeat",     mnemonic: "cc_setonmouserepeat",     kind: OK::SecondaryFlag, arity: EV_CC },
    Entry { op: 1414, name: "cc_setoninvtransmit",     mnemonic: "cc_setoninvtransmit",     kind: OK::SecondaryFlag, arity: EV_CC },
    Entry { op: 1415, name: "cc_setonstattransmit",    mnemonic: "cc_setonstattransmit",    kind: OK::SecondaryFlag, arity: EV_CC },
    Entry { op: 1416, name: "cc_setontargetenter",     mnemonic: "cc_setontargetenter",     kind: OK::SecondaryFlag, arity: EV_CC },
    Entry { op: 1417, name: "cc_setonscrollwheel",     mnemonic: "cc_setonscrollwheel",     kind: OK::SecondaryFlag, arity: EV_CC },
    Entry { op: 1418, name: "cc_setonchattransmit",    mnemonic: "cc_setonchattransmit",    kind: OK::SecondaryFlag, arity: EV_CC },
    Entry { op: 1419, name: "cc_setonkey",             mnemonic: "cc_setonkey",             kind: OK::SecondaryFlag, arity: EV_CC },
    Entry { op: 1420, name: "cc_setonfriendtransmit",  mnemonic: "cc_setonfriendtransmit",  kind: OK::SecondaryFlag, arity: EV_CC },
    Entry { op: 1421, name: "cc_setonclantransmit",    mnemonic: "cc_setonclantransmit",    kind: OK::SecondaryFlag, arity: EV_CC },
    Entry { op: 1422, name: "cc_setonmisctransmit",    mnemonic: "cc_setonmisctransmit",    kind: OK::SecondaryFlag, arity: EV_CC },
    Entry { op: 1423, name: "cc_setondialogabort",     mnemonic: "cc_setondialogabort",     kind: OK::SecondaryFlag, arity: EV_CC },
    Entry { op: 1424, name: "cc_setonsubchange",       mnemonic: "cc_setonsubchange",       kind: OK::SecondaryFlag, arity: EV_CC },

    // --- 1500..1802: cc_get* readers (active component) ---
    Entry { op: 1500, name: "cc_getx",            mnemonic: "cc_getx",            kind: OK::SecondaryFlag, arity: fx(0, 0, 1, 0) },
    Entry { op: 1501, name: "cc_gety",            mnemonic: "cc_gety",            kind: OK::SecondaryFlag, arity: fx(0, 0, 1, 0) },
    Entry { op: 1502, name: "cc_getwidth",        mnemonic: "cc_getwidth",        kind: OK::SecondaryFlag, arity: fx(0, 0, 1, 0) },
    Entry { op: 1503, name: "cc_getheight",       mnemonic: "cc_getheight",       kind: OK::SecondaryFlag, arity: fx(0, 0, 1, 0) },
    Entry { op: 1504, name: "cc_gethide",         mnemonic: "cc_gethide",         kind: OK::SecondaryFlag, arity: fx(0, 0, 1, 0) },
    Entry { op: 1505, name: "cc_getlayer",        mnemonic: "cc_getlayer",        kind: OK::SecondaryFlag, arity: fx(0, 0, 1, 0) },
    Entry { op: 1600, name: "cc_getscrollx",      mnemonic: "cc_getscrollx",      kind: OK::SecondaryFlag, arity: fx(0, 0, 1, 0) },
    Entry { op: 1601, name: "cc_getscrolly",      mnemonic: "cc_getscrolly",      kind: OK::SecondaryFlag, arity: fx(0, 0, 1, 0) },
    Entry { op: 1602, name: "cc_gettext",         mnemonic: "cc_gettext",         kind: OK::SecondaryFlag, arity: fx(0, 0, 0, 1) },
    Entry { op: 1603, name: "cc_getscrollwidth",  mnemonic: "cc_getscrollwidth",  kind: OK::SecondaryFlag, arity: fx(0, 0, 1, 0) },
    Entry { op: 1604, name: "cc_getscrollheight", mnemonic: "cc_getscrollheight", kind: OK::SecondaryFlag, arity: fx(0, 0, 1, 0) },
    Entry { op: 1605, name: "cc_getmodelzoom",    mnemonic: "cc_getmodelzoom",    kind: OK::SecondaryFlag, arity: fx(0, 0, 1, 0) },
    Entry { op: 1606, name: "cc_getmodelangle_x", mnemonic: "cc_getmodelangle_x", kind: OK::SecondaryFlag, arity: fx(0, 0, 1, 0) },
    Entry { op: 1607, name: "cc_getmodelangle_z", mnemonic: "cc_getmodelangle_z", kind: OK::SecondaryFlag, arity: fx(0, 0, 1, 0) },
    Entry { op: 1608, name: "cc_getmodelangle_y", mnemonic: "cc_getmodelangle_y", kind: OK::SecondaryFlag, arity: fx(0, 0, 1, 0) },
    Entry { op: 1700, name: "cc_getinvobject",    mnemonic: "cc_getinvobject",    kind: OK::SecondaryFlag, arity: fx(0, 0, 1, 0) },
    Entry { op: 1701, name: "cc_getinvcount",     mnemonic: "cc_getinvcount",     kind: OK::SecondaryFlag, arity: fx(0, 0, 1, 0) },
    Entry { op: 1702, name: "cc_getid",           mnemonic: "cc_getid",           kind: OK::SecondaryFlag, arity: fx(0, 0, 1, 0) },
    Entry { op: 1800, name: "cc_gettargetmask",   mnemonic: "cc_gettargetmask",   kind: OK::SecondaryFlag, arity: fx(0, 0, 1, 0) },
    Entry { op: 1801, name: "cc_getop",           mnemonic: "cc_getop",           kind: OK::SecondaryFlag, arity: fx(1, 0, 0, 1) },
    Entry { op: 1802, name: "cc_getopbase",       mnemonic: "cc_getopbase",       kind: OK::SecondaryFlag, arity: fx(0, 0, 0, 1) },

    // --- 2000..2424: if_set* mirrors — Java does `opcode -= 1000` and pops the
    // component id from the int stack (one extra int pop vs the cc_ twin). The 1-byte
    // operand still exists in the encoding but is ignored. ---
    Entry { op: 2000, name: "if_setposition", mnemonic: "if_setposition", kind: OK::SecondaryFlag, arity: fx(3, 0, 0, 0) },
    Entry { op: 2001, name: "if_setsize",     mnemonic: "if_setsize",     kind: OK::SecondaryFlag, arity: fx(3, 0, 0, 0) },
    Entry { op: 2003, name: "if_sethide",     mnemonic: "if_sethide",     kind: OK::SecondaryFlag, arity: fx(2, 0, 0, 0) },
    Entry { op: 2100, name: "if_setscrollpos",     mnemonic: "if_setscrollpos",     kind: OK::SecondaryFlag, arity: fx(3, 0, 0, 0) },
    Entry { op: 2101, name: "if_setcolour",        mnemonic: "if_setcolour",        kind: OK::SecondaryFlag, arity: fx(2, 0, 0, 0) },
    Entry { op: 2102, name: "if_setfill",          mnemonic: "if_setfill",          kind: OK::SecondaryFlag, arity: fx(2, 0, 0, 0) },
    Entry { op: 2103, name: "if_settrans",         mnemonic: "if_settrans",         kind: OK::SecondaryFlag, arity: fx(2, 0, 0, 0) },
    Entry { op: 2104, name: "if_setlinewid",       mnemonic: "if_setlinewid",       kind: OK::SecondaryFlag, arity: fx(2, 0, 0, 0) },
    Entry { op: 2105, name: "if_setgraphic",       mnemonic: "if_setgraphic",       kind: OK::SecondaryFlag, arity: fx(2, 0, 0, 0) },
    Entry { op: 2106, name: "if_set2dangle",       mnemonic: "if_set2dangle",       kind: OK::SecondaryFlag, arity: fx(2, 0, 0, 0) },
    Entry { op: 2107, name: "if_settiling",        mnemonic: "if_settiling",        kind: OK::SecondaryFlag, arity: fx(2, 0, 0, 0) },
    Entry { op: 2108, name: "if_setmodel",         mnemonic: "if_setmodel",         kind: OK::SecondaryFlag, arity: fx(2, 0, 0, 0) },
    Entry { op: 2109, name: "if_setmodelangle",    mnemonic: "if_setmodelangle",    kind: OK::SecondaryFlag, arity: fx(7, 0, 0, 0) },
    Entry { op: 2110, name: "if_setmodelanim",     mnemonic: "if_setmodelanim",     kind: OK::SecondaryFlag, arity: fx(2, 0, 0, 0) },
    Entry { op: 2111, name: "if_setmodelorthog",   mnemonic: "if_setmodelorthog",   kind: OK::SecondaryFlag, arity: fx(2, 0, 0, 0) },
    Entry { op: 2112, name: "if_settext",          mnemonic: "if_settext",          kind: OK::SecondaryFlag, arity: fx(1, 1, 0, 0) },
    Entry { op: 2113, name: "if_settextfont",      mnemonic: "if_settextfont",      kind: OK::SecondaryFlag, arity: fx(2, 0, 0, 0) },
    Entry { op: 2114, name: "if_settextalign",     mnemonic: "if_settextalign",     kind: OK::SecondaryFlag, arity: fx(4, 0, 0, 0) },
    Entry { op: 2115, name: "if_settextshadow",    mnemonic: "if_settextshadow",    kind: OK::SecondaryFlag, arity: fx(2, 0, 0, 0) },
    Entry { op: 2116, name: "if_setoutline",       mnemonic: "if_setoutline",       kind: OK::SecondaryFlag, arity: fx(2, 0, 0, 0) },
    Entry { op: 2117, name: "if_setgraphicshadow", mnemonic: "if_setgraphicshadow", kind: OK::SecondaryFlag, arity: fx(2, 0, 0, 0) },
    Entry { op: 2118, name: "if_setvflip",         mnemonic: "if_setvflip",         kind: OK::SecondaryFlag, arity: fx(2, 0, 0, 0) },
    Entry { op: 2119, name: "if_sethflip",         mnemonic: "if_sethflip",         kind: OK::SecondaryFlag, arity: fx(2, 0, 0, 0) },
    Entry { op: 2120, name: "if_setscrollsize",    mnemonic: "if_setscrollsize",    kind: OK::SecondaryFlag, arity: fx(3, 0, 0, 0) },
    Entry { op: 2200, name: "if_setobject",            mnemonic: "if_setobject",            kind: OK::SecondaryFlag, arity: fx(3, 0, 0, 0) },
    Entry { op: 2201, name: "if_setnpchead",           mnemonic: "if_setnpchead",           kind: OK::SecondaryFlag, arity: fx(2, 0, 0, 0) },
    Entry { op: 2202, name: "if_setplayerhead_self",   mnemonic: "if_setplayerhead_self",   kind: OK::SecondaryFlag, arity: fx(1, 0, 0, 0) },
    Entry { op: 2300, name: "if_setop",                mnemonic: "if_setop",                kind: OK::SecondaryFlag, arity: fx(2, 1, 0, 0) },
    Entry { op: 2301, name: "if_setdraggable",         mnemonic: "if_setdraggable",         kind: OK::SecondaryFlag, arity: fx(3, 0, 0, 0) },
    Entry { op: 2302, name: "if_setdraggablebehavior", mnemonic: "if_setdraggablebehavior", kind: OK::SecondaryFlag, arity: fx(2, 0, 0, 0) },
    Entry { op: 2303, name: "if_setdragdeadzone",      mnemonic: "if_setdragdeadzone",      kind: OK::SecondaryFlag, arity: fx(2, 0, 0, 0) },
    Entry { op: 2304, name: "if_setdragdeadtime",      mnemonic: "if_setdragdeadtime",      kind: OK::SecondaryFlag, arity: fx(2, 0, 0, 0) },
    Entry { op: 2305, name: "if_setopbase",            mnemonic: "if_setopbase",            kind: OK::SecondaryFlag, arity: fx(1, 1, 0, 0) },
    Entry { op: 2306, name: "if_settargetverb",        mnemonic: "if_settargetverb",        kind: OK::SecondaryFlag, arity: fx(1, 1, 0, 0) },
    Entry { op: 2307, name: "if_clearops",             mnemonic: "if_clearops",             kind: OK::SecondaryFlag, arity: fx(1, 0, 0, 0) },
    Entry { op: 2400, name: "if_setonclick",           mnemonic: "if_setonclick",           kind: OK::SecondaryFlag, arity: EV_IF },
    Entry { op: 2401, name: "if_setonhold",            mnemonic: "if_setonhold",            kind: OK::SecondaryFlag, arity: EV_IF },
    Entry { op: 2402, name: "if_setonrelease",         mnemonic: "if_setonrelease",         kind: OK::SecondaryFlag, arity: EV_IF },
    Entry { op: 2403, name: "if_setonmouseover",       mnemonic: "if_setonmouseover",       kind: OK::SecondaryFlag, arity: EV_IF },
    Entry { op: 2404, name: "if_setonmouseleave",      mnemonic: "if_setonmouseleave",      kind: OK::SecondaryFlag, arity: EV_IF },
    Entry { op: 2405, name: "if_setondrag",            mnemonic: "if_setondrag",            kind: OK::SecondaryFlag, arity: EV_IF },
    Entry { op: 2406, name: "if_setontargetleave",     mnemonic: "if_setontargetleave",     kind: OK::SecondaryFlag, arity: EV_IF },
    Entry { op: 2407, name: "if_setonvartransmit",     mnemonic: "if_setonvartransmit",     kind: OK::SecondaryFlag, arity: EV_IF },
    Entry { op: 2408, name: "if_setontimer",           mnemonic: "if_setontimer",           kind: OK::SecondaryFlag, arity: EV_IF },
    Entry { op: 2409, name: "if_setonop",              mnemonic: "if_setonop",              kind: OK::SecondaryFlag, arity: EV_IF },
    Entry { op: 2410, name: "if_setondragcomplete",    mnemonic: "if_setondragcomplete",    kind: OK::SecondaryFlag, arity: EV_IF },
    Entry { op: 2411, name: "if_setonclickrepeat",     mnemonic: "if_setonclickrepeat",     kind: OK::SecondaryFlag, arity: EV_IF },
    Entry { op: 2412, name: "if_setonmouserepeat",     mnemonic: "if_setonmouserepeat",     kind: OK::SecondaryFlag, arity: EV_IF },
    Entry { op: 2414, name: "if_setoninvtransmit",     mnemonic: "if_setoninvtransmit",     kind: OK::SecondaryFlag, arity: EV_IF },
    Entry { op: 2415, name: "if_setonstattransmit",    mnemonic: "if_setonstattransmit",    kind: OK::SecondaryFlag, arity: EV_IF },
    Entry { op: 2416, name: "if_setontargetenter",     mnemonic: "if_setontargetenter",     kind: OK::SecondaryFlag, arity: EV_IF },
    Entry { op: 2417, name: "if_setonscrollwheel",     mnemonic: "if_setonscrollwheel",     kind: OK::SecondaryFlag, arity: EV_IF },
    Entry { op: 2418, name: "if_setonchattransmit",    mnemonic: "if_setonchattransmit",    kind: OK::SecondaryFlag, arity: EV_IF },
    Entry { op: 2419, name: "if_setonkey",             mnemonic: "if_setonkey",             kind: OK::SecondaryFlag, arity: EV_IF },
    Entry { op: 2420, name: "if_setonfriendtransmit",  mnemonic: "if_setonfriendtransmit",  kind: OK::SecondaryFlag, arity: EV_IF },
    Entry { op: 2421, name: "if_setonclantransmit",    mnemonic: "if_setonclantransmit",    kind: OK::SecondaryFlag, arity: EV_IF },
    Entry { op: 2422, name: "if_setonmisctransmit",    mnemonic: "if_setonmisctransmit",    kind: OK::SecondaryFlag, arity: EV_IF },
    Entry { op: 2423, name: "if_setondialogabort",     mnemonic: "if_setondialogabort",     kind: OK::SecondaryFlag, arity: EV_IF },
    Entry { op: 2424, name: "if_setonsubchange",       mnemonic: "if_setonsubchange",       kind: OK::SecondaryFlag, arity: EV_IF },

    // --- 2500..2802: if_get* readers (component id on stack) ---
    Entry { op: 2500, name: "if_getx",            mnemonic: "if_getx",            kind: OK::SecondaryFlag, arity: fx(1, 0, 1, 0) },
    Entry { op: 2501, name: "if_gety",            mnemonic: "if_gety",            kind: OK::SecondaryFlag, arity: fx(1, 0, 1, 0) },
    Entry { op: 2502, name: "if_getwidth",        mnemonic: "if_getwidth",        kind: OK::SecondaryFlag, arity: fx(1, 0, 1, 0) },
    Entry { op: 2503, name: "if_getheight",       mnemonic: "if_getheight",       kind: OK::SecondaryFlag, arity: fx(1, 0, 1, 0) },
    Entry { op: 2504, name: "if_gethide",         mnemonic: "if_gethide",         kind: OK::SecondaryFlag, arity: fx(1, 0, 1, 0) },
    Entry { op: 2505, name: "if_getlayer",        mnemonic: "if_getlayer",        kind: OK::SecondaryFlag, arity: fx(1, 0, 1, 0) },
    Entry { op: 2600, name: "if_getscrollx",      mnemonic: "if_getscrollx",      kind: OK::SecondaryFlag, arity: fx(1, 0, 1, 0) },
    Entry { op: 2601, name: "if_getscrolly",      mnemonic: "if_getscrolly",      kind: OK::SecondaryFlag, arity: fx(1, 0, 1, 0) },
    Entry { op: 2602, name: "if_gettext",         mnemonic: "if_gettext",         kind: OK::SecondaryFlag, arity: fx(1, 0, 0, 1) },
    Entry { op: 2603, name: "if_getscrollwidth",  mnemonic: "if_getscrollwidth",  kind: OK::SecondaryFlag, arity: fx(1, 0, 1, 0) },
    Entry { op: 2604, name: "if_getscrollheight", mnemonic: "if_getscrollheight", kind: OK::SecondaryFlag, arity: fx(1, 0, 1, 0) },
    Entry { op: 2605, name: "if_getmodelzoom",    mnemonic: "if_getmodelzoom",    kind: OK::SecondaryFlag, arity: fx(1, 0, 1, 0) },
    Entry { op: 2606, name: "if_getmodelangle_x", mnemonic: "if_getmodelangle_x", kind: OK::SecondaryFlag, arity: fx(1, 0, 1, 0) },
    Entry { op: 2607, name: "if_getmodelangle_z", mnemonic: "if_getmodelangle_z", kind: OK::SecondaryFlag, arity: fx(1, 0, 1, 0) },
    Entry { op: 2608, name: "if_getmodelangle_y", mnemonic: "if_getmodelangle_y", kind: OK::SecondaryFlag, arity: fx(1, 0, 1, 0) },
    Entry { op: 2700, name: "if_getinvobject",    mnemonic: "if_getinvobject",    kind: OK::SecondaryFlag, arity: fx(1, 0, 1, 0) },
    Entry { op: 2701, name: "if_getinvcount",     mnemonic: "if_getinvcount",     kind: OK::SecondaryFlag, arity: fx(1, 0, 1, 0) },
    Entry { op: 2702, name: "if_hassub",          mnemonic: "if_hassub",          kind: OK::SecondaryFlag, arity: fx(1, 0, 1, 0) },
    Entry { op: 2800, name: "if_gettargetmask",   mnemonic: "if_gettargetmask",   kind: OK::SecondaryFlag, arity: fx(1, 0, 1, 0) },
    Entry { op: 2801, name: "if_getop",           mnemonic: "if_getop",           kind: OK::SecondaryFlag, arity: fx(2, 0, 0, 1) },
    Entry { op: 2802, name: "if_getopbase",       mnemonic: "if_getopbase",       kind: OK::SecondaryFlag, arity: fx(1, 0, 0, 1) },

    // --- 3100..3323: game/world state ---
    Entry { op: 3100, name: "mes",                 mnemonic: "mes",                 kind: OK::Filler, arity: fx(0, 1, 0, 0) },
    Entry { op: 3101, name: "anim",                mnemonic: "anim",                kind: OK::Filler, arity: fx(2, 0, 0, 0) },
    Entry { op: 3103, name: "if_close",            mnemonic: "if_close",            kind: OK::Filler, arity: fx(0, 0, 0, 0) },
    // pops the count as a *string* and parses it (ScriptRunner.java:1337-1351)
    Entry { op: 3104, name: "resume_countdialog",  mnemonic: "resume_countdialog",  kind: OK::Filler, arity: fx(0, 1, 0, 0) },
    Entry { op: 3105, name: "resume_namedialog",   mnemonic: "resume_namedialog",   kind: OK::Filler, arity: fx(0, 1, 0, 0) },
    Entry { op: 3106, name: "resume_stringdialog", mnemonic: "resume_stringdialog", kind: OK::Filler, arity: fx(0, 1, 0, 0) },
    Entry { op: 3107, name: "opplayer",            mnemonic: "opplayer",            kind: OK::Filler, arity: fx(1, 1, 0, 0) },
    Entry { op: 3108, name: "if_dragpickup",       mnemonic: "if_dragpickup",       kind: OK::Filler, arity: fx(3, 0, 0, 0) },
    Entry { op: 3109, name: "cc_dragpickup",       mnemonic: "cc_dragpickup",       kind: OK::SecondaryFlag, arity: fx(2, 0, 0, 0) },
    Entry { op: 3200, name: "sound_synth",         mnemonic: "sound_synth",         kind: OK::Filler, arity: fx(3, 0, 0, 0) },
    Entry { op: 3201, name: "sound_song",          mnemonic: "sound_song",          kind: OK::Filler, arity: fx(1, 0, 0, 0) },
    Entry { op: 3202, name: "sound_jingle",        mnemonic: "sound_jingle",        kind: OK::Filler, arity: fx(2, 0, 0, 0) },
    Entry { op: 3300, name: "clientclock",         mnemonic: "clientclock",         kind: OK::Filler, arity: fx(0, 0, 1, 0) },
    Entry { op: 3301, name: "inv_getobj",          mnemonic: "inv_getobj",          kind: OK::Filler, arity: fx(2, 0, 1, 0) },
    Entry { op: 3302, name: "inv_getnum",          mnemonic: "inv_getnum",          kind: OK::Filler, arity: fx(2, 0, 1, 0) },
    Entry { op: 3303, name: "inv_total",           mnemonic: "inv_total",           kind: OK::Filler, arity: fx(2, 0, 1, 0) },
    Entry { op: 3304, name: "inv_size",            mnemonic: "inv_size",            kind: OK::Filler, arity: fx(1, 0, 1, 0) },
    Entry { op: 3305, name: "stat",                mnemonic: "stat",                kind: OK::Filler, arity: fx(1, 0, 1, 0) },
    Entry { op: 3306, name: "stat_base",           mnemonic: "stat_base",           kind: OK::Filler, arity: fx(1, 0, 1, 0) },
    Entry { op: 3307, name: "stat_xp",             mnemonic: "stat_xp",             kind: OK::Filler, arity: fx(1, 0, 1, 0) },
    Entry { op: 3308, name: "coord",               mnemonic: "coord",               kind: OK::Filler, arity: fx(0, 0, 1, 0) },
    Entry { op: 3309, name: "coordx",              mnemonic: "coordx",              kind: OK::Filler, arity: fx(1, 0, 1, 0) },
    Entry { op: 3310, name: "coordy",              mnemonic: "coordy",              kind: OK::Filler, arity: fx(1, 0, 1, 0) },
    Entry { op: 3311, name: "coordz",              mnemonic: "coordz",              kind: OK::Filler, arity: fx(1, 0, 1, 0) },
    Entry { op: 3312, name: "map_members",         mnemonic: "map_members",         kind: OK::Filler, arity: fx(0, 0, 1, 0) },
    Entry { op: 3313, name: "invother_getobj",     mnemonic: "invother_getobj",     kind: OK::Filler, arity: fx(2, 0, 1, 0) },
    Entry { op: 3314, name: "invother_getnum",     mnemonic: "invother_getnum",     kind: OK::Filler, arity: fx(2, 0, 1, 0) },
    Entry { op: 3315, name: "invother_total",      mnemonic: "invother_total",      kind: OK::Filler, arity: fx(2, 0, 1, 0) },
    Entry { op: 3316, name: "staffmodlevel",       mnemonic: "staffmodlevel",       kind: OK::Filler, arity: fx(0, 0, 1, 0) },
    Entry { op: 3317, name: "reboottimer",         mnemonic: "reboottimer",         kind: OK::Filler, arity: fx(0, 0, 1, 0) },
    Entry { op: 3318, name: "map_world",           mnemonic: "map_world",           kind: OK::Filler, arity: fx(0, 0, 1, 0) },
    Entry { op: 3321, name: "runenergy_visible",   mnemonic: "runenergy_visible",   kind: OK::Filler, arity: fx(0, 0, 1, 0) },
    Entry { op: 3322, name: "runweight_visible",   mnemonic: "runweight_visible",   kind: OK::Filler, arity: fx(0, 0, 1, 0) },
    Entry { op: 3323, name: "playermod",           mnemonic: "playermod",           kind: OK::Filler, arity: fx(0, 0, 1, 0) },

    // --- 3400..3625: enums + friend/clan/ignore ---
    Entry { op: 3400, name: "enum_string",            mnemonic: "enum_string",            kind: OK::Filler, arity: fx(2, 0, 0, 1) },
    Entry { op: 3408, name: "enum",                   mnemonic: "enum",                   kind: OK::Filler, arity: Arity::EnumLookup },
    Entry { op: 3600, name: "friend_count",           mnemonic: "friend_count",           kind: OK::Filler, arity: fx(0, 0, 1, 0) },
    Entry { op: 3601, name: "friend_getname",         mnemonic: "friend_getname",         kind: OK::Filler, arity: fx(1, 0, 0, 1) },
    Entry { op: 3602, name: "friend_getworld",        mnemonic: "friend_getworld",        kind: OK::Filler, arity: fx(1, 0, 1, 0) },
    Entry { op: 3603, name: "friend_getrank",         mnemonic: "friend_getrank",         kind: OK::Filler, arity: fx(1, 0, 1, 0) },
    Entry { op: 3604, name: "friend_setrank",         mnemonic: "friend_setrank",         kind: OK::Filler, arity: fx(1, 1, 0, 0) },
    Entry { op: 3605, name: "friend_add",             mnemonic: "friend_add",             kind: OK::Filler, arity: fx(0, 1, 0, 0) },
    Entry { op: 3606, name: "friend_del",             mnemonic: "friend_del",             kind: OK::Filler, arity: fx(0, 1, 0, 0) },
    Entry { op: 3607, name: "ignore_add",             mnemonic: "ignore_add",             kind: OK::Filler, arity: fx(0, 1, 0, 0) },
    Entry { op: 3608, name: "ignore_del",             mnemonic: "ignore_del",             kind: OK::Filler, arity: fx(0, 1, 0, 0) },
    Entry { op: 3609, name: "friend_test",            mnemonic: "friend_test",            kind: OK::Filler, arity: fx(0, 1, 1, 0) },
    Entry { op: 3611, name: "clan_getchatdisplayname", mnemonic: "clan_getchatdisplayname", kind: OK::Filler, arity: fx(0, 0, 0, 1) },
    Entry { op: 3612, name: "clan_getchatcount",      mnemonic: "clan_getchatcount",      kind: OK::Filler, arity: fx(0, 0, 1, 0) },
    Entry { op: 3613, name: "clan_getchatusername",   mnemonic: "clan_getchatusername",   kind: OK::Filler, arity: fx(1, 0, 0, 1) },
    Entry { op: 3614, name: "clan_getchatuserworld",  mnemonic: "clan_getchatuserworld",  kind: OK::Filler, arity: fx(1, 0, 1, 0) },
    Entry { op: 3615, name: "clan_getchatuserrank",   mnemonic: "clan_getchatuserrank",   kind: OK::Filler, arity: fx(1, 0, 1, 0) },
    Entry { op: 3616, name: "clan_getchatminkick",    mnemonic: "clan_getchatminkick",    kind: OK::Filler, arity: fx(0, 0, 1, 0) },
    Entry { op: 3617, name: "clan_kickuser",          mnemonic: "clan_kickuser",          kind: OK::Filler, arity: fx(0, 1, 0, 0) },
    Entry { op: 3618, name: "clan_getchatrank",       mnemonic: "clan_getchatrank",       kind: OK::Filler, arity: fx(0, 0, 1, 0) },
    Entry { op: 3619, name: "clan_joinchat",          mnemonic: "clan_joinchat",          kind: OK::Filler, arity: fx(0, 1, 0, 0) },
    Entry { op: 3620, name: "clan_leavechat",         mnemonic: "clan_leavechat",         kind: OK::Filler, arity: fx(0, 0, 0, 0) },
    Entry { op: 3621, name: "ignore_count",           mnemonic: "ignore_count",           kind: OK::Filler, arity: fx(0, 0, 1, 0) },
    Entry { op: 3622, name: "ignore_getname",         mnemonic: "ignore_getname",         kind: OK::Filler, arity: fx(1, 0, 0, 1) },
    Entry { op: 3623, name: "ignore_test",            mnemonic: "ignore_test",            kind: OK::Filler, arity: fx(0, 1, 1, 0) },
    Entry { op: 3624, name: "clan_isself",            mnemonic: "clan_isself",            kind: OK::Filler, arity: fx(1, 0, 1, 0) },
    Entry { op: 3625, name: "clan_getchatownername",  mnemonic: "clan_getchatownername",  kind: OK::Filler, arity: fx(0, 0, 0, 1) },

    // --- 4000..4015: arithmetic / bitwise ---
    Entry { op: 4000, name: "add",          mnemonic: "add",          kind: OK::Filler, arity: fx(2, 0, 1, 0) },
    Entry { op: 4001, name: "sub",          mnemonic: "sub",          kind: OK::Filler, arity: fx(2, 0, 1, 0) },
    Entry { op: 4002, name: "multiply",     mnemonic: "multiply",     kind: OK::Filler, arity: fx(2, 0, 1, 0) },
    Entry { op: 4003, name: "divide",       mnemonic: "divide",       kind: OK::Filler, arity: fx(2, 0, 1, 0) },
    Entry { op: 4004, name: "random",       mnemonic: "random",       kind: OK::Filler, arity: fx(1, 0, 1, 0) },
    Entry { op: 4005, name: "randominc",    mnemonic: "randominc",    kind: OK::Filler, arity: fx(1, 0, 1, 0) },
    Entry { op: 4006, name: "interpolate",  mnemonic: "interpolate",  kind: OK::Filler, arity: fx(5, 0, 1, 0) },
    Entry { op: 4007, name: "addpercent",   mnemonic: "addpercent",   kind: OK::Filler, arity: fx(2, 0, 1, 0) },
    Entry { op: 4008, name: "setbit",       mnemonic: "setbit",       kind: OK::Filler, arity: fx(2, 0, 1, 0) },
    Entry { op: 4009, name: "clearbit",     mnemonic: "clearbit",     kind: OK::Filler, arity: fx(2, 0, 1, 0) },
    Entry { op: 4010, name: "testbit",      mnemonic: "testbit",      kind: OK::Filler, arity: fx(2, 0, 1, 0) },
    Entry { op: 4011, name: "modulo",       mnemonic: "modulo",       kind: OK::Filler, arity: fx(2, 0, 1, 0) },
    Entry { op: 4012, name: "pow",          mnemonic: "pow",          kind: OK::Filler, arity: fx(2, 0, 1, 0) },
    Entry { op: 4013, name: "invpow",       mnemonic: "invpow",       kind: OK::Filler, arity: fx(2, 0, 1, 0) },
    Entry { op: 4014, name: "and",          mnemonic: "and",          kind: OK::Filler, arity: fx(2, 0, 1, 0) },
    Entry { op: 4015, name: "or",           mnemonic: "or",           kind: OK::Filler, arity: fx(2, 0, 1, 0) },

    // --- 4100..4120: string ops ---
    Entry { op: 4100, name: "append_num",            mnemonic: "append_num",            kind: OK::Filler, arity: fx(1, 1, 0, 1) },
    Entry { op: 4101, name: "append",                mnemonic: "append",                kind: OK::Filler, arity: fx(0, 2, 0, 1) },
    Entry { op: 4102, name: "append_signnum",        mnemonic: "append_signnum",        kind: OK::Filler, arity: fx(1, 1, 0, 1) },
    Entry { op: 4103, name: "lowercase",             mnemonic: "lowercase",             kind: OK::Filler, arity: fx(0, 1, 0, 1) },
    Entry { op: 4104, name: "fromdate",              mnemonic: "fromdate",              kind: OK::Filler, arity: fx(1, 0, 0, 1) },
    Entry { op: 4105, name: "text_gender",           mnemonic: "text_gender",           kind: OK::Filler, arity: fx(0, 2, 0, 1) },
    Entry { op: 4106, name: "tostring",              mnemonic: "tostring",              kind: OK::Filler, arity: fx(1, 0, 0, 1) },
    Entry { op: 4107, name: "compare",               mnemonic: "compare",               kind: OK::Filler, arity: fx(0, 2, 1, 0) },
    Entry { op: 4108, name: "paraheight",            mnemonic: "paraheight",            kind: OK::Filler, arity: fx(2, 1, 1, 0) },
    Entry { op: 4109, name: "parawidth",             mnemonic: "parawidth",             kind: OK::Filler, arity: fx(2, 1, 1, 0) },
    Entry { op: 4110, name: "text_switch",           mnemonic: "text_switch",           kind: OK::Filler, arity: fx(1, 2, 0, 1) },
    Entry { op: 4111, name: "escape",                mnemonic: "escape",                kind: OK::Filler, arity: fx(0, 1, 0, 1) },
    Entry { op: 4112, name: "append_char",           mnemonic: "append_char",           kind: OK::Filler, arity: fx(1, 1, 0, 1) },
    Entry { op: 4113, name: "char_isprintable",      mnemonic: "char_isprintable",      kind: OK::Filler, arity: fx(1, 0, 1, 0) },
    Entry { op: 4114, name: "char_isalphanumeric",   mnemonic: "char_isalphanumeric",   kind: OK::Filler, arity: fx(1, 0, 1, 0) },
    Entry { op: 4115, name: "char_isalpha",          mnemonic: "char_isalpha",          kind: OK::Filler, arity: fx(1, 0, 1, 0) },
    Entry { op: 4116, name: "char_isnumeric",        mnemonic: "char_isnumeric",        kind: OK::Filler, arity: fx(1, 0, 1, 0) },
    Entry { op: 4117, name: "string_length",         mnemonic: "string_length",         kind: OK::Filler, arity: fx(0, 1, 1, 0) },
    Entry { op: 4118, name: "substring",             mnemonic: "substring",             kind: OK::Filler, arity: fx(2, 1, 0, 1) },
    Entry { op: 4119, name: "removetags",            mnemonic: "removetags",            kind: OK::Filler, arity: fx(0, 1, 0, 1) },
    Entry { op: 4120, name: "string_indexof_char",   mnemonic: "string_indexof_char",   kind: OK::Filler, arity: fx(1, 1, 1, 0) },

    // --- 4200..4207: obj-type accessors ---
    Entry { op: 4200, name: "oc_name",      mnemonic: "oc_name",      kind: OK::Filler, arity: fx(1, 0, 0, 1) },
    Entry { op: 4201, name: "oc_op",        mnemonic: "oc_op",        kind: OK::Filler, arity: fx(2, 0, 0, 1) },
    Entry { op: 4202, name: "oc_iop",       mnemonic: "oc_iop",       kind: OK::Filler, arity: fx(2, 0, 0, 1) },
    Entry { op: 4203, name: "oc_cost",      mnemonic: "oc_cost",      kind: OK::Filler, arity: fx(1, 0, 1, 0) },
    Entry { op: 4204, name: "oc_stackable", mnemonic: "oc_stackable", kind: OK::Filler, arity: fx(1, 0, 1, 0) },
    Entry { op: 4205, name: "oc_cert",      mnemonic: "oc_cert",      kind: OK::Filler, arity: fx(1, 0, 1, 0) },
    Entry { op: 4206, name: "oc_uncert",    mnemonic: "oc_uncert",    kind: OK::Filler, arity: fx(1, 0, 1, 0) },
    Entry { op: 4207, name: "oc_members",   mnemonic: "oc_members",   kind: OK::Filler, arity: fx(1, 0, 1, 0) },

    // --- 5000..5017: chat / social ---
    Entry { op: 5000, name: "chat_getfilter_public",          mnemonic: "chat_getfilter_public",         kind: OK::Filler, arity: fx(0, 0, 1, 0) },
    Entry { op: 5001, name: "chat_setfilter",                 mnemonic: "chat_setfilter",                kind: OK::Filler, arity: fx(3, 0, 0, 0) },
    Entry { op: 5002, name: "chat_sendabusereport",           mnemonic: "chat_sendabusereport",          kind: OK::Filler, arity: fx(2, 1, 0, 0) },
    Entry { op: 5003, name: "chat_gethistory_bytypeandline",  mnemonic: "chat_gethistory_bytypeandline", kind: OK::Filler, arity: fx(1, 0, 0, 1) },
    Entry { op: 5004, name: "chat_gethistory_byuid",          mnemonic: "chat_gethistory_byuid",         kind: OK::Filler, arity: fx(1, 0, 1, 0) },
    Entry { op: 5005, name: "chat_getfilter_private",         mnemonic: "chat_getfilter_private",        kind: OK::Filler, arity: fx(0, 0, 1, 0) },
    Entry { op: 5008, name: "chat_sendpublic",                mnemonic: "chat_sendpublic",               kind: OK::Filler, arity: fx(0, 1, 0, 0) },
    Entry { op: 5009, name: "chat_sendprivate",               mnemonic: "chat_sendprivate",              kind: OK::Filler, arity: fx(0, 2, 0, 0) },
    Entry { op: 5010, name: "chat_sendclan",                  mnemonic: "chat_sendclan",                 kind: OK::Filler, arity: fx(1, 0, 0, 1) },
    Entry { op: 5011, name: "chat_getscreenname",             mnemonic: "chat_getscreenname",            kind: OK::Filler, arity: fx(1, 0, 0, 1) },
    Entry { op: 5015, name: "chat_playername",                mnemonic: "chat_playername",               kind: OK::Filler, arity: fx(0, 0, 0, 1) },
    Entry { op: 5016, name: "chat_getfilter_trade",           mnemonic: "chat_getfilter_trade",          kind: OK::Filler, arity: fx(0, 0, 1, 0) },
    Entry { op: 5017, name: "chat_gethistorylength",          mnemonic: "chat_gethistorylength",         kind: OK::Filler, arity: fx(0, 0, 1, 0) },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opcode_lookup_finds_known_entries() {
        assert_eq!(opcode_name(0), Some("push_constant_int"));
        assert_eq!(mnemonic(0), "push");
        assert_eq!(operand_kind(0), OperandKind::Int);
        assert_eq!(stack_delta(0), Some((1, 0)));

        assert_eq!(mnemonic(33), "push");
        assert_eq!(operand_kind(33), OperandKind::LocalInt);

        assert_eq!(mnemonic(4000), "add");
        assert_eq!(operand_kind(4000), OperandKind::Filler);
        assert_eq!(stack_delta(4000), Some((-1, 0)));
    }

    #[test]
    fn unknown_opcode_returns_defaults() {
        assert_eq!(opcode_name(9999), None);
        assert_eq!(mnemonic(9999), "?");
        assert_eq!(operand_kind(9999), OperandKind::Int);
        assert_eq!(arity(9999), None);
        assert_eq!(stack_delta(9999), None);
    }

    #[test]
    fn branch_classification() {
        assert!(is_branch(6));
        assert!(is_branch(8));
        assert!(is_branch(32));
        assert!(!is_branch(0));
        assert!(!is_branch(40));
        assert!(is_unconditional_branch(6));
        assert!(!is_unconditional_branch(8));
        assert!(is_conditional_branch(8));
        assert!(!is_conditional_branch(6));
    }

    #[test]
    fn branch_keyword_table() {
        assert_eq!(branch_keyword(8), "eq");
        assert_eq!(branch_keyword(7), "neq");
        assert_eq!(branch_keyword(9), "lt");
        assert_eq!(branch_keyword(10), "gt");
        assert_eq!(branch_keyword(31), "le");
        assert_eq!(branch_keyword(32), "ge");
        assert_eq!(branch_keyword(6), ""); // not conditional
        assert_eq!(branch_keyword(0), "");
    }

    #[test]
    fn dynamic_arity_ops_have_no_static_delta() {
        for op in [21u16, 37, 40, 1400, 2400, 3408] {
            assert_eq!(stack_delta(op), None, "op {op} should be dynamic");
            assert!(arity(op).is_some(), "op {op} must still be in the table");
        }
    }

    /// Every `if_*` mirror (2000..2424) must pop exactly one more int than its
    /// `cc_*` twin (op - 1000) and otherwise match — that's literally how the Java
    /// dispatcher implements them (`opcode -= 1000` after popping the component id).
    #[test]
    fn if_mirrors_are_cc_plus_one_component_pop() {
        for e in OPCODES {
            if !(2000..2500).contains(&e.op) {
                continue;
            }
            let twin = arity(e.op - 1000).expect("every if_* mirror has a cc_ twin");
            match (e.arity, twin) {
                (
                    Arity::Fixed { ipop, spop, ipush, spush },
                    Arity::Fixed { ipop: ti, spop: ts, ipush: tpi, spush: tps },
                ) => {
                    assert_eq!(ipop, ti + 1, "op {}: int pops", e.op);
                    assert_eq!(spop, ts, "op {}: str pops", e.op);
                    assert_eq!(ipush, tpi, "op {}: int pushes", e.op);
                    assert_eq!(spush, tps, "op {}: str pushes", e.op);
                }
                (
                    Arity::EventHandler { component_from_stack: true },
                    Arity::EventHandler { component_from_stack: false },
                ) => {}
                (a, b) => panic!("op {}: mismatched arity kinds {a:?} vs {b:?}", e.op),
            }
        }
    }

    #[test]
    fn no_duplicate_opcodes_in_table() {
        let mut seen = std::collections::HashSet::new();
        for e in OPCODES {
            assert!(seen.insert(e.op), "duplicate table entry for op {}", e.op);
        }
    }
}
