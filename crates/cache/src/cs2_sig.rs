//! CS2 signature inference + stack-balance verification.
//!
//! Runs a depth-tracking abstract interpretation over every decoded [`ClientScript`]:
//! pushes/pops follow the exact arities in [`crate::cs2_opcodes`], constants from
//! `push_constant_int` / `push_constant_string` are propagated so the dynamic-arity
//! ops can be resolved statically:
//!
//! - `join_string` — count is the int operand
//! - `gosub_with_params` — pops the callee's declared args, pushes its *inferred*
//!   returns (fixpoint over the call graph; CS2 has no recursion, so this terminates)
//! - `cc_seton*` / `if_seton*` — the descriptor string (top of string stack) and the
//!   `Y` trigger-list count must be constants at the call site
//! - `enum` (3408) — the `outputtype` argument must be a constant char
//! - `return` — the stack depths at the return site *are* the script's return
//!   signature; every return site of a script must agree
//!
//! Output is a [`ScriptSig`] per script — exactly what the decompiler needs to render
//! `gosub` as a typed call expression — plus diagnostics for anything that violates
//! the model (which doubles as an empirical proof of the arity table over the cache).

use std::collections::{BTreeMap, HashMap, VecDeque};

use crate::cs2::ClientScript;
use crate::cs2_opcodes::{arity, branch_keyword, is_unconditional_branch, opcode_name, Arity};

/// Argument and return counts for one script. Args come from the decode trailer;
/// returns are inferred from stack depths at the script's return sites.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptSig {
    pub int_args: u16,
    pub str_args: u16,
    pub int_returns: u16,
    pub str_returns: u16,
}

/// One analysis failure, tied to a script (and instruction where meaningful).
#[derive(Debug, Clone)]
pub struct Diag {
    pub script: u32,
    pub pc: Option<usize>,
    pub message: String,
}

impl std::fmt::Display for Diag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.pc {
            Some(pc) => write!(f, "script {} pc {}: {}", self.script, pc, self.message),
            None => write!(f, "script {}: {}", self.script, self.message),
        }
    }
}

/// Result of analysing the whole archive.
pub struct Analysis {
    pub sigs: BTreeMap<u32, ScriptSig>,
    pub diags: Vec<Diag>,
}

/// Abstract int-stack slot: known constant or opaque.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IntVal {
    Const(i32),
    Unknown,
}

/// Abstract string-stack slot. Constants are interned per script via index into the
/// script's own `string_operands`, so cloning a state is cheap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StrVal {
    /// `string_operands[pc]` of the originating `push_constant_string`.
    ConstAt(usize),
    Unknown,
}

#[derive(Debug, Clone, Default)]
struct State {
    ints: Vec<IntVal>,
    strs: Vec<StrVal>,
}

impl State {
    fn depths(&self) -> (usize, usize) {
        (self.ints.len(), self.strs.len())
    }
}

enum ScriptResult {
    /// `Some((int_returns, str_returns))`; `None` only in prune mode when every
    /// return site sits behind the recursive call (no base case reached).
    Done(Option<(u16, u16)>),
    /// Needs this callee's signature before it can be analysed.
    Blocked(u32),
    Failed(Vec<Diag>),
}

