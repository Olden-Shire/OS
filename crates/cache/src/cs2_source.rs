//! Structured CS2 source format — RuneScript-style text for decompiled scripts.
//!
//! [`print`] renders [`crate::cs2_ir::ScriptIr`] to source; [`parse`] is its exact
//! inverse (`parse(print(ir)) == ir`), which together with the proven
//! bytecode ↔ IR round-trip (`cs2_decompile` / `cs2_compile`) makes the whole
//! text ↔ cache pipeline byte-exact.
//!
//! ```text
//! [clientscript,script_117]()()
//!
//! if (%varbit3756 < 1 | %varbit3756 > 14) {
//!     if_setontimer(35913774, "", -1)
//!     ~script_113;
//! } else {
//!     if_setontimer(35913774, "", 120)
//! }
//! return;
//! ```
//!
//! Format notes (all chosen so the canonical bytecode shapes survive a round trip):
//!
//! - Locals are canonical: `$int<N>` / `$string<N>`, args first. Non-arg locals are
//!   declared with `def_int $int<N>;` lines after the header.
//! - Vars are numeric sigils (`%varp287`, `%varbit12`, `%varcint5`, `%varcstr1`).
//! - Gosubs are `~name(...)` (name from `script.pack`, else `script_<id>`).
//! - A leading `.` marks the secondary-component flag on cc_/if_ commands.
//! - Arithmetic (ops 4000-4003/4011/4014/4015) prints as `calc(...)` with infix
//!   operators; everything else is a plain call. String concatenation is `join(...)`
//!   (not `<...>` interpolation — cache strings contain literal `<col=...>` tags).
//! - `&` / `|` don't mix without parens; chains are right-associative, matching the
//!   condition trees the lifter builds.
//! - Calling a value-returning script/command as a statement implies the compiler's
//!   `pop_*_discard`; mixed value/discard multi-assigns use `$_int` / `$_str`.

use std::collections::BTreeMap;

use crate::cs2_asm::{quote, NameMaps};
use crate::cs2_ir::{Cond, Expr, ScriptIr, Stmt, Target};
use crate::cs2_opcodes::{arity, opcode_by_name, opcode_name, Arity};
use crate::cs2_sig::ScriptSig;

#[derive(Debug, Clone)]
pub struct SourceError {
    pub line: usize,
    pub message: String,
}

impl std::fmt::Display for SourceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

// ---------------------------------------------------------------------------------
// Printing
// ---------------------------------------------------------------------------------

/// Operator + precedence for the `calc()` arithmetic commands. Precedence:
/// `& |` (1) < `+ -` (2) < `* / %` (3); all left-associative.
fn calc_op(op: u16) -> Option<(&'static str, u8)> {
    match op {
        4000 => Some(("+", 2)),
        4001 => Some(("-", 2)),
        4002 => Some(("*", 3)),
        4003 => Some(("/", 3)),
        4011 => Some(("%", 3)),
        4014 => Some(("&", 1)),
        4015 => Some(("|", 1)),
        _ => None,
    }
}

fn calc_op_by_sym(sym: &str) -> Option<(u16, u8)> {
    match sym {
        "+" => Some((4000, 2)),
        "-" => Some((4001, 2)),
        "*" => Some((4002, 3)),
        "/" => Some((4003, 3)),
        "%" => Some((4011, 3)),
        "&" => Some((4014, 1)),
        "|" => Some((4015, 1)),
        _ => None,
    }
}

fn cmp_sym(op: u16) -> &'static str {
    match op {
        7 => "!",
        8 => "=",
        9 => "<",
        10 => ">",
        31 => "<=",
        32 => ">=",
        _ => "?",
    }
}

fn cmp_op_by_sym(sym: &str) -> Option<u16> {
    match sym {
        "!" => Some(7),
        "=" => Some(8),
        "<" => Some(9),
        ">" => Some(10),
        "<=" => Some(31),
        ">=" => Some(32),
        _ => None,
    }
}

/// Render a lifted script to source text.
#[must_use]
pub fn print(ir: &ScriptIr, names: &NameMaps) -> String {
    let mut out = String::new();
    let name = names
        .script_name(ir.id as i32)
        .map_or_else(|| format!("script_{}", ir.id), str::to_owned);

    out.push_str(&format!("[clientscript,{name}]("));
    let mut params: Vec<String> = Vec::new();
    for i in 0..ir.int_args {
        params.push(format!("int $int{i}"));
    }
    for i in 0..ir.str_args {
        params.push(format!("string $string{i}"));
    }
    out.push_str(&params.join(", "));
    out.push_str(")(");
    let mut rets: Vec<&str> = Vec::new();
    rets.extend(std::iter::repeat_n("int", ir.int_returns as usize));
    rets.extend(std::iter::repeat_n("string", ir.str_returns as usize));
    out.push_str(&rets.join(", "));
    out.push_str(")\n");

    if let Some(n) = &ir.name {
        out.push_str(&format!(".name {}\n", quote(n)));
    }
    for i in ir.int_args..ir.int_locals {
        out.push_str(&format!("def_int $int{i};\n"));
    }
    for i in ir.str_args..ir.str_locals {
        out.push_str(&format!("def_string $string{i};\n"));
    }

    out.push('\n');
    print_stmts(&mut out, &ir.body, 0, names);
    out
}

