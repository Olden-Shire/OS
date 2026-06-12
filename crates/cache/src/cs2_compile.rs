//! CS2 recompiler — structured [`crate::cs2_ir`] → bytecode.
//!
//! The exact inverse of [`crate::cs2_decompile`]: every IR construct has one canonical
//! lowering, chosen to match what the Jagex compiler emitted (verified byte-for-byte
//! over the whole cache by `tests/cs2_recompile.rs`). Conditional shapes:
//!
//! ```text
//! cmp (full form):    lhs; rhs; cond_op → T;  branch → F
//! a && b:             emit a with T = next instruction (the inverted-branch pair),
//!                     then b in full form
//! a || b:             emit a as a fallthrough link (cond_op → T, no branch),
//!                     then b in full form
//! if:                 cond(T=then, F=end);    then...
//! if/else:            cond(T=then, F=else);   then...; branch → end; else...
//! while:              top: cond(T=body, F=end); body...; branch → top
//! ```
//!
//! Constructs that share exit labels in the original (e.g. a `while` as the last
//! statement of a then-block reusing the `if`'s false-target) need no special casing —
//! the labels simply resolve to the same pc.

use crate::cs2::ClientScript;
use crate::cs2_ir::{Cond, Expr, ScriptIr, Stmt, Target};

/// Codegen failure — only possible for IR shapes the lifter never produces (it exists
/// so hand-written or future-extended IR fails loudly instead of emitting bad bytes).
#[derive(Debug, Clone)]
pub struct CompileError(pub String);

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Clone, Copy)]
struct Label(usize);

#[derive(Default)]
struct Emitter {
    ops: Vec<u16>,
    iops: Vec<i32>,
    sops: Vec<String>,
    /// `(pc, label)` pairs whose int operand becomes `label_pc - pc - 1`.
    patches: Vec<(usize, usize)>,
    labels: Vec<Option<usize>>,
}

impl Emitter {
    fn emit(&mut self, op: u16, iop: i32) {
        self.ops.push(op);
        self.iops.push(iop);
        self.sops.push(String::new());
    }

    fn emit_str(&mut self, op: u16, s: &str) {
        self.ops.push(op);
        self.iops.push(0);
        self.sops.push(s.to_owned());
    }

    fn label(&mut self) -> Label {
        self.labels.push(None);
        Label(self.labels.len() - 1)
    }

    fn place(&mut self, l: Label) {
        debug_assert!(self.labels[l.0].is_none(), "label placed twice");
        self.labels[l.0] = Some(self.ops.len());
    }

    fn branch(&mut self, op: u16, l: Label) {
        self.patches.push((self.ops.len(), l.0));
        self.emit(op, 0);
    }

    fn finish(mut self, ir: &ScriptIr) -> Result<ClientScript, CompileError> {
        for (pc, label) in &self.patches {
            let Some(target) = self.labels[*label] else {
                return Err(CompileError(format!("unplaced label at pc {pc}")));
            };
            self.iops[*pc] = target as i32 - *pc as i32 - 1;
        }
        Ok(ClientScript {
            name: ir.name.clone(),
            instructions: self.ops,
            int_operands: self.iops,
            string_operands: self.sops,
            int_local_count: ir.int_locals,
            string_local_count: ir.str_locals,
            int_arg_count: ir.int_args,
            string_arg_count: ir.str_args,
        })
    }
}

/// Compile lifted IR back to a [`ClientScript`]. For IR produced by
/// [`crate::cs2_decompile::lift`], `compile(ir).encode()` reproduces the original
/// group bytes exactly.
pub fn compile(ir: &ScriptIr) -> Result<ClientScript, CompileError> {
    let mut e = Emitter::default();
    emit_stmts(&mut e, &ir.body)?;
    e.finish(ir)
}

fn emit_stmts(e: &mut Emitter, body: &[Stmt]) -> Result<(), CompileError> {
    for stmt in body {
        emit_stmt(e, stmt)?;
    }
    Ok(())
}

fn emit_stmt(e: &mut Emitter, stmt: &Stmt) -> Result<(), CompileError> {
    match stmt {
        Stmt::Assign { targets, value } => {
            // Array stores evaluate their index before the value.
            if let [Target::Array { array, index }] = targets.as_slice() {
                emit_expr(e, index)?;
                emit_expr(e, value)?;
                e.emit(46, i32::from(*array));
                return Ok(());
            }
            emit_expr(e, value)?;
            for t in targets.iter().rev() {
                match t {
                    Target::LocalInt(n) => e.emit(34, i32::from(*n)),
                    Target::LocalStr(n) => e.emit(36, i32::from(*n)),
                    Target::Varp(n) => e.emit(2, i32::from(*n)),
                    Target::Varbit(n) => e.emit(27, i32::from(*n)),
                    Target::VarcInt(n) => e.emit(43, i32::from(*n)),
                    Target::VarcStr(n) => e.emit(48, i32::from(*n)),
                    Target::DiscardInt => e.emit(38, 0),
                    Target::DiscardStr => e.emit(39, 0),
                    Target::Array { .. } => {
                        return Err(CompileError("array store in multi-assignment".into()));
                    }
                }
            }
        }
        Stmt::Eval(expr) => emit_expr(e, expr)?,
        Stmt::DefineArray { array, elem_type, len } => {
            emit_expr(e, len)?;
            e.emit(44, (i32::from(*array) << 16) | i32::from(*elem_type));
        }
        Stmt::Return(exprs) => {
            for x in exprs {
                emit_expr(e, x)?;
            }
            e.emit(21, 0);
        }
        Stmt::If { cond, then_body, else_body } => {
            let l_then = e.label();
            let l_false = e.label();
            emit_cond(e, cond, l_then, l_false)?;
            e.place(l_then);
            emit_stmts(e, then_body)?;
            if else_body.is_empty() {
                e.place(l_false);
            } else {
                let l_end = e.label();
                e.branch(6, l_end);
                e.place(l_false);
                emit_stmts(e, else_body)?;
                e.place(l_end);
            }
        }
        Stmt::While { cond, body } => {
            let l_top = e.label();
            let l_body = e.label();
            let l_end = e.label();
            e.place(l_top);
            emit_cond(e, cond, l_body, l_end)?;
            e.place(l_body);
            emit_stmts(e, body)?;
            e.branch(6, l_top);
            e.place(l_end);
        }
    }
    Ok(())
}

