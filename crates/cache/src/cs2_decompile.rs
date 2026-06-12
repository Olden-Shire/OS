//! CS2 lifter — bytecode → structured [`crate::cs2_ir`].
//!
//! Two phases, both *strict*: any shape outside the canonical patterns the Jagex
//! compiler emits is an error, and the caller falls back to `.cs2asm` disassembly for
//! that script. Never guess — byte-exact recompilation depends on only accepting
//! bytecode we know how to regenerate.
//!
//! **Phase 1 — linearize.** A single forward pass with a symbolic stack per Java
//! stack. Pushes build [`Expr`] trees; ops with known arity (see
//! [`crate::cs2_opcodes::Arity`]) fold their operands into calls; sink ops
//! (`pop_*`) accumulate; whenever both stacks return to empty a complete statement is
//! emitted. Branches and returns must occur at empty-stack points (the compiler always
//! evaluates a full condition right before its branch). Multi-return gosubs push
//! sentinels that only sinks may consume (`$a, $b = ~script(...)`).
//!
//! **Phase 2 — structure.** Branch targets must land on statement boundaries. The
//! canonical shapes (verified empirically over the whole cache):
//!
//! ```text
//! if:        cond → T;  jump → F;  T: then...;            F:
//! if/else:   cond → T;  jump → F;  T: then...; jump → E;  F: else...;  E:
//! while:     C: cond → T;  jump → F;  T: body...; jump → C;  F:
//! a && b:    cond a → next;  jump → F;  next: cond b → T;  jump → F
//! a || b:    cond a → T;     jump → next;  next: cond b → T;  jump → F
//! ```
//!
//! Nested single-arm `if`s compile to bytecode identical to `&&`, so the lifter
//! canonicalises to `&&` — the recompiler emits that same shape, keeping the round
//! trip exact.

use std::collections::BTreeMap;

use crate::cs2::ClientScript;
use crate::cs2_ir::{Cond, Expr, ScriptIr, Stmt, Target};
use crate::cs2_opcodes::{arity, is_conditional_branch, is_unconditional_branch, opcode_name, Arity};
use crate::cs2_sig::ScriptSig;

/// Why a script couldn't be lifted. The packer treats this as "emit `.cs2asm`".
#[derive(Debug, Clone)]
pub struct LiftError {
    pub pc: Option<usize>,
    pub message: String,
}

impl std::fmt::Display for LiftError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.pc {
            Some(pc) => write!(f, "pc {}: {}", pc, self.message),
            None => write!(f, "{}", self.message),
        }
    }
}

fn err<T>(pc: usize, message: impl Into<String>) -> Result<T, LiftError> {
    Err(LiftError { pc: Some(pc), message: message.into() })
}

/// Lift one script to structured IR. `sigs` must cover every gosub callee
/// (use [`crate::cs2_sig::analyze_all`] over the whole archive first).
pub fn lift(
    id: u32,
    s: &ClientScript,
    sigs: &BTreeMap<u32, ScriptSig>,
) -> Result<ScriptIr, LiftError> {
    let items = linearize(id, s, sigs)?;
    let mut pc_to_idx = BTreeMap::new();
    for (i, item) in items.iter().enumerate() {
        pc_to_idx.insert(item.start_pc, i);
    }
    let body = structure(&items, 0, items.len(), &pc_to_idx)?;

    let sig = sigs.get(&id).copied().unwrap_or(ScriptSig {
        int_args: s.int_arg_count,
        str_args: s.string_arg_count,
        int_returns: 0,
        str_returns: 0,
    });
    Ok(ScriptIr {
        id,
        name: s.name.clone(),
        int_args: s.int_arg_count,
        str_args: s.string_arg_count,
        int_locals: s.int_local_count,
        str_locals: s.string_local_count,
        int_returns: sig.int_returns,
        str_returns: sig.str_returns,
        body,
    })
}

// ---------------------------------------------------------------------------------
// Phase 1: linearize
// ---------------------------------------------------------------------------------

/// One statement-level unit, starting at bytecode pc `start_pc` (branch targets must
/// land exactly on these).
#[derive(Debug)]
struct ItemBuf {
    start_pc: usize,
    item: Item,
}

#[derive(Debug)]
enum Item {
    Stmt(Stmt),
    /// Conditional branch closing a condition region. `lhs` was pushed first.
    CondJump { op: u16, lhs: Expr, rhs: Expr, target_pc: usize },
    Jump { target_pc: usize },
}