/// Analyse every script and infer its signature. Scripts whose callees haven't been
/// resolved yet are retried after the callees succeed; if a round makes no progress
/// (recursion, or a dependency on a failed script) the stragglers get diagnostics.
#[must_use]
pub fn analyze_all(scripts: &BTreeMap<u32, ClientScript>) -> Analysis {
    let mut sigs: BTreeMap<u32, ScriptSig> = BTreeMap::new();
    let mut diags: Vec<Diag> = Vec::new();
    let mut pending: Vec<u32> = scripts.keys().copied().collect();

    loop {
        let mut blocked: Vec<(u32, u32)> = Vec::new();
        let mut progressed = false;

        for id in pending {
            let s = &scripts[&id];
            let done = |int_returns: u16, str_returns: u16, sigs: &mut BTreeMap<u32, ScriptSig>| {
                sigs.insert(
                    id,
                    ScriptSig {
                        int_args: s.int_arg_count,
                        str_args: s.string_arg_count,
                        int_returns,
                        str_returns,
                    },
                );
            };
            match analyze_script(id, s, &sigs, None, false) {
                ScriptResult::Done(Some((ir, sr))) => {
                    done(ir, sr, &mut sigs);
                    progressed = true;
                }
                ScriptResult::Done(None) => unreachable!("normal mode always reaches a return or fails"),
                // Self-recursive: find the base-case signature with recursive paths
                // pruned, then re-analyse with it as hypothesis and verify every
                // return site (including those behind the recursive call) agrees.
                ScriptResult::Blocked(callee) if callee == id => {
                    match analyze_script(id, s, &sigs, None, true) {
                        ScriptResult::Done(Some((ir, sr))) => {
                            match analyze_script(id, s, &sigs, Some((ir, sr)), false) {
                                ScriptResult::Done(Some(confirmed)) if confirmed == (ir, sr) => {
                                    done(ir, sr, &mut sigs);
                                    progressed = true;
                                }
                                ScriptResult::Done(_) => {
                                    diags.push(Diag {
                                        script: id,
                                        pc: None,
                                        message: format!(
                                            "recursive script: base-case returns ({ir}, {sr}) contradicted on recursive paths"
                                        ),
                                    });
                                    progressed = true;
                                }
                                ScriptResult::Blocked(c) => blocked.push((id, c)),
                                ScriptResult::Failed(d) => {
                                    diags.extend(d);
                                    progressed = true;
                                }
                            }
                        }
                        ScriptResult::Done(None) => {
                            diags.push(Diag {
                                script: id,
                                pc: None,
                                message: "recursive script with no base-case return".into(),
                            });
                            progressed = true;
                        }
                        ScriptResult::Blocked(c) => blocked.push((id, c)),
                        ScriptResult::Failed(d) => {
                            diags.extend(d);
                            progressed = true;
                        }
                    }
                }
                ScriptResult::Blocked(callee) => blocked.push((id, callee)),
                ScriptResult::Failed(d) => {
                    diags.extend(d);
                    progressed = true; // failed scripts don't get retried
                }
            }
        }

        if blocked.is_empty() {
            break;
        }
        if !progressed {
            for (id, callee) in blocked {
                diags.push(Diag {
                    script: id,
                    pc: None,
                    message: format!(
                        "unresolvable: blocked on callee {callee} (recursion or failed callee)"
                    ),
                });
            }
            break;
        }
        pending = blocked.into_iter().map(|(id, _)| id).collect();
    }

    Analysis { sigs, diags }
}