fn indent(out: &mut String, depth: usize) {
    for _ in 0..depth {
        out.push_str("    ");
    }
}

fn print_stmts(out: &mut String, body: &[Stmt], depth: usize, names: &NameMaps) {
    for stmt in body {
        print_stmt(out, stmt, depth, names);
    }
}

fn print_stmt(out: &mut String, stmt: &Stmt, depth: usize, names: &NameMaps) {
    indent(out, depth);
    match stmt {
        Stmt::Assign { targets, value } => {
            if targets.iter().all(|t| matches!(t, Target::DiscardInt | Target::DiscardStr)) {
                // Bare call statement — the parser re-derives the discards from the
                // callee/command signature.
                out.push_str(&print_expr(value, names));
                out.push_str(";\n");
                return;
            }
            let rendered: Vec<String> = targets.iter().map(|t| print_target(t, names)).collect();
            out.push_str(&rendered.join(", "));
            out.push_str(" = ");
            out.push_str(&print_expr(value, names));
            out.push_str(";\n");
        }
        Stmt::Eval(expr) => {
            out.push_str(&print_expr(expr, names));
            out.push_str(";\n");
        }
        Stmt::DefineArray { array, elem_type, len } => {
            if *elem_type == 105 {
                out.push_str(&format!("def_int $array{array}("));
            } else {
                out.push_str(&format!("def_type{elem_type} $array{array}("));
            }
            out.push_str(&print_expr(len, names));
            out.push_str(");\n");
        }
        Stmt::Return(exprs) => {
            if exprs.is_empty() {
                out.push_str("return;\n");
            } else {
                let rendered: Vec<String> = exprs.iter().map(|x| print_expr(x, names)).collect();
                out.push_str(&format!("return({});\n", rendered.join(", ")));
            }
        }
        Stmt::If { cond, then_body, else_body } => {
            out.push_str(&format!("if ({}) {{\n", print_cond(cond)));
            print_stmts(out, then_body, depth + 1, names);
            indent(out, depth);
            out.push('}');
            // `else if` chains stay flat.
            if let [Stmt::If { .. }] = else_body.as_slice() {
                out.push_str(" else ");
                let mut nested = String::new();
                print_stmt(&mut nested, &else_body[0], depth, names);
                out.push_str(nested.trim_start());
            } else if else_body.is_empty() {
                out.push('\n');
            } else {
                out.push_str(" else {\n");
                print_stmts(out, else_body, depth + 1, names);
                indent(out, depth);
                out.push_str("}\n");
            }
        }
        Stmt::While { cond, body } => {
            out.push_str(&format!("while ({}) {{\n", print_cond(cond)));
            print_stmts(out, body, depth + 1, names);
            indent(out, depth);
            out.push_str("}\n");
        }
    }
}

fn print_target(t: &Target, names: &NameMaps) -> String {
    match t {
        Target::LocalInt(n) => format!("$int{n}"),
        Target::LocalStr(n) => format!("$string{n}"),
        Target::Varp(n) => format!("%varp{n}"),
        Target::Varbit(n) => format!("%varbit{n}"),
        Target::VarcInt(n) => format!("%varcint{n}"),
        Target::VarcStr(n) => format!("%varcstr{n}"),
        Target::Array { array, index } => format!("$array{array}({})", print_expr(index, names)),
        Target::DiscardInt => "$_int".to_owned(),
        Target::DiscardStr => "$_str".to_owned(),
    }
}

fn print_cond(c: &Cond) -> String {
    match c {
        Cond::Cmp { op, lhs, rhs } => format!(
            "{} {} {}",
            print_expr(lhs, &NameMaps::new()),
            cmp_sym(*op),
            print_expr(rhs, &NameMaps::new())
        ),
        Cond::And(..) | Cond::Or(..) => print_chain(c),
    }
}

/// Flatten the right spine of a same-operator chain; any other nested combinator gets
/// parens (mirrors the parser's right-associative, no-mixing grammar).
fn print_chain(c: &Cond) -> String {
    let is_and = matches!(c, Cond::And(..));
    let sym = if is_and { " & " } else { " | " };
    let mut terms: Vec<String> = Vec::new();
    let mut cur = c;
    loop {
        let (a, b) = match cur {
            Cond::And(a, b) if is_and => (a, b),
            Cond::Or(a, b) if !is_and => (a, b),
            other => {
                terms.push(print_term(other));
                break;
            }
        };
        terms.push(print_term(a));
        cur = b;
    }
    terms.join(sym)
}

fn print_term(c: &Cond) -> String {
    match c {
        Cond::Cmp { .. } => print_cond(c),
        _ => format!("({})", print_cond(c)),
    }
}

fn print_expr(e: &Expr, names: &NameMaps) -> String {
    if let Expr::Command { op, .. } = e {
        if calc_op(*op).is_some() {
            return format!("calc({})", print_calc(e, 0, false, names));
        }
    }
    print_atom(e, names)
}