/// Symbolic stack slot.
#[derive(Debug, Clone)]
enum Entry {
    Plain { expr: Expr, seq: usize },
    /// One value of a multi-push call parked in `slots`.
    Multi { slot: usize, seq: usize },
}

impl Entry {
    fn seq(&self) -> usize {
        match self {
            Entry::Plain { seq, .. } | Entry::Multi { seq, .. } => *seq,
        }
    }
}

#[derive(Debug)]
struct MultiSlot {
    call: Expr,
    pushes: usize,
}

struct Linearizer {
    ints: Vec<Entry>,
    strs: Vec<Entry>,
    /// `(target, consumed entry, sink pc)` in pop order, awaiting empty-stack flush.
    pending: Vec<(Target, Entry)>,
    slots: Vec<MultiSlot>,
    items: Vec<ItemBuf>,
    /// pc of the first instruction of the in-progress region.
    cur_start: Option<usize>,
}

impl Linearizer {
    fn depth0(&self) -> bool {
        self.ints.is_empty() && self.strs.is_empty()
    }

    fn pop_int(&mut self, pc: usize, what: &str) -> Result<Entry, LiftError> {
        self.ints.pop().map_or_else(|| err(pc, format!("{what}: int stack underflow")), Ok)
    }

    fn pop_str(&mut self, pc: usize, what: &str) -> Result<Entry, LiftError> {
        self.strs.pop().map_or_else(|| err(pc, format!("{what}: string stack underflow")), Ok)
    }

    fn pop_plain_int(&mut self, pc: usize, what: &str) -> Result<Expr, LiftError> {
        match self.pop_int(pc, what)? {
            Entry::Plain { expr, .. } => Ok(expr),
            Entry::Multi { .. } => err(pc, format!("{what}: multi-return value used in expression")),
        }
    }

    /// Pop `ni` ints + `ns` strings and merge them into evaluation (push) order.
    fn pop_args(&mut self, pc: usize, what: &str, ni: usize, ns: usize) -> Result<Vec<Expr>, LiftError> {
        if self.ints.len() < ni {
            return err(pc, format!("{what}: int stack underflow (need {ni}, have {})", self.ints.len()));
        }
        if self.strs.len() < ns {
            return err(pc, format!("{what}: string stack underflow (need {ns}, have {})", self.strs.len()));
        }
        let mut taken: Vec<Entry> = Vec::with_capacity(ni + ns);
        taken.extend(self.ints.drain(self.ints.len() - ni..));
        taken.extend(self.strs.drain(self.strs.len() - ns..));
        taken.sort_by_key(Entry::seq);
        let mut out = Vec::with_capacity(taken.len());
        for e in taken {
            match e {
                Entry::Plain { expr, .. } => out.push(expr),
                Entry::Multi { .. } => {
                    return err(pc, format!("{what}: multi-return value used in expression"));
                }
            }
        }
        Ok(out)
    }

    /// A call produced `expr` pushing `ipush` ints + `spush` strings.
    fn push_result(&mut self, pc: usize, expr: Expr, ipush: usize, spush: usize) -> Result<(), LiftError> {
        match ipush + spush {
            0 => {
                if !self.depth0() {
                    return err(pc, "void call with values still on the stack");
                }
                self.flush_stmt(pc, Stmt::Eval(expr));
            }
            1 => {
                let e = Entry::Plain { expr, seq: pc };
                if ipush == 1 {
                    self.ints.push(e);
                } else {
                    self.strs.push(e);
                }
            }
            _ => {
                let slot = self.slots.len();
                self.slots.push(MultiSlot { call: expr, pushes: ipush + spush });
                for _ in 0..ipush {
                    self.ints.push(Entry::Multi { slot, seq: pc });
                }
                for _ in 0..spush {
                    self.strs.push(Entry::Multi { slot, seq: pc });
                }
            }
        }
        Ok(())
    }

    fn flush_stmt(&mut self, _pc: usize, stmt: Stmt) {
        let start_pc = self.cur_start.take().expect("statement must span instructions");
        self.items.push(ItemBuf { start_pc, item: Item::Stmt(stmt) });
    }

