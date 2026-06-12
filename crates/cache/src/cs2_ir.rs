//! Decompiled CS2 IR — expression trees and structured statements.
//!
//! Produced by [`crate::cs2_decompile`] from raw [`crate::cs2::ClientScript`] bytecode,
//! consumed by the source printer and (in reverse) the recompiler. Every construct here
//! has exactly **one** bytecode lowering, and the lifter only produces a construct when
//! the bytecode matches that lowering — that invariant is what makes
//! decompile → recompile byte-exact.
//!
//! Ordering conventions (these encode the original instruction order, so the
//! recompiler can reproduce it):
//!
//! - Call/return argument lists are in **evaluation order** (the order the args' root
//!   pushes appeared in the bytecode), even when int and string args interleave across
//!   the two stacks.
//! - [`Stmt::Assign`] `targets` are in **source order** (`$a, $b = ~foo`); the
//!   bytecode pops them in reverse, and codegen re-reverses.

/// An expression — something that leaves exactly one value on one of the two stacks
/// (except [`Expr::Command`]/[`Expr::Gosub`] used as statements or multi-assign
/// sources, which may push 0 or several).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    /// op 0 `push_constant_int`.
    ConstInt(i32),
    /// op 3 `push_constant_string`.
    ConstStr(String),
    /// op 33 `push_int_local`.
    LocalInt(u16),
    /// op 35 `push_string_local`.
    LocalStr(u16),
    /// op 1 `push_varp`.
    Varp(u16),
    /// op 25 `push_varbit`.
    Varbit(u16),
    /// op 42 `push_varc_int`.
    VarcInt(u16),
    /// op 47 `push_varc_str`.
    VarcStr(u16),
    /// op 45 `push_array_int` — operand is the array id, index comes off the stack.
    ArrayLoad { array: u8, index: Box<Expr> },
    /// op 37 `join_string` — operand is `parts.len()`.
    Join(Vec<Expr>),
    /// op 40 `gosub_with_params` — args in evaluation order.
    Gosub { script: u32, args: Vec<Expr> },
    /// Any other opcode (native command). `flag` is the 1-byte secondary-component
    /// operand carried by cc_/if_ ops (rendered as a `.` prefix); always false for ops
    /// whose operand byte is filler. Args in evaluation order — for the event-handler
    /// installers this includes the descriptor string and any `Y` trigger list exactly
    /// as pushed.
    Command { op: u16, flag: bool, args: Vec<Expr> },
}

/// Assignment destination (the sink op that consumed a value).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// op 34 `pop_int_local`.
    LocalInt(u16),
    /// op 36 `pop_string_local`.
    LocalStr(u16),
    /// op 2 `pop_varp`.
    Varp(u16),
    /// op 27 `pop_varbit`.
    Varbit(u16),
    /// op 43 `pop_varc_int`.
    VarcInt(u16),
    /// op 48 `pop_varc_str`.
    VarcStr(u16),
    /// op 46 `pop_array_int` — index evaluated before the value.
    Array { array: u8, index: Expr },
    /// op 38 `pop_int_discard` — calling an int-returning script/command as a
    /// statement; the printer hides it and codegen re-derives it from the value type.
    DiscardInt,
    /// op 39 `pop_string_discard`.
    DiscardStr,
}

/// A branch condition. `Cmp.op` is the original conditional-branch opcode
/// (7/8/9/10/31/32) with `lhs` pushed first, exactly as `ScriptRunner` compares them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cond {
    Cmp { op: u16, lhs: Expr, rhs: Expr },
    /// Short-circuit AND chain (canonical form — nested single-arm `if`s that compile
    /// to identical bytecode are also rendered this way).
    And(Box<Cond>, Box<Cond>),
    /// Short-circuit OR chain.
    Or(Box<Cond>, Box<Cond>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stmt {
    /// `targets = value` — one target for plain assignments, several when `value` is a
    /// multi-return [`Expr::Gosub`]. Targets in source order.
    Assign { targets: Vec<Target>, value: Expr },
    /// A command/gosub that pushes nothing, evaluated for effect.
    Eval(Expr),
    /// op 44 `define_array` — operand packs `(array_id << 16) | type_char`.
    DefineArray { array: u8, elem_type: u8, len: Expr },
    /// op 21 — return values in evaluation order.
    Return(Vec<Expr>),
    If { cond: Cond, then_body: Vec<Stmt>, else_body: Vec<Stmt> },
    While { cond: Cond, body: Vec<Stmt> },
}

/// A fully lifted script.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptIr {
    pub id: u32,
    pub name: Option<String>,
    pub int_args: u16,
    pub str_args: u16,
    pub int_locals: u16,
    pub str_locals: u16,
    pub int_returns: u16,
    pub str_returns: u16,
    pub body: Vec<Stmt>,
}