fn print_calc(e: &Expr, parent_prec: u8, is_right: bool, names: &NameMaps) -> String {
    if let Expr::Command { op, args, .. } = e {
        if let Some((sym, prec)) = calc_op(*op) {
            let body = format!(
                "{} {sym} {}",
                print_calc(&args[0], prec, false, names),
                print_calc(&args[1], prec, true, names)
            );
            // Left-associative: parenthesise lower precedence anywhere, equal
            // precedence on the right.
            if prec < parent_prec || (prec == parent_prec && is_right) {
                return format!("({body})");
            }
            return body;
        }
    }
    print_atom(e, names)
}

fn print_atom(e: &Expr, names: &NameMaps) -> String {
    match e {
        Expr::ConstInt(v) => v.to_string(),
        Expr::ConstStr(s) => quote(s),
        Expr::LocalInt(n) => format!("$int{n}"),
        Expr::LocalStr(n) => format!("$string{n}"),
        Expr::Varp(n) => format!("%varp{n}"),
        Expr::Varbit(n) => format!("%varbit{n}"),
        Expr::VarcInt(n) => format!("%varcint{n}"),
        Expr::VarcStr(n) => format!("%varcstr{n}"),
        Expr::ArrayLoad { array, index } => {
            format!("$array{array}({})", print_expr(index, names))
        }
        Expr::Join(parts) => {
            let rendered: Vec<String> = parts.iter().map(|p| print_expr(p, names)).collect();
            format!("join({})", rendered.join(", "))
        }
        Expr::Gosub { script, args } => {
            let name = names
                .script_name(*script as i32)
                .map_or_else(|| format!("script_{script}"), str::to_owned);
            if args.is_empty() {
                format!("~{name}")
            } else {
                let rendered: Vec<String> = args.iter().map(|a| print_expr(a, names)).collect();
                format!("~{name}({})", rendered.join(", "))
            }
        }
        Expr::Command { op, flag, args } => {
            let name = opcode_name(*op).unwrap_or("?");
            let dot = if *flag { "." } else { "" };
            let rendered: Vec<String> = args.iter().map(|a| print_expr(a, names)).collect();
            format!("{dot}{name}({})", rendered.join(", "))
        }
    }
}

// ---------------------------------------------------------------------------------
// Lexing
// ---------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Ident(String),
    /// `$int3`, `$string0`, `$array1`, `$_int`, `$_str` (sigil `$`) or `%varp287`
    /// style (sigil `%`).
    Var(char, String),
    Num(i32),
    Str(String),
    Sym(&'static str),
}

const SYMS: &[&str] = &[
    "<=", ">=", "(", ")", "{", "}", "[", "]", ",", ";", "=", "!", "<", ">", "&", "|", "+",
    "-", "*", "/", "%", "~", ".",
];

fn lex(text: &str) -> Result<Vec<(Tok, usize)>, SourceError> {
    let mut toks = Vec::new();
    let mut chars = text.char_indices().peekable();
    let mut line = 1usize;
    let bytes: Vec<char> = text.chars().collect();
    let _ = bytes;

    while let Some(&(_, c)) = chars.peek() {
        match c {
            '\n' => {
                line += 1;
                chars.next();
            }
            c if c.is_whitespace() => {
                chars.next();
            }
            '/' => {
                // `//` comment or the division operator.
                let mut ahead = chars.clone();
                ahead.next();
                if matches!(ahead.peek(), Some(&(_, '/'))) {
                    for (_, c) in chars.by_ref() {
                        if c == '\n' {
                            line += 1;
                            break;
                        }
                    }
                } else {
                    chars.next();
                    toks.push((Tok::Sym("/"), line));
                }
            }
            '"' => {
                chars.next();
                let mut s = String::new();
                loop {
                    match chars.next() {
                        Some((_, '"')) => break,
                        Some((_, '\\')) => match chars.next() {
                            Some((_, '\\')) => s.push('\\'),
                            Some((_, '"')) => s.push('"'),
                            Some((_, 'n')) => s.push('\n'),
                            Some((_, 'r')) => s.push('\r'),
                            Some((_, 't')) => s.push('\t'),
                            other => {
                                return Err(SourceError {
                                    line,
                                    message: format!("invalid string escape {other:?}"),
                                })
                            }
                        },
                        Some((_, '\n')) | None => {
                            return Err(SourceError { line, message: "unterminated string".into() })
                        }
                        Some((_, c)) => s.push(c),
                    }
                }
                toks.push((Tok::Str(s), line));
            }
            '$' | '%' => {
                // `%` is also the modulo operator — a var only when an identifier
                // character follows directly.
                let sigil = c;
                let mut ahead = chars.clone();
                ahead.next();
                let is_var = matches!(ahead.peek(), Some(&(_, c2)) if c2.is_ascii_alphanumeric() || c2 == '_');
                if !is_var {
                    if sigil == '%' {
                        chars.next();
                        toks.push((Tok::Sym("%"), line));
                        continue;
                    }
                    return Err(SourceError { line, message: "dangling `$`".into() });
                }
                chars.next();
                let mut name = String::new();
                while let Some(&(_, c2)) = chars.peek() {
                    if c2.is_ascii_alphanumeric() || c2 == '_' {
                        name.push(c2);
                        chars.next();
                    } else {
                        break;
                    }
                }
                toks.push((Tok::Var(sigil, name), line));
            }
            c if c.is_ascii_digit() => {
                let mut n = String::new();
                while let Some(&(_, c2)) = chars.peek() {
                    if c2.is_ascii_digit() {
                        n.push(c2);
                        chars.next();
                    } else {
                        break;
                    }
                }
                let v: i32 = n
                    .parse()
                    .map_err(|_| SourceError { line, message: format!("integer out of range: {n}") })?;
                toks.push((Tok::Num(v), line));
            }
            c if c.is_ascii_alphabetic() || c == '_' => {
                let mut name = String::new();
                while let Some(&(_, c2)) = chars.peek() {
                    if c2.is_ascii_alphanumeric() || c2 == '_' {
                        name.push(c2);
                        chars.next();
                    } else {
                        break;
                    }
                }
                toks.push((Tok::Ident(name), line));
            }
            _ => {
                let rest: String = chars.clone().map(|(_, c)| c).take(2).collect();
                let sym = SYMS
                    .iter()
                    .find(|s| rest.starts_with(**s))
                    .ok_or_else(|| SourceError { line, message: format!("unexpected character {c:?}") })?;
                for _ in 0..sym.len() {
                    chars.next();
                }
                toks.push((Tok::Sym(sym), line));
            }
        }
    }
    Ok(toks)
}