    /// A sink consumed a value. Flush an assignment once both stacks are empty.
    fn sink(&mut self, pc: usize, target: Target, value: Entry) -> Result<(), LiftError> {
        self.pending.push((target, value));
        if !self.depth0() {
            return Ok(());
        }
        let pending = std::mem::take(&mut self.pending);
        let stmt = if pending.len() == 1 {
            let (target, value) = pending.into_iter().next().expect("len 1");
            match value {
                Entry::Plain { expr, .. } => Stmt::Assign { targets: vec![target], value: expr },
                Entry::Multi { slot, .. } => {
                    if self.slots[slot].pushes != 1 {
                        return err(pc, "partial consumption of multi-return call");
                    }
                    Stmt::Assign { targets: vec![target], value: self.slots[slot].call.clone() }
                }
            }
        } else {
            // Several sinks with no intervening empty-stack point: must all consume the
            // same multi-return call. Pop order is reverse source order.
            let mut slot_id: Option<usize> = None;
            let mut targets = Vec::with_capacity(pending.len());
            for (target, value) in pending {
                match value {
                    Entry::Multi { slot, .. } if slot_id.is_none() || slot_id == Some(slot) => {
                        slot_id = Some(slot);
                        targets.push(target);
                    }
                    _ => return err(pc, "interleaved assignments are not a multi-return"),
                }
            }
            let slot = slot_id.expect("non-empty pending");
            if self.slots[slot].pushes != targets.len() {
                return err(pc, "multi-return not fully consumed by assignment");
            }
            targets.reverse();
            Stmt::Assign { targets, value: self.slots[slot].call.clone() }
        };
        self.flush_stmt(pc, stmt);
        Ok(())
    }
}