/// Depth-tracking walk over one script. Every reachable path must reach `return` with
/// consistent depths; joins must agree on depth; no stack may underflow.
///
/// Self-recursion controls: a gosub back to `id` uses `self_sig` as the callee
/// signature when provided; otherwise with `prune_self` the path is abandoned at the
/// recursive call (base-case discovery pass); otherwise the script is `Blocked` on
/// itself, which the driver turns into the prune→hypothesise→verify sequence.
fn analyze_script(
    id: u32,
    s: &ClientScript,
    sigs: &BTreeMap<u32, ScriptSig>,
    self_sig: Option<(u16, u16)>,
    prune_self: bool,
) -> ScriptResult {
    let n = s.instructions.len();
    let fail = |pc: usize, message: String| {
        ScriptResult::Failed(vec![Diag { script: id, pc: Some(pc), message }])
    };
    if n == 0 {
        return ScriptResult::Failed(vec![Diag {
            script: id,
            pc: None,
            message: "empty script (no instructions)".into(),
        }]);
    }

    // Depths recorded at each pc on first visit; revisits must match. Constants are
    // only propagated on the first visit, which is safe because every dynamic-arity op
    // gets its constants from pushes in the same basic block (the Jagex compiler emits
    // descriptor/count pushes adjacent to the op that consumes them).
    let mut seen: HashMap<usize, (usize, usize)> = HashMap::new();
    let mut work: VecDeque<(usize, State)> = VecDeque::new();
    let mut returns: Option<(usize, usize)> = None;
    work.push_back((0, State::default()));

    while let Some((entry_pc, mut st)) = work.pop_front() {
        let mut pc = entry_pc;
        loop {
            if pc >= n {
                return fail(pc, "execution fell off the end of the script".into());
            }
            if let Some(&depths) = seen.get(&pc) {
                if depths != st.depths() {
                    return fail(
                        pc,
                        format!(
                            "inconsistent stack depths at join: {:?} vs {:?}",
                            depths,
                            st.depths()
                        ),
                    );
                }
                break; // path already explored from here
            }
            seen.insert(pc, st.depths());

            let op = s.instructions[pc];
            let opn = opcode_name(op).unwrap_or("?");
            let Some(ar) = arity(op) else {
                return fail(pc, format!("opcode {op} ({opn}) missing from arity table"));
            };

            macro_rules! pop_ints {
                ($k:expr) => {{
                    let k = $k;
                    if st.ints.len() < k {
                        return fail(pc, format!("{opn}: int stack underflow (need {k}, have {})", st.ints.len()));
                    }
                    st.ints.truncate(st.ints.len() - k);
                }};
            }
            macro_rules! pop_strs {
                ($k:expr) => {{
                    let k = $k;
                    if st.strs.len() < k {
                        return fail(pc, format!("{opn}: string stack underflow (need {k}, have {})", st.strs.len()));
                    }
                    st.strs.truncate(st.strs.len() - k);
                }};
            }

            match ar {
                Arity::Fixed { ipop, spop, ipush, spush } => {
                    // Branches consume their comparands before transferring control.
                    pop_ints!(ipop as usize);
                    pop_strs!(spop as usize);

                    if !branch_keyword(op).is_empty() || is_unconditional_branch(op) {
                        let target = pc as i64 + 1 + i64::from(s.int_operands[pc]);
                        if target < 0 || target >= n as i64 {
                            return fail(pc, format!("branch target {target} out of bounds"));
                        }
                        let target = target as usize;
                        if is_unconditional_branch(op) {
                            pc = target;
                            continue;
                        }
                        work.push_back((target, st.clone()));
                        pc += 1;
                        continue;
                    }

                    for _ in 0..ipush {
                        st.ints.push(if op == 0 {
                            IntVal::Const(s.int_operands[pc])
                        } else {
                            IntVal::Unknown
                        });
                    }
                    for _ in 0..spush {
                        st.strs.push(if op == 3 { StrVal::ConstAt(pc) } else { StrVal::Unknown });
                    }
                }

                Arity::Return => {
                    let d = st.depths();
                    match returns {
                        None => returns = Some(d),
                        Some(prev) if prev != d => {
                            return fail(
                                pc,
                                format!("return depth mismatch: {prev:?} earlier vs {d:?} here"),
                            );
                        }
                        Some(_) => {}
                    }
                    break;
                }

                Arity::JoinString => {
                    let count = s.int_operands[pc];
                    if count < 0 {
                        return fail(pc, format!("join_string with negative count {count}"));
                    }
                    pop_strs!(count as usize);
                    st.strs.push(StrVal::Unknown);
                }

                Arity::Gosub => {
                    let callee = s.int_operands[pc] as u32;
                    let (args, rets) = if callee == id {
                        match self_sig {
                            Some(h) => ((s.int_arg_count, s.string_arg_count), h),
                            None if prune_self => break, // base-case discovery: abandon path
                            None => return ScriptResult::Blocked(callee),
                        }
                    } else {
                        let Some(sig) = sigs.get(&callee) else {
                            return ScriptResult::Blocked(callee);
                        };
                        ((sig.int_args, sig.str_args), (sig.int_returns, sig.str_returns))
                    };
                    pop_ints!(args.0 as usize);
                    pop_strs!(args.1 as usize);
                    for _ in 0..rets.0 {
                        st.ints.push(IntVal::Unknown);
                    }
                    for _ in 0..rets.1 {
                        st.strs.push(StrVal::Unknown);
                    }
                }

                Arity::EventHandler { component_from_stack } => {
                    if component_from_stack {
                        pop_ints!(1);
                    }
                    let Some(StrVal::ConstAt(at)) = st.strs.last().copied() else {
                        return fail(pc, format!("{opn}: descriptor string is not a constant"));
                    };
                    pop_strs!(1);
                    let mut descriptor = s.string_operands[at].as_str();

                    if let Some(stripped) = descriptor.strip_suffix('Y') {
                        descriptor = stripped;
                        let Some(&IntVal::Const(count)) = st.ints.last() else {
                            return fail(pc, format!("{opn}: 'Y' trigger count is not a constant"));
                        };
                        pop_ints!(1);
                        if count > 0 {
                            pop_ints!(count as usize);
                        }
                    }
                    let str_args = descriptor.chars().filter(|&c| c == 's').count();
                    let int_args = descriptor.chars().count() - str_args;
                    pop_strs!(str_args);
                    pop_ints!(int_args);
                    pop_ints!(1); // handler script id
                }

                Arity::EnumLookup => {
                    // pops (top→bottom): key, enum id, outputtype, inputtype
                    pop_ints!(2);
                    let Some(&IntVal::Const(outputtype)) = st.ints.last() else {
                        return fail(pc, "enum: outputtype is not a constant".into());
                    };
                    pop_ints!(2);
                    if outputtype == i32::from(b's') {
                        st.strs.push(StrVal::Unknown);
                    } else {
                        st.ints.push(IntVal::Unknown);
                    }
                }
            }

            pc += 1;
        }
    }

    ScriptResult::Done(returns.map(|(i, s)| (i as u16, s as u16)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a script from (opcode, int operand) pairs with optional string operands.
    fn script(ops: &[(u16, i32)], strs: &[(usize, &str)], int_args: u16) -> ClientScript {
        let mut s = ClientScript {
            name: None,
            instructions: ops.iter().map(|&(op, _)| op).collect(),
            int_operands: ops.iter().map(|&(_, v)| v).collect(),
            string_operands: vec![String::new(); ops.len()],
            int_local_count: int_args,
            string_local_count: 0,
            int_arg_count: int_args,
            string_arg_count: 0,
        };
        for &(i, text) in strs {
            s.string_operands[i] = text.to_owned();
        }
        s
    }

    #[test]
    fn straight_line_script_returns_one_int() {
        // push 1; push 2; add; return  → returns (1, 0)
        let s = script(&[(0, 1), (0, 2), (4000, 0), (21, 0)], &[], 0);
        let mut all = BTreeMap::new();
        all.insert(0u32, s);
        let a = analyze_all(&all);
        assert!(a.diags.is_empty(), "{:?}", a.diags);
        assert_eq!(
            a.sigs[&0],
            ScriptSig { int_args: 0, str_args: 0, int_returns: 1, str_returns: 0 }
        );
    }

    #[test]
    fn branch_join_with_consistent_depths_passes() {
        // push 1; push 0; branch_equals +2; push 5; drop; return
        // (both arms reach `return` with empty stacks)
        let s = script(
            &[(0, 1), (0, 0), (8, 2), (0, 5), (38, 0), (21, 0)],
            &[],
            0,
        );
        let mut all = BTreeMap::new();
        all.insert(0u32, s);
        let a = analyze_all(&all);
        assert!(a.diags.is_empty(), "{:?}", a.diags);
        assert_eq!(a.sigs[&0].int_returns, 0);
    }

    #[test]
    fn inconsistent_join_depth_is_reported() {
        // push 1; push 0; branch_equals +1; push 5; return
        // taken arm reaches `return` with 0 ints, fallthrough with 1.
        let s = script(&[(0, 1), (0, 0), (8, 1), (0, 5), (21, 0)], &[], 0);
        let mut all = BTreeMap::new();
        all.insert(0u32, s);
        let a = analyze_all(&all);
        assert_eq!(a.diags.len(), 1, "{:?}", a.diags);
        // Surfaces either as a join mismatch (branch target seen at another depth) or
        // a return-depth mismatch depending on exploration order — both are correct.
        assert!(
            a.diags[0].message.contains("inconsistent stack depths")
                || a.diags[0].message.contains("return depth mismatch"),
            "{}",
            a.diags[0]
        );
    }

    #[test]
    fn underflow_is_reported() {
        let s = script(&[(4000, 0), (21, 0)], &[], 0); // add on empty stack
        let mut all = BTreeMap::new();
        all.insert(0u32, s);
        let a = analyze_all(&all);
        assert_eq!(a.diags.len(), 1);
        assert!(a.diags[0].message.contains("underflow"), "{}", a.diags[0]);
    }

    #[test]
    fn gosub_uses_callee_signature_via_fixpoint() {
        // script 1: push 7; gosub 0; drop; return     (callee returns one int)
        // script 0: push 42; return                   (0 args → 1 int return)
        // Analysis order forces 1 to block on 0 first (BTreeMap iterates 0 first,
        // so flip ids: callee is 1, caller is 0).
        let caller = script(&[(40, 1), (38, 0), (21, 0)], &[], 0);
        let callee = script(&[(0, 42), (21, 0)], &[], 0);
        let mut all = BTreeMap::new();
        all.insert(0u32, caller);
        all.insert(1u32, callee);
        let a = analyze_all(&all);
        assert!(a.diags.is_empty(), "{:?}", a.diags);
        assert_eq!(a.sigs[&1].int_returns, 1);
        assert_eq!(a.sigs[&0].int_returns, 0);
    }

    #[test]
    fn event_handler_descriptor_drives_pops() {
        // cc_setonclick("Is"): push handler id, push int arg, push str arg,
        // push descriptor, op 1400; return.
        // Pops: descriptor, 's' str, 'I' int, handler id → all consumed.
        let s = script(
            &[(0, 99), (0, 5), (3, 0), (3, 0), (1400, 0), (21, 0)],
            &[(2, "arg"), (3, "Is")],
            0,
        );
        let mut all = BTreeMap::new();
        all.insert(0u32, s);
        let a = analyze_all(&all);
        assert!(a.diags.is_empty(), "{:?}", a.diags);
        assert_eq!(a.sigs[&0].int_returns, 0);
        assert_eq!(a.sigs[&0].str_returns, 0);
    }

    #[test]
    fn enum_output_type_selects_stack() {
        // push inputtype 'i'; push outputtype 's'; push enum 3; push key 4; enum;
        // pop_string_discard; return
        let s = script(
            &[(0, 105), (0, 115), (0, 3), (0, 4), (3408, 0), (39, 0), (21, 0)],
            &[],
            0,
        );
        let mut all = BTreeMap::new();
        all.insert(0u32, s);
        let a = analyze_all(&all);
        assert!(a.diags.is_empty(), "{:?}", a.diags);
    }

    #[test]
    fn self_recursion_resolves_via_base_case() {
        // pc0 push 1; pc1 push 1; pc2 branch_equals +2 → pc5 (base case);
        // pc3 gosub self; pc4 return (recursive result);
        // pc5 push 42; pc6 return.
        // Both return sites leave one int → signature (0 args, 1 int return).
        let s = script(
            &[(0, 1), (0, 1), (8, 2), (40, 0), (21, 0), (0, 42), (21, 0)],
            &[],
            0,
        );
        let mut all = BTreeMap::new();
        all.insert(0u32, s);
        let a = analyze_all(&all);
        assert!(a.diags.is_empty(), "{:?}", a.diags);
        assert_eq!(
            a.sigs[&0],
            ScriptSig { int_args: 0, str_args: 0, int_returns: 1, str_returns: 0 }
        );
    }

    #[test]
    fn mutual_recursion_is_reported_not_hung() {
        // 0 gosubs 1, 1 gosubs 0 — neither can be hypothesised, both diagnosed.
        let a_calls_b = script(&[(40, 1), (21, 0)], &[], 0);
        let b_calls_a = script(&[(40, 0), (21, 0)], &[], 0);
        let mut all = BTreeMap::new();
        all.insert(0u32, a_calls_b);
        all.insert(1u32, b_calls_a);
        let a = analyze_all(&all);
        assert_eq!(a.diags.len(), 2, "{:?}", a.diags);
        assert!(a.diags[0].message.contains("blocked on callee"), "{}", a.diags[0]);
    }
}