// ---------------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------------

struct Parser<'a> {
    toks: Vec<(Tok, usize)>,
    pos: usize,
    names: &'a NameMaps,
    sigs: &'a BTreeMap<u32, ScriptSig>,
}

/// Parse source text back to IR. `sigs` provides gosub callee signatures (needed to
/// re-derive implicit result discards on bare call statements); pass the same table
/// the decompile side used — at pack time, build it with [`parse_signature`] over
/// every `.cs2` file first.
pub fn parse(
    text: &str,
    names: &NameMaps,
    sigs: &BTreeMap<u32, ScriptSig>,
) -> Result<ScriptIr, SourceError> {
    let toks = lex(text)?;
    let mut p = Parser { toks, pos: 0, names, sigs };
    p.script()
}

/// Read just the header of a source file — `(script id, signature)`. Used by the pack
/// side to assemble the callee-signature table before compiling any bodies.
pub fn parse_signature(
    text: &str,
    names: &NameMaps,
) -> Result<(u32, ScriptSig), SourceError> {
    let toks = lex(text)?;
    let empty = BTreeMap::new();
    let mut p = Parser { toks, pos: 0, names, sigs: &empty };
    p.header()
}

impl Parser<'_> {
    fn line(&self) -> usize {
        self.toks.get(self.pos).map_or_else(
            || self.toks.last().map_or(1, |(_, l)| *l),
            |(_, l)| *l,
        )
    }

    fn fail<T>(&self, message: impl Into<String>) -> Result<T, SourceError> {
        Err(SourceError { line: self.line(), message: message.into() })
    }

    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos).map(|(t, _)| t)
    }

    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).map(|(t, _)| t.clone());
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    fn expect_sym(&mut self, s: &str) -> Result<(), SourceError> {
        match self.next() {
            Some(Tok::Sym(got)) if got == s => Ok(()),
            got => {
                self.pos = self.pos.saturating_sub(1);
                self.fail(format!("expected `{s}`, got {got:?}"))
            }
        }
    }

    fn eat_sym(&mut self, s: &str) -> bool {
        if matches!(self.peek(), Some(Tok::Sym(got)) if *got == s) {
            self.pos += 1;
            return true;
        }
        false
    }

    fn expect_ident(&mut self) -> Result<String, SourceError> {
        match self.next() {
            Some(Tok::Ident(s)) => Ok(s),
            got => {
                self.pos = self.pos.saturating_sub(1);
                self.fail(format!("expected identifier, got {got:?}"))
            }
        }
    }

    // ----- header -----

    /// `[clientscript,name](params)(returns)` — shared by full parses and the pack
    /// side's signature pre-scan.
    fn header(&mut self) -> Result<(u32, ScriptSig), SourceError> {
        self.expect_sym("[")?;
        let trigger = self.expect_ident()?;
        if trigger != "clientscript" {
            return self.fail(format!("expected trigger `clientscript`, got `{trigger}`"));
        }
        self.expect_sym(",")?;
        let name = self.expect_ident()?;
        self.expect_sym("]")?;
        let id = self.names.script_id(&name).map(|v| v as u32).or_else(|| {
            name.strip_prefix("script_").and_then(|n| n.parse().ok())
        });
        let Some(id) = id else {
            return self.fail(format!("unknown script name `{name}`"));
        };

        // Parameters — canonical names/order enforced.
        self.expect_sym("(")?;
        let mut int_args = 0u16;
        let mut str_args = 0u16;
        while !self.eat_sym(")") {
            if int_args + str_args > 0 {
                self.expect_sym(",")?;
            }
            let ty = self.expect_ident()?;
            let var = self.next();
            match (ty.as_str(), var) {
                ("int", Some(Tok::Var('$', v))) if v == format!("int{int_args}") => int_args += 1,
                ("string", Some(Tok::Var('$', v))) if v == format!("string{str_args}") => {
                    str_args += 1;
                }
                (ty, var) => return self.fail(format!("bad parameter `{ty} {var:?}`")),
            }
        }

        // Returns.
        self.expect_sym("(")?;
        let mut int_returns = 0u16;
        let mut str_returns = 0u16;
        while !self.eat_sym(")") {
            if int_returns + str_returns > 0 {
                self.expect_sym(",")?;
            }
            match self.expect_ident()?.as_str() {
                "int" => int_returns += 1,
                "string" => str_returns += 1,
                other => return self.fail(format!("bad return type `{other}`")),
            }
        }
        Ok((id, ScriptSig { int_args, str_args, int_returns, str_returns }))
    }

    fn script(&mut self) -> Result<ScriptIr, SourceError> {
        let (id, sig) = self.header()?;
        let ScriptSig { int_args, str_args, int_returns, str_returns } = sig;

        // Optional `.name "..."` directive.
        let mut dbg_name = None;
        if matches!(self.peek(), Some(Tok::Sym("."))) {
            if let Some((Tok::Ident(w), _)) = self.toks.get(self.pos + 1) {
                if w == "name" {
                    self.pos += 2;
                    match self.next() {
                        Some(Tok::Str(s)) => dbg_name = Some(s),
                        got => return self.fail(format!("expected string after .name, got {got:?}")),
                    }
                }
            }
        }

        // Local declarations.
        let mut int_locals = int_args;
        let mut str_locals = str_args;
        loop {
            let Some(Tok::Ident(w)) = self.peek() else { break };
            let is_int = w == "def_int";
            let is_str = w == "def_string";
            if !is_int && !is_str {
                break;
            }
            // `def_int $array0(...)` is an array-definition *statement* — stop here.
            if let Some((Tok::Var('$', v), _)) = self.toks.get(self.pos + 1) {
                if v.starts_with("array") {
                    break;
                }
            }
            self.pos += 1;
            match self.next() {
                Some(Tok::Var('$', v)) if is_int && v == format!("int{int_locals}") => {
                    int_locals += 1;
                }
                Some(Tok::Var('$', v)) if is_str && v == format!("string{str_locals}") => {
                    str_locals += 1;
                }
                got => return self.fail(format!("bad local declaration {got:?}")),
            }
            self.expect_sym(";")?;
        }

        let mut body = Vec::new();
        while self.peek().is_some() {
            body.push(self.stmt()?);
        }

        Ok(ScriptIr {
            id,
            name: dbg_name,
            int_args,
            str_args,
            int_locals,
            str_locals,
            int_returns,
            str_returns,
            body,
        })
    }

    // ----- statements -----

    fn block(&mut self) -> Result<Vec<Stmt>, SourceError> {
        self.expect_sym("{")?;
        let mut body = Vec::new();
        while !self.eat_sym("}") {
            if self.peek().is_none() {
                return self.fail("unterminated block");
            }
            body.push(self.stmt()?);
        }
        Ok(body)
    }

    fn stmt(&mut self) -> Result<Stmt, SourceError> {
        match self.peek() {
            Some(Tok::Ident(w)) if w == "if" => self.if_stmt(),
            Some(Tok::Ident(w)) if w == "while" => {
                self.pos += 1;
                self.expect_sym("(")?;
                let cond = self.cond()?;
                self.expect_sym(")")?;
                let body = self.block()?;
                Ok(Stmt::While { cond, body })
            }
            Some(Tok::Ident(w)) if w == "return" => {
                self.pos += 1;
                let mut exprs = Vec::new();
                if self.eat_sym("(") {
                    while !self.eat_sym(")") {
                        if !exprs.is_empty() {
                            self.expect_sym(",")?;
                        }
                        exprs.push(self.expr()?);
                    }
                }
                self.expect_sym(";")?;
                Ok(Stmt::Return(exprs))
            }
            Some(Tok::Ident(w)) if w.starts_with("def_") => self.def_stmt(),
            Some(Tok::Var(..)) => self.assign_stmt(),
            _ => {
                let value = self.expr()?;
                self.expect_sym(";")?;
                self.bare_stmt(value)
            }
        }
    }

    fn if_stmt(&mut self) -> Result<Stmt, SourceError> {
        self.pos += 1; // `if`
        self.expect_sym("(")?;
        let cond = self.cond()?;
        self.expect_sym(")")?;
        let then_body = self.block()?;
        let mut else_body = Vec::new();
        if matches!(self.peek(), Some(Tok::Ident(w)) if w == "else") {
            self.pos += 1;
            if matches!(self.peek(), Some(Tok::Ident(w)) if w == "if") {
                else_body.push(self.if_stmt()?);
            } else {
                else_body = self.block()?;
            }
        }
        Ok(Stmt::If { cond, then_body, else_body })
    }

    fn def_stmt(&mut self) -> Result<Stmt, SourceError> {
        let Some(Tok::Ident(kw)) = self.next() else { unreachable!("checked by stmt()") };
        let elem_type = match kw.as_str() {
            "def_int" => 105u8,
            other => match other.strip_prefix("def_type").and_then(|n| n.parse::<u8>().ok()) {
                Some(t) => t,
                None => return self.fail(format!("unknown definition keyword `{other}`")),
            },
        };
        let array = match self.next() {
            Some(Tok::Var('$', v)) => match v.strip_prefix("array").and_then(|n| n.parse::<u8>().ok()) {
                Some(a) => a,
                None => return self.fail(format!("expected $array<N>, got ${v}")),
            },
            got => return self.fail(format!("expected $array<N>, got {got:?}")),
        };
        self.expect_sym("(")?;
        let len = self.expr()?;
        self.expect_sym(")")?;
        self.expect_sym(";")?;
        Ok(Stmt::DefineArray { array, elem_type, len })
    }

    fn assign_stmt(&mut self) -> Result<Stmt, SourceError> {
        let mut targets = vec![self.target()?];
        while self.eat_sym(",") {
            targets.push(self.target()?);
        }
        self.expect_sym("=")?;
        let value = self.expr()?;
        self.expect_sym(";")?;
        Ok(Stmt::Assign { targets, value })
    }

    fn target(&mut self) -> Result<Target, SourceError> {
        match self.next() {
            Some(Tok::Var(sigil, v)) => {
                if let Some(t) = self.simple_target(sigil, &v) {
                    return Ok(t);
                }
                if sigil == '$' {
                    if let Some(a) = v.strip_prefix("array").and_then(|n| n.parse::<u8>().ok()) {
                        self.expect_sym("(")?;
                        let index = self.expr()?;
                        self.expect_sym(")")?;
                        return Ok(Target::Array { array: a, index });
                    }
                }
                self.fail(format!("unknown assignment target {sigil}{v}"))
            }
            got => self.fail(format!("expected assignment target, got {got:?}")),
        }
    }

    fn simple_target(&self, sigil: char, v: &str) -> Option<Target> {
        let num = |p: &str| v.strip_prefix(p).and_then(|n| n.parse::<u16>().ok());
        match sigil {
            '$' => {
                if v == "_int" {
                    return Some(Target::DiscardInt);
                }
                if v == "_str" {
                    return Some(Target::DiscardStr);
                }
                // `int...` must check before `string...`? They don't overlap.
                if let Some(n) = num("int") {
                    return Some(Target::LocalInt(n));
                }
                if let Some(n) = num("string") {
                    return Some(Target::LocalStr(n));
                }
                None
            }
            '%' => {
                if let Some(n) = num("varbit") {
                    return Some(Target::Varbit(n));
                }
                if let Some(n) = num("varcint") {
                    return Some(Target::VarcInt(n));
                }
                if let Some(n) = num("varcstr") {
                    return Some(Target::VarcStr(n));
                }
                if let Some(n) = num("varp") {
                    return Some(Target::Varp(n));
                }
                None
            }
            _ => None,
        }
    }

    /// `expr;` — re-derive the implicit discards (or none) from what the expression
    /// pushes, mirroring how the lifter classified the original drop opcodes.
    fn bare_stmt(&self, value: Expr) -> Result<Stmt, SourceError> {
        let (ipush, spush) = self.expr_pushes(&value)?;
        if ipush + spush == 0 {
            return Ok(Stmt::Eval(value));
        }
        let mut targets = Vec::with_capacity(ipush + spush);
        targets.extend(std::iter::repeat_n(Target::DiscardInt, ipush));
        targets.extend(std::iter::repeat_n(Target::DiscardStr, spush));
        Ok(Stmt::Assign { targets, value })
    }

    fn expr_pushes(&self, e: &Expr) -> Result<(usize, usize), SourceError> {
        Ok(match e {
            Expr::Gosub { script, .. } => {
                let Some(sig) = self.sigs.get(script) else {
                    return self.fail(format!("unknown callee signature for script {script}"));
                };
                (sig.int_returns as usize, sig.str_returns as usize)
            }
            Expr::Command { op, args, .. } => match arity(*op) {
                Some(Arity::Fixed { ipush, spush, .. }) => (ipush as usize, spush as usize),
                Some(Arity::EventHandler { .. }) => (0, 0),
                Some(Arity::EnumLookup) => match args.get(1) {
                    Some(Expr::ConstInt(t)) if *t == i32::from(b's') => (0, 1),
                    _ => (1, 0),
                },
                _ => return self.fail(format!("cannot derive pushes for op {op}")),
            },
            Expr::ConstStr(_) | Expr::LocalStr(_) | Expr::VarcStr(_) | Expr::Join(_) => (0, 1),
            _ => (1, 0),
        })
    }

    // ----- conditions -----

    fn cond(&mut self) -> Result<Cond, SourceError> {
        let first = self.cond_atom()?;
        let join = match self.peek() {
            Some(Tok::Sym("&")) => "&",
            Some(Tok::Sym("|")) => "|",
            _ => return Ok(first),
        };
        let mut terms = vec![first];
        while self.eat_sym(join) {
            terms.push(self.cond_atom()?);
        }
        // No mixing without parens.
        if matches!(self.peek(), Some(Tok::Sym("&")) | Some(Tok::Sym("|"))) {
            return self.fail("cannot mix `&` and `|` without parentheses");
        }
        // Right-associative fold — matches the lifter's chain shape.
        let mut cond = terms.pop().expect("at least one term");
        while let Some(t) = terms.pop() {
            cond = if join == "&" {
                Cond::And(Box::new(t), Box::new(cond))
            } else {
                Cond::Or(Box::new(t), Box::new(cond))
            };
        }
        Ok(cond)
    }

    fn cond_atom(&mut self) -> Result<Cond, SourceError> {
        if self.eat_sym("(") {
            let inner = self.cond()?;
            self.expect_sym(")")?;
            return Ok(inner);
        }
        let lhs = self.expr()?;
        let op = match self.next() {
            Some(Tok::Sym(s)) => match cmp_op_by_sym(s) {
                Some(op) => op,
                None => {
                    self.pos -= 1;
                    return self.fail(format!("expected comparison operator, got `{s}`"));
                }
            },
            got => {
                self.pos = self.pos.saturating_sub(1);
                return self.fail(format!("expected comparison operator, got {got:?}"));
            }
        };
        let rhs = self.expr()?;
        Ok(Cond::Cmp { op, lhs, rhs })
    }

    // ----- expressions -----

    fn expr(&mut self) -> Result<Expr, SourceError> {
        match self.peek().cloned() {
            Some(Tok::Num(_)) | Some(Tok::Sym("-")) => {
                let neg = self.eat_sym("-");
                match self.next() {
                    Some(Tok::Num(v)) => Ok(Expr::ConstInt(if neg { v.wrapping_neg() } else { v })),
                    got => self.fail(format!("expected number, got {got:?}")),
                }
            }
            Some(Tok::Str(_)) => {
                let Some(Tok::Str(s)) = self.next() else { unreachable!() };
                Ok(Expr::ConstStr(s))
            }
            Some(Tok::Var(sigil, v)) => {
                self.pos += 1;
                if let Some(t) = self.simple_target(sigil, &v) {
                    return match t {
                        Target::LocalInt(n) => Ok(Expr::LocalInt(n)),
                        Target::LocalStr(n) => Ok(Expr::LocalStr(n)),
                        Target::Varp(n) => Ok(Expr::Varp(n)),
                        Target::Varbit(n) => Ok(Expr::Varbit(n)),
                        Target::VarcInt(n) => Ok(Expr::VarcInt(n)),
                        Target::VarcStr(n) => Ok(Expr::VarcStr(n)),
                        _ => self.fail(format!("`{sigil}{v}` is not a value")),
                    };
                }
                if sigil == '$' {
                    if let Some(a) = v.strip_prefix("array").and_then(|n| n.parse::<u8>().ok()) {
                        self.expect_sym("(")?;
                        let index = self.expr()?;
                        self.expect_sym(")")?;
                        return Ok(Expr::ArrayLoad { array: a, index: Box::new(index) });
                    }
                }
                self.fail(format!("unknown variable {sigil}{v}"))
            }
            Some(Tok::Sym("~")) => {
                self.pos += 1;
                let name = self.expect_ident()?;
                let script = self
                    .names
                    .script_id(&name)
                    .map(|v| v as u32)
                    .or_else(|| name.strip_prefix("script_").and_then(|n| n.parse().ok()));
                let Some(script) = script else {
                    return self.fail(format!("unknown script name `{name}`"));
                };
                let args = if self.eat_sym("(") { self.args()? } else { Vec::new() };
                Ok(Expr::Gosub { script, args })
            }
            Some(Tok::Sym(".")) => {
                self.pos += 1;
                let name = self.expect_ident()?;
                self.command(&name, true)
            }
            Some(Tok::Ident(name)) => {
                self.pos += 1;
                match name.as_str() {
                    "calc" => {
                        self.expect_sym("(")?;
                        let e = self.calc_expr(0)?;
                        self.expect_sym(")")?;
                        Ok(e)
                    }
                    "join" => {
                        self.expect_sym("(")?;
                        Ok(Expr::Join(self.args()?))
                    }
                    _ => self.command(&name, false),
                }
            }
            got => self.fail(format!("expected expression, got {got:?}")),
        }
    }

    fn command(&mut self, name: &str, flag: bool) -> Result<Expr, SourceError> {
        let Some(op) = opcode_by_name(name) else {
            return self.fail(format!("unknown command `{name}`"));
        };
        self.expect_sym("(")?;
        let args = self.args()?;
        Ok(Expr::Command { op, flag, args })
    }

    fn args(&mut self) -> Result<Vec<Expr>, SourceError> {
        let mut args = Vec::new();
        while !self.eat_sym(")") {
            if !args.is_empty() {
                self.expect_sym(",")?;
            }
            args.push(self.expr()?);
        }
        Ok(args)
    }

    /// Precedence-climbing infix parser, active only inside `calc(...)`.
    fn calc_expr(&mut self, min_prec: u8) -> Result<Expr, SourceError> {
        let mut lhs = if self.eat_sym("(") {
            let inner = self.calc_expr(0)?;
            self.expect_sym(")")?;
            inner
        } else {
            self.expr()?
        };
        loop {
            let Some(Tok::Sym(s)) = self.peek() else { break };
            let Some((op, prec)) = calc_op_by_sym(s) else { break };
            if prec < min_prec {
                break;
            }
            self.pos += 1;
            let rhs = self.calc_rhs(prec + 1)?;
            lhs = Expr::Command { op, flag: false, args: vec![lhs, rhs] };
        }
        Ok(lhs)
    }

    fn calc_rhs(&mut self, min_prec: u8) -> Result<Expr, SourceError> {
        self.calc_expr(min_prec)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cs2_ir::{Cond, Expr, Stmt, Target};

    fn sample_ir() -> ScriptIr {
        ScriptIr {
            id: 7,
            name: None,
            int_args: 1,
            str_args: 0,
            int_locals: 2,
            str_locals: 1,
            int_returns: 1,
            str_returns: 0,
            body: vec![
                Stmt::If {
                    cond: Cond::Or(
                        Box::new(Cond::Cmp {
                            op: 9,
                            lhs: Expr::Varbit(3756),
                            rhs: Expr::ConstInt(1),
                        }),
                        Box::new(Cond::Cmp {
                            op: 10,
                            lhs: Expr::Varbit(3756),
                            rhs: Expr::ConstInt(14),
                        }),
                    ),
                    then_body: vec![Stmt::Assign {
                        targets: vec![Target::LocalInt(1)],
                        value: Expr::Command {
                            op: 4000,
                            flag: false,
                            args: vec![
                                Expr::LocalInt(0),
                                Expr::Command {
                                    op: 4002,
                                    flag: false,
                                    args: vec![Expr::ConstInt(2), Expr::ConstInt(3)],
                                },
                            ],
                        },
                    }],
                    else_body: vec![Stmt::Eval(Expr::Command {
                        op: 3100,
                        flag: false,
                        args: vec![Expr::Join(vec![
                            Expr::ConstStr("a\"b".into()),
                            Expr::LocalStr(0),
                        ])],
                    })],
                },
                Stmt::Return(vec![Expr::ConstInt(-1)]),
            ],
        }
    }

    #[test]
    fn print_parse_round_trips_ir() {
        let ir = sample_ir();
        let names = NameMaps::new();
        let sigs = BTreeMap::new();
        let text = print(&ir, &names);
        let back = parse(&text, &names, &sigs).unwrap_or_else(|e| panic!("{e}\n{text}"));
        assert_eq!(back, ir, "round-trip mismatch:\n{text}");
    }

    #[test]
    fn calc_parens_reconstruct_exact_tree() {
        // sub(add(a, b), c) → calc($int0 + $int1 - 1) ; add(a, sub(b, c)) needs parens.
        let left = Expr::Command {
            op: 4001,
            flag: false,
            args: vec![
                Expr::Command {
                    op: 4000,
                    flag: false,
                    args: vec![Expr::LocalInt(0), Expr::LocalInt(1)],
                },
                Expr::ConstInt(1),
            ],
        };
        let right = Expr::Command {
            op: 4000,
            flag: false,
            args: vec![
                Expr::LocalInt(0),
                Expr::Command {
                    op: 4001,
                    flag: false,
                    args: vec![Expr::LocalInt(1), Expr::ConstInt(1)],
                },
            ],
        };
        for ir_expr in [left, right] {
            let ir = ScriptIr {
                id: 0,
                name: None,
                int_args: 2,
                str_args: 0,
                int_locals: 3,
                str_locals: 0,
                int_returns: 0,
                str_returns: 0,
                body: vec![Stmt::Assign {
                    targets: vec![Target::LocalInt(2)],
                    value: ir_expr,
                }],
            };
            let names = NameMaps::new();
            let text = print(&ir, &names);
            let back = parse(&text, &names, &BTreeMap::new()).unwrap_or_else(|e| panic!("{e}\n{text}"));
            assert_eq!(back, ir, "calc round-trip mismatch:\n{text}");
        }
    }

    #[test]
    fn bare_gosub_rederives_discards() {
        let mut sigs = BTreeMap::new();
        sigs.insert(
            113u32,
            crate::cs2_sig::ScriptSig { int_args: 0, str_args: 0, int_returns: 1, str_returns: 0 },
        );
        let ir = ScriptIr {
            id: 0,
            name: None,
            int_args: 0,
            str_args: 0,
            int_locals: 0,
            str_locals: 0,
            int_returns: 0,
            str_returns: 0,
            body: vec![
                Stmt::Assign {
                    targets: vec![Target::DiscardInt],
                    value: Expr::Gosub { script: 113, args: vec![] },
                },
                Stmt::Return(vec![]),
            ],
        };
        let names = NameMaps::new();
        let text = print(&ir, &names);
        let back = parse(&text, &names, &sigs).unwrap_or_else(|e| panic!("{e}\n{text}"));
        assert_eq!(back, ir, "{text}");
    }
}