fn linearize(
    id: u32,
    s: &ClientScript,
    sigs: &BTreeMap<u32, ScriptSig>,
) -> Result<Vec<ItemBuf>, LiftError> {
    let n = s.instructions.len();

    // Branch targets — every one must land on a statement boundary.
    let mut labels = vec![false; n];
    for pc in 0..n {
        let op = s.instructions[pc];
        if is_conditional_branch(op) || is_unconditional_branch(op) {
            let t = pc as i64 + 1 + i64::from(s.int_operands[pc]);
            if t < 0 || t >= n as i64 {
                return err(pc, format!("branch target {t} out of bounds"));
            }
            labels[t as usize] = true;
        }
    }

    let mut lx = Linearizer {
        ints: Vec::new(),
        strs: Vec::new(),
        pending: Vec::new(),
        slots: Vec::new(),
        items: Vec::new(),
        cur_start: None,
    };

    for pc in 0..n {
        if labels[pc] && (!lx.depth0() || !lx.pending.is_empty() || lx.cur_start.is_some()) {
            return err(pc, "branch target inside a statement");
        }
        let op = s.instructions[pc];
        let opn = opcode_name(op).unwrap_or("?");
        let Some(ar) = arity(op) else {
            return err(pc, format!("opcode {op} missing from arity table"));
        };
        if lx.cur_start.is_none() {
            lx.cur_start = Some(pc);
        }
        // Only sinks may run while assignments are pending (multi-return pops).
        let is_sink = matches!(op, 2 | 27 | 34 | 36 | 38 | 39 | 43 | 46 | 48);
        if !lx.pending.is_empty() && !is_sink {
            return err(pc, format!("{opn} while an assignment is mid-flight"));
        }

        match op {
            // --- pushes ---
            0 => lx.ints.push(Entry::Plain { expr: Expr::ConstInt(s.int_operands[pc]), seq: pc }),
            3 => lx.strs.push(Entry::Plain { expr: Expr::ConstStr(s.string_operands[pc].clone()), seq: pc }),
            1 => lx.ints.push(Entry::Plain { expr: Expr::Varp(s.int_operands[pc] as u16), seq: pc }),
            25 => lx.ints.push(Entry::Plain { expr: Expr::Varbit(s.int_operands[pc] as u16), seq: pc }),
            33 => lx.ints.push(Entry::Plain { expr: Expr::LocalInt(s.int_operands[pc] as u16), seq: pc }),
            35 => lx.strs.push(Entry::Plain { expr: Expr::LocalStr(s.int_operands[pc] as u16), seq: pc }),
            42 => lx.ints.push(Entry::Plain { expr: Expr::VarcInt(s.int_operands[pc] as u16), seq: pc }),
            47 => lx.strs.push(Entry::Plain { expr: Expr::VarcStr(s.int_operands[pc] as u16), seq: pc }),
            45 => {
                let index = lx.pop_plain_int(pc, opn)?;
                let expr = Expr::ArrayLoad { array: s.int_operands[pc] as u8, index: Box::new(index) };
                lx.ints.push(Entry::Plain { expr, seq: pc });
            }

            // --- sinks ---
            34 => { let v = lx.pop_int(pc, opn)?; lx.sink(pc, Target::LocalInt(s.int_operands[pc] as u16), v)?; }
            36 => { let v = lx.pop_str(pc, opn)?; lx.sink(pc, Target::LocalStr(s.int_operands[pc] as u16), v)?; }
            2 => { let v = lx.pop_int(pc, opn)?; lx.sink(pc, Target::Varp(s.int_operands[pc] as u16), v)?; }
            27 => { let v = lx.pop_int(pc, opn)?; lx.sink(pc, Target::Varbit(s.int_operands[pc] as u16), v)?; }
            43 => { let v = lx.pop_int(pc, opn)?; lx.sink(pc, Target::VarcInt(s.int_operands[pc] as u16), v)?; }
            48 => { let v = lx.pop_str(pc, opn)?; lx.sink(pc, Target::VarcStr(s.int_operands[pc] as u16), v)?; }
            38 => { let v = lx.pop_int(pc, opn)?; lx.sink(pc, Target::DiscardInt, v)?; }
            39 => { let v = lx.pop_str(pc, opn)?; lx.sink(pc, Target::DiscardStr, v)?; }
            46 => {
                let value = lx.pop_int(pc, opn)?;
                let index = lx.pop_plain_int(pc, opn)?;
                lx.sink(pc, Target::Array { array: s.int_operands[pc] as u8, index }, value)?;
            }

            // --- array definition ---
            44 => {
                let len = lx.pop_plain_int(pc, opn)?;
                if !lx.depth0() {
                    return err(pc, "define_array with values still on the stack");
                }
                let packed = s.int_operands[pc];
                lx.flush_stmt(pc, Stmt::DefineArray {
                    array: (packed >> 16) as u8,
                    elem_type: (packed & 0xFFFF) as u8,
                    len,
                });
            }

            // --- control flow ---
            6 => {
                if !lx.depth0() {
                    return err(pc, "jump with values still on the stack");
                }
                let target_pc = (pc as i64 + 1 + i64::from(s.int_operands[pc])) as usize;
                let start_pc = lx.cur_start.take().expect("set above");
                lx.items.push(ItemBuf { start_pc, item: Item::Jump { target_pc } });
            }
            7 | 8 | 9 | 10 | 31 | 32 => {
                let rhs = lx.pop_plain_int(pc, opn)?;
                let lhs = lx.pop_plain_int(pc, opn)?;
                if !lx.depth0() {
                    return err(pc, "conditional branch with values still on the stack");
                }
                let target_pc = (pc as i64 + 1 + i64::from(s.int_operands[pc])) as usize;
                let start_pc = lx.cur_start.take().expect("set above");
                lx.items.push(ItemBuf { start_pc, item: Item::CondJump { op, lhs, rhs, target_pc } });
            }
            21 => {
                let ni = lx.ints.len();
                let ns = lx.strs.len();
                let mut taken: Vec<Entry> = lx.ints.drain(..).chain(lx.strs.drain(..)).collect();
                taken.sort_by_key(Entry::seq);
                // Multi-return forwarding (`return(~foo(...))`) surfaces the call once.
                let mut exprs = Vec::with_capacity(taken.len());
                let mut seen_slots: Vec<usize> = Vec::new();
                for e in taken {
                    match e {
                        Entry::Plain { expr, .. } => exprs.push(expr),
                        Entry::Multi { slot, .. } => {
                            if !seen_slots.contains(&slot) {
                                seen_slots.push(slot);
                                exprs.push(lx.slots[slot].call.clone());
                            }
                        }
                    }
                }
                let _ = (ni, ns);
                if lx.cur_start.is_none() {
                    lx.cur_start = Some(pc);
                }
                lx.flush_stmt(pc, Stmt::Return(exprs));
            }

            // --- string join ---
            37 => {
                let count = s.int_operands[pc];
                if count < 0 {
                    return err(pc, "join_string with negative count");
                }
                let parts = lx.pop_args(pc, opn, 0, count as usize)?;
                lx.push_result(pc, Expr::Join(parts), 0, 1)?;
            }

            // --- gosub ---
            40 => {
                let callee = s.int_operands[pc] as u32;
                let Some(sig) = sigs.get(&callee) else {
                    return err(pc, format!("gosub {callee}: callee signature unknown"));
                };
                let args = lx.pop_args(pc, opn, sig.int_args as usize, sig.str_args as usize)?;
                lx.push_result(
                    pc,
                    Expr::Gosub { script: callee, args },
                    sig.int_returns as usize,
                    sig.str_returns as usize,
                )?;
            }

            // --- everything else: native commands ---
            _ => {
                let flag = s.int_operands[pc] == 1
                    && matches!(crate::cs2_opcodes::operand_kind(op), crate::cs2_opcodes::OperandKind::SecondaryFlag);
                let (ni, ns, ipush, spush) = match ar {
                    Arity::Fixed { ipop, spop, ipush, spush } => {
                        (ipop as usize, spop as usize, ipush as usize, spush as usize)
                    }
                    Arity::EventHandler { component_from_stack } => {
                        let Some(Entry::Plain { expr: Expr::ConstStr(d), .. }) = lx.strs.last() else {
                            return err(pc, format!("{opn}: descriptor string is not a constant"));
                        };
                        let mut descriptor = d.as_str();
                        let mut ni = usize::from(component_from_stack) + 1; // + handler id
                        let mut ns = 1; // descriptor itself
                        if let Some(stripped) = descriptor.strip_suffix('Y') {
                            descriptor = stripped;
                            // count sits below the descriptor on the int side
                            let at = lx.ints.len().checked_sub(1 + usize::from(component_from_stack));
                            let Some(Entry::Plain { expr: Expr::ConstInt(count), .. }) =
                                at.and_then(|i| lx.ints.get(i))
                            else {
                                return err(pc, format!("{opn}: 'Y' trigger count is not a constant"));
                            };
                            ni += 1 + (*count).max(0) as usize;
                        }
                        ns += descriptor.chars().filter(|&c| c == 's').count();
                        ni += descriptor.chars().filter(|&c| c != 's').count();
                        (ni, ns, 0, 0)
                    }
                    Arity::EnumLookup => {
                        let at = lx.ints.len().checked_sub(3);
                        let Some(Entry::Plain { expr: Expr::ConstInt(outputtype), .. }) =
                            at.and_then(|i| lx.ints.get(i))
                        else {
                            return err(pc, "enum: outputtype is not a constant");
                        };
                        if *outputtype == i32::from(b's') {
                            (4, 0, 0, 1)
                        } else {
                            (4, 0, 1, 0)
                        }
                    }
                    Arity::Return | Arity::JoinString | Arity::Gosub => {
                        unreachable!("handled by dedicated match arms")
                    }
                };
                let args = lx.pop_args(pc, opn, ni, ns)?;
                lx.push_result(pc, Expr::Command { op, flag, args }, ipush, spush)?;
            }
        }
    }

    if lx.cur_start.is_some() || !lx.depth0() || !lx.pending.is_empty() {
        return Err(LiftError {
            pc: None,
            message: format!("script {id}: trailing incomplete statement"),
        });
    }
    Ok(lx.items)
}