/// Full-form condition: control reaches `t` when true, `f` when false.
fn emit_cond(e: &mut Emitter, cond: &Cond, t: Label, f: Label) -> Result<(), CompileError> {
    match cond {
        Cond::Cmp { op, lhs, rhs } => {
            emit_expr(e, lhs)?;
            emit_expr(e, rhs)?;
            e.branch(*op, t);
            e.branch(6, f);
        }
        Cond::And(a, b) => {
            // Left side targets the instruction right after its own group — the
            // observed "cond → next; branch → F" inverted pair.
            let l_next = e.label();
            emit_cond(e, a, l_next, f)?;
            e.place(l_next);
            emit_cond(e, b, t, f)?;
        }
        Cond::Or(a, b) => {
            emit_or_link(e, a, t)?;
            emit_cond(e, b, t, f)?;
        }
    }
    Ok(())
}

/// Fallthrough link: jump to `t` when true, fall through when false (no trailing
/// branch). Only comparisons and nested ORs can sit in this position — the lifter
/// never puts an AND group here.
fn emit_or_link(e: &mut Emitter, cond: &Cond, t: Label) -> Result<(), CompileError> {
    match cond {
        Cond::Cmp { op, lhs, rhs } => {
            emit_expr(e, lhs)?;
            emit_expr(e, rhs)?;
            e.branch(*op, t);
            Ok(())
        }
        Cond::Or(a, b) => {
            emit_or_link(e, a, t)?;
            emit_or_link(e, b, t)
        }
        Cond::And(..) => Err(CompileError("AND group in fallthrough condition position".into())),
    }
}

fn emit_expr(e: &mut Emitter, expr: &Expr) -> Result<(), CompileError> {
    match expr {
        Expr::ConstInt(v) => e.emit(0, *v),
        Expr::ConstStr(s) => e.emit_str(3, s),
        Expr::LocalInt(n) => e.emit(33, i32::from(*n)),
        Expr::LocalStr(n) => e.emit(35, i32::from(*n)),
        Expr::Varp(n) => e.emit(1, i32::from(*n)),
        Expr::Varbit(n) => e.emit(25, i32::from(*n)),
        Expr::VarcInt(n) => e.emit(42, i32::from(*n)),
        Expr::VarcStr(n) => e.emit(47, i32::from(*n)),
        Expr::ArrayLoad { array, index } => {
            emit_expr(e, index)?;
            e.emit(45, i32::from(*array));
        }
        Expr::Join(parts) => {
            for p in parts {
                emit_expr(e, p)?;
            }
            e.emit(37, parts.len() as i32);
        }
        Expr::Gosub { script, args } => {
            for a in args {
                emit_expr(e, a)?;
            }
            e.emit(40, *script as i32);
        }
        Expr::Command { op, flag, args } => {
            for a in args {
                emit_expr(e, a)?;
            }
            e.emit(*op, i32::from(*flag));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cs2_decompile::lift;
    use crate::cs2_sig::analyze_all;
    use std::collections::BTreeMap;

    /// lift → compile must reproduce the exact instruction stream for a structured
    /// script (full-cache proof lives in tests/cs2_recompile.rs).
    #[test]
    fn round_trips_if_else_while() {
        let s = ClientScript {
            name: None,
            // while ($i0 < 10) { if ($i0 = 5) { $i1 = 1 } else { $i1 = 2 } $i0 += 1 } return
            instructions: vec![
                33, 0, 9, 6, // while header
                33, 0, 8, 6, // if header
                0, 34, 6, // then + skip
                0, 34, // else
                33, 0, 4000, 34, // increment
                6,  // loop back
                21, // return
            ],
            int_operands: vec![
                0, 10, 1, 14, // while: lt → +1, jump → end(19)
                0, 5, 1, 3, // if: eq → +1, jump → else(12)
                1, 1, 2, // $i1 = 1; jump → +2 (14)
                2, 1, // $i1 = 2
                0, 1, 0, 0, // $i0 = add($i0, 1)
                -18, // back to 0
                0,
            ],
            string_operands: vec![String::new(); 20],
            int_local_count: 2,
            string_local_count: 0,
            int_arg_count: 0,
            string_arg_count: 0,
        };
        let mut all = BTreeMap::new();
        all.insert(0u32, s);
        let a = analyze_all(&all);
        assert!(a.diags.is_empty(), "{:?}", a.diags);
        let ir = lift(0, &all[&0], &a.sigs).expect("lift");
        let back = compile(&ir).expect("compile");
        assert_eq!(back.instructions, all[&0].instructions);
        assert_eq!(back.int_operands, all[&0].int_operands);
        assert_eq!(back.encode(), all[&0].encode());
    }
}