// ---------------------------------------------------------------------------------
// Phase 2: structure
// ---------------------------------------------------------------------------------

fn structure(
    items: &[ItemBuf],
    lo: usize,
    hi: usize,
    pc_to_idx: &BTreeMap<usize, usize>,
) -> Result<Vec<Stmt>, LiftError> {
    let idx_of = |pc: usize, at: usize| -> Result<usize, LiftError> {
        pc_to_idx
            .get(&pc)
            .copied()
            .map_or_else(|| err(at, format!("branch target pc {pc} is not a statement boundary")), Ok)
    };

    let mut out = Vec::new();
    let mut i = lo;
    while i < hi {
        match &items[i].item {
            Item::Stmt(stmt) => {
                out.push(stmt.clone());
                i += 1;
            }
            Item::Jump { target_pc } => {
                return err(items[i].start_pc, format!("unstructured jump to pc {target_pc}"));
            }
            Item::CondJump { .. } => {
                let at = items[i].start_pc;
                let (links, run_end) = collect_links(items, i, hi);
                let starts: Vec<usize> = links.iter().map(|l| l.start_pc).collect();

                // Try the longest chain prefix first (canonical &&/|| grouping); fall
                // back to shorter prefixes so nested ifs whose inner false-targets
                // differ from the outer's still structure (the leftover links are
                // re-parsed inside the then-block).
                let mut committed = None;
                let mut last_err: Option<LiftError> = None;
                for p in (1..=links.len()).rev() {
                    let Some(f_pc) = links[p - 1].f else { continue };
                    let j = links[p - 1].next_idx;
                    let t_pc = if p < links.len() {
                        starts[p]
                    } else if run_end < hi {
                        items[run_end].start_pc
                    } else {
                        continue; // chain at the very end of the block — no body
                    };
                    let Ok(idx_f) = idx_of(f_pc, at) else { continue };
                    if idx_f <= j || idx_f > hi {
                        continue;
                    }

                    // A trailing back-jump into the chain marks a while loop; landing
                    // on link m > 0 means links[0..m] are an enclosing if-condition
                    // (`if (c) { while (w) {...} }` shares one false-target).
                    let back_m = match items[idx_f - 1].item {
                        Item::Jump { target_pc } => starts[..p].iter().position(|&s| s == target_pc),
                        _ => None,
                    };
                    let attempt = (|| -> Result<_, LiftError> {
                        match back_m {
                            Some(0) => {
                                let cond = build_cond(&links[..p], t_pc, f_pc)?;
                                let body = structure(items, j, idx_f - 1, pc_to_idx)?;
                                Ok((Stmt::While { cond, body }, idx_f))
                            }
                            Some(m) => {
                                let c_if = build_cond(&links[..m], starts[m], f_pc)?;
                                let c_while = build_cond(&links[m..p], t_pc, f_pc)?;
                                let body = structure(items, j, idx_f - 1, pc_to_idx)?;
                                Ok((
                                    Stmt::If {
                                        cond: c_if,
                                        then_body: vec![Stmt::While { cond: c_while, body }],
                                        else_body: Vec::new(),
                                    },
                                    idx_f,
                                ))
                            }
                            None => {
                                let cond = build_cond(&links[..p], t_pc, f_pc)?;
                                // A trailing *forward* jump skips an else-block. A
                                // backward one belongs to a nested while that is the
                                // then-block's last statement (it shares the if's
                                // false-target) — leave it in the block for the
                                // recursion to structure.
                                if let Item::Jump { target_pc } = items[idx_f - 1].item {
                                    if target_pc > f_pc {
                                        let idx_e = idx_of(target_pc, items[idx_f - 1].start_pc)?;
                                        if idx_e < idx_f || idx_e > hi {
                                            return err(
                                                items[idx_f - 1].start_pc,
                                                "else-skip jump outside the enclosing block",
                                            );
                                        }
                                        let then_body = structure(items, j, idx_f - 1, pc_to_idx)?;
                                        let else_body = structure(items, idx_f, idx_e, pc_to_idx)?;
                                        return Ok((Stmt::If { cond, then_body, else_body }, idx_e));
                                    }
                                }
                                let then_body = structure(items, j, idx_f, pc_to_idx)?;
                                Ok((Stmt::If { cond, then_body, else_body: Vec::new() }, idx_f))
                            }
                        }
                    })();
                    match attempt {
                        Ok(done) => {
                            committed = Some(done);
                            break;
                        }
                        Err(e) => last_err = Some(e),
                    }
                }
                let Some((stmt, next_i)) = committed else {
                    return Err(last_err.unwrap_or(LiftError {
                        pc: Some(at),
                        message: "condition chain has no workable grouping".into(),
                    }));
                };
                out.push(stmt);
                i = next_i;
            }
        }
    }
    Ok(out)
}

/// One comparison of a condition chain. Form A (`f == None`) jumps to `t` when true
/// and falls through to the next link when false; Form B carries an explicit
/// false-jump.
struct Link {
    cond: Cond,
    t: usize,
    f: Option<usize>,
    start_pc: usize,
    /// Item index just past this link (past the false-jump for Form B).
    next_idx: usize,
}

/// Collect the maximal run of condition links starting at item `i`.
fn collect_links(items: &[ItemBuf], i: usize, hi: usize) -> (Vec<Link>, usize) {
    let mut links = Vec::new();
    let mut x = i;
    while x < hi {
        let Item::CondJump { op, lhs, rhs, target_pc } = &items[x].item else { break };
        let cond = Cond::Cmp { op: *op, lhs: lhs.clone(), rhs: rhs.clone() };
        let start_pc = items[x].start_pc;
        if x + 1 < hi {
            if let Item::Jump { target_pc: f } = items[x + 1].item {
                links.push(Link { cond, t: *target_pc, f: Some(f), start_pc, next_idx: x + 2 });
                x += 2;
                continue;
            }
        }
        links.push(Link { cond, t: *target_pc, f: None, start_pc, next_idx: x + 1 });
        x += 1;
    }
    (links, x)
}

/// Build a boolean expression from a link slice given the chain's overall exits:
/// control flows to `t_exit` when the whole condition is true, `f_exit` when false.
/// Grouping is recovered purely from jump targets:
///
/// - Form A, `t == t_exit` — `c || rest`
/// - Form A, `t == start of link m` — `(c || links[1..m]) && links[m..]`
/// - Form B, `t == next link, f == f_exit` — `c && rest`
/// - Form B, `t == t_exit, f == next link` — `c || rest` (jump form)
/// - Form B, `t == next link, f == start of link m` — `(c && links[1..m]) || links[m..]`
fn build_cond(links: &[Link], t_exit: usize, f_exit: usize) -> Result<Cond, LiftError> {
    let l0 = &links[0];
    let at = l0.start_pc;
    if links.len() == 1 {
        if l0.t == t_exit && l0.f == Some(f_exit) {
            return Ok(l0.cond.clone());
        }
        return err(at, "condition link targets don't match the chain exits");
    }
    let starts: Vec<usize> = links.iter().map(|l| l.start_pc).collect();
    let pair = |a: Cond, b: Cond, or: bool| {
        if or { Cond::Or(Box::new(a), Box::new(b)) } else { Cond::And(Box::new(a), Box::new(b)) }
    };
    match l0.f {
        None => {
            if l0.t == t_exit {
                let rest = build_cond(&links[1..], t_exit, f_exit)?;
                return Ok(pair(l0.cond.clone(), rest, true));
            }
            if let Some(m) = starts[1..].iter().position(|&s| s == l0.t).map(|k| k + 1) {
                if m >= 2 {
                    let group = build_cond(&links[1..m], starts[m], f_exit)?;
                    let left = pair(l0.cond.clone(), group, true);
                    let right = build_cond(&links[m..], t_exit, f_exit)?;
                    return Ok(pair(left, right, false));
                }
            }
            err(at, "fallthrough condition link's true-target not in the chain")
        }
        Some(f1) => {
            if l0.t == starts[1] && f1 == f_exit {
                let rest = build_cond(&links[1..], t_exit, f_exit)?;
                return Ok(pair(l0.cond.clone(), rest, false));
            }
            if l0.t == t_exit && f1 == starts[1] {
                let rest = build_cond(&links[1..], t_exit, f_exit)?;
                return Ok(pair(l0.cond.clone(), rest, true));
            }
            if l0.t == starts[1] {
                if let Some(m) = starts[1..].iter().position(|&s| s == f1).map(|k| k + 1) {
                    if m >= 2 {
                        let group = build_cond(&links[1..m], t_exit, starts[m])?;
                        let left = pair(l0.cond.clone(), group, false);
                        let right = build_cond(&links[m..], t_exit, f_exit)?;
                        return Ok(pair(left, right, true));
                    }
                }
            }
            err(at, "condition link targets don't form a chain")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cs2_sig::analyze_all;

    fn script(ops: &[(u16, i32)], strs: &[(usize, &str)]) -> ClientScript {
        let mut s = ClientScript {
            name: None,
            instructions: ops.iter().map(|&(op, _)| op).collect(),
            int_operands: ops.iter().map(|&(_, v)| v).collect(),
            string_operands: vec![String::new(); ops.len()],
            int_local_count: 4,
            string_local_count: 4,
            int_arg_count: 0,
            string_arg_count: 0,
        };
        for &(i, text) in strs {
            s.string_operands[i] = text.to_owned();
        }
        s
    }

    fn lift_one(s: ClientScript) -> Result<ScriptIr, LiftError> {
        let mut all = BTreeMap::new();
        all.insert(0u32, s);
        let a = analyze_all(&all);
        assert!(a.diags.is_empty(), "{:?}", a.diags);
        lift(0, &all[&0], &a.sigs)
    }

    #[test]
    fn assignment_and_calc_expression() {
        // $int0 = add(2, 3); return
        let ir = lift_one(script(&[(0, 2), (0, 3), (4000, 0), (34, 0), (21, 0)], &[])).unwrap();
        assert_eq!(ir.body.len(), 2);
        let Stmt::Assign { targets, value } = &ir.body[0] else { panic!("{:?}", ir.body) };
        assert_eq!(targets, &vec![Target::LocalInt(0)]);
        let Expr::Command { op: 4000, args, .. } = value else { panic!("{value:?}") };
        assert_eq!(args, &vec![Expr::ConstInt(2), Expr::ConstInt(3)]);
    }

    #[test]
    fn if_else_structure() {
        // if ($int0 = 1) { $int1 = 2 } else { $int1 = 3 } return
        // 0: push_local 0; 1: push 1; 2: branch_equals → 5; 3: jump → 8(F);
        // 5: push 2; 6: pop_local 1; 7: jump → 10(E); 8: push 3; 9: pop_local 1; 10: return
        let ir = lift_one(script(
            &[
                (33, 0), (0, 1), (8, 1), (6, 3),
                (0, 2), (34, 1), (6, 2),
                (0, 3), (34, 1),
                (21, 0),
            ],
            &[],
        ))
        .unwrap();
        assert_eq!(ir.body.len(), 2);
        let Stmt::If { cond, then_body, else_body } = &ir.body[0] else { panic!("{:?}", ir.body) };
        assert_eq!(
            cond,
            &Cond::Cmp { op: 8, lhs: Expr::LocalInt(0), rhs: Expr::ConstInt(1) }
        );
        assert_eq!(then_body.len(), 1);
        assert_eq!(else_body.len(), 1);
    }

    #[test]
    fn while_loop_structure() {
        // while ($int0 < 10) { $int0 = add($int0, 1) } return
        // 0: push_local 0; 1: push 10; 2: branch_lt → 5; 3: jump → 10(F);
        // 5: push_local 0; 6: push 1; 7: add; 8: pop_local 0; 9: jump → 0;
        // 10: return
        let ir = lift_one(script(
            &[
                (33, 0), (0, 10), (9, 1), (6, 5),
                (33, 0), (0, 1), (4000, 0), (34, 0), (6, -9),
                (21, 0),
            ],
            &[],
        ))
        .unwrap();
        assert_eq!(ir.body.len(), 2);
        let Stmt::While { cond, body } = &ir.body[0] else { panic!("{:?}", ir.body) };
        assert_eq!(
            cond,
            &Cond::Cmp { op: 9, lhs: Expr::LocalInt(0), rhs: Expr::ConstInt(10) }
        );
        assert_eq!(body.len(), 1);
    }

    #[test]
    fn and_chain_canonicalises() {
        // if ($int0 = 1 && $int1 = 2) { mes("x") } return
        // 0: push_local 0; 1: push 1; 2: beq → 5; 3: jump → 11(F);
        // 5: push_local 1; 6: push 2; 7: beq → 10; 8: jump → 11(F);
        // 10(wait pcs)...
        // pcs: 0,1,2,3 | 4,5,6,7 | 8: push "x"; 9: mes; 10: return
        // chain1 t=4 f=10; chain2 at 4: t=8 f=10
        let ir = lift_one(script(
            &[
                (33, 0), (0, 1), (8, 1), (6, 6),
                (33, 1), (0, 2), (8, 1), (6, 2),
                (3, 0), (3100, 0),
                (21, 0),
            ],
            &[(8, "x")],
        ))
        .unwrap();
        let Stmt::If { cond, then_body, else_body } = &ir.body[0] else { panic!("{:?}", ir.body) };
        assert!(matches!(cond, Cond::And(..)), "{cond:?}");
        assert_eq!(then_body.len(), 1);
        assert!(else_body.is_empty());
    }

    #[test]
    fn multi_return_gosub_assigns_in_source_order() {
        // callee (id 1): return(11, 22)
        // caller (id 0): $int0, $int1 = gosub 1; return
        let callee = script(&[(0, 11), (0, 22), (21, 0)], &[]);
        let caller = script(&[(40, 1), (34, 1), (34, 0), (21, 0)], &[]);
        let mut all = BTreeMap::new();
        all.insert(0u32, caller);
        all.insert(1u32, callee);
        let a = analyze_all(&all);
        assert!(a.diags.is_empty(), "{:?}", a.diags);
        let ir = lift(0, &all[&0], &a.sigs).unwrap();
        let Stmt::Assign { targets, value } = &ir.body[0] else { panic!("{:?}", ir.body) };
        // pops were pop $1 then pop $0 → source order $0, $1
        assert_eq!(targets, &vec![Target::LocalInt(0), Target::LocalInt(1)]);
        assert!(matches!(value, Expr::Gosub { script: 1, .. }));
    }
}
