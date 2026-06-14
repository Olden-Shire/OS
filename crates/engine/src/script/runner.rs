//! Script executor — mirrors the Engine-TS reference ScriptRunner.ts.
//! One dispatch loop with the op handlers inlined per section
//! (CoreOps / NumberOps / StringOps / ServerOps / PlayerOps / NpcOps
//! in the reference). Entity-dependent ops resolve their handles
//! through &mut World.

use crate::dbg_log;
use crate::entity::{npc, player};
use crate::script::opcode as op;
use crate::script::state::{Execution, Pointer, ScriptArg, ScriptState};
use crate::world::World;

use protocol::server as msg;

/// Run `state` until it finishes, suspends, or errors. Errors print a
/// stack backtrace (script name + line numbers) like the reference
/// and report to the player when one is attached.
/// Cross-cutting accumulator (nanoseconds) of time spent executing RuneScript
/// this cycle. Reset at the head of [`World::cycle`] and read at the tail, so
/// the control panel can show RuneScript as its own slice even though scripts
/// run interleaved inside the world/npc/player phases.
pub static SCRIPT_NS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn execute(state: &mut ScriptState, world: &mut World) -> Execution {
    state.execution = Execution::Running;

    let t0 = std::time::Instant::now();
    let result = run(state, world);
    SCRIPT_NS.fetch_add(
        t0.elapsed().as_nanos() as u64,
        std::sync::atomic::Ordering::Relaxed,
    );
    if let Err(err) = result {
        let line = state.script.line_number(state.pc);
        eprintln!("script error: {err}");
        eprintln!("file: {}", state.script.file_name());
        eprintln!("stack backtrace:");
        eprintln!("    1: {} - {}:{line}", state.script.name(), state.script.file_name());
        let mut trace = 1;
        for (script, pc) in state.debug_frames.iter().rev() {
            trace += 1;
            eprintln!("    {trace}: {} - {}:{}", script.name(), script.file_name(),
                      script.line_number(*pc));
        }

        if let Some(pid) = state.active_player {
            if let Some(p) = world.players.get_mut(pid).and_then(|o| o.as_mut()) {
                p.write(msg::message_game(&format!("script error: {err}")));
            }
        }

        state.execution = Execution::Aborted;
    }

    state.execution
}

fn run(state: &mut ScriptState, world: &mut World) -> Result<(), String> {
    while state.execution == Execution::Running {
        if state.opcount > 500_000 {
            return Err("too many instructions".to_string());
        }
        state.opcount += 1;

        state.pc += 1;
        let pc = state.pc as usize;
        if pc >= state.script.opcodes.len() {
            return Err(format!("invalid program counter: {}", state.pc));
        }
        let opcode = state.script.opcodes[pc];
        step(state, world, opcode)
            .map_err(|e| format!("{} {e}", op::name(opcode)))?;
    }
    Ok(())
}

/// Java `String.compareTo` — 1:1 with Engine-TS `javaStringCompare`. Returns the
/// char-code difference at the first mismatch (unclamped), or the length difference
/// when one string is a prefix of the other. OSRS strings are CP1252/BMP, so a
/// `char`-wise comparison matches Java's UTF-16 code-unit comparison.
fn java_string_compare(a: &str, b: &str) -> i32 {
    for (ca, cb) in a.chars().zip(b.chars()) {
        if ca != cb {
            return ca as i32 - cb as i32;
        }
    }
    a.chars().count() as i32 - b.chars().count() as i32
}

fn active_player<'w>(state: &ScriptState, world: &'w mut World)
    -> Result<&'w mut player::Player, String>
{
    let pid = if state.secondary() { state.active_player2 } else { state.active_player };
    let pid = pid.ok_or("no active_player")?;
    world.players.get_mut(pid).and_then(|o| o.as_mut())
        .ok_or_else(|| "active_player slot empty".to_string())
}

fn active_npc<'w>(state: &ScriptState, world: &'w mut World)
    -> Result<&'w mut npc::Npc, String>
{
    let nid = if state.secondary() { state.active_npc2 } else { state.active_npc };
    let nid = nid.ok_or("no active_npc")?;
    world.npcs.get_mut(nid).and_then(|o| o.as_mut())
        .ok_or_else(|| "active_npc slot empty".to_string())
}

/// Pop a typed argument list described by a leading typespec string — 1:1 with
/// Engine-TS `popScriptArgs`. The typespec (one char per arg, 's' = string) is
/// on top of the string stack; values are then popped in reverse so the result
/// is in forward order.
fn pop_script_args(state: &mut ScriptState) -> Vec<crate::script::state::ScriptArg> {
    use crate::script::state::ScriptArg;
    let types = state.pop_string();
    let mut args: Vec<ScriptArg> = vec![ScriptArg::Int(0); types.len()];
    for (i, ch) in types.char_indices().rev() {
        args[i] = if ch == 's' {
            ScriptArg::Str(state.pop_string())
        } else {
            ScriptArg::Int(state.pop_int())
        };
    }
    args
}

fn step(state: &mut ScriptState, world: &mut World, opcode: u16) -> Result<(), String> {
    match opcode {
        // ── Core language ─────────────────────────────────────────
        op::PUSH_CONSTANT_INT => {
            let v = state.int_operand();
            state.push_int(v);
        }
        op::PUSH_CONSTANT_STRING => {
            let s = state.string_operand().to_string();
            state.push_string(s);
        }
        op::PUSH_INT_LOCAL => {
            let v = state.int_locals[state.int_operand() as usize];
            state.push_int(v);
        }
        op::POP_INT_LOCAL => {
            let idx = state.int_operand() as usize;
            let v = state.pop_int();
            state.int_locals[idx] = v;
        }
        op::PUSH_STRING_LOCAL => {
            let s = state.string_locals[state.int_operand() as usize].clone();
            state.push_string(s);
        }
        op::POP_STRING_LOCAL => {
            let idx = state.int_operand() as usize;
            let s = state.pop_string();
            state.string_locals[idx] = s;
        }
        op::BRANCH => {
            state.pc += state.int_operand();
        }
        op::BRANCH_NOT => {
            let b = state.pop_int();
            let a = state.pop_int();
            if a != b {
                state.pc += state.int_operand();
            }
        }
        op::BRANCH_EQUALS => {
            let b = state.pop_int();
            let a = state.pop_int();
            if a == b {
                state.pc += state.int_operand();
            }
        }
        op::BRANCH_LESS_THAN => {
            let b = state.pop_int();
            let a = state.pop_int();
            if a < b {
                state.pc += state.int_operand();
            }
        }
        op::BRANCH_GREATER_THAN => {
            let b = state.pop_int();
            let a = state.pop_int();
            if a > b {
                state.pc += state.int_operand();
            }
        }
        op::BRANCH_LESS_THAN_OR_EQUALS => {
            let b = state.pop_int();
            let a = state.pop_int();
            if a <= b {
                state.pc += state.int_operand();
            }
        }
        op::BRANCH_GREATER_THAN_OR_EQUALS => {
            let b = state.pop_int();
            let a = state.pop_int();
            if a >= b {
                state.pc += state.int_operand();
            }
        }
        op::POP_INT_DISCARD => {
            state.pop_int();
        }
        op::POP_STRING_DISCARD => {
            state.pop_string();
        }
        op::RETURN => {
            if state.frames.is_empty() {
                state.execution = Execution::Finished;
            } else {
                state.pop_frame();
            }
        }
        op::JOIN_STRING => {
            let count = state.int_operand() as usize;
            let parts = state.pop_strings(count);
            state.push_string(parts.concat());
        }
        op::GOSUB => {
            if state.frames.len() >= 50 {
                return Err("stack overflow".to_string());
            }
            let id = state.pop_int();
            let proc = world.scripts.as_ref()
                .and_then(|s| s.get(id))
                .ok_or_else(|| format!("unable to find proc {id}"))?;
            state.gosub_frame(proc);
        }
        op::GOSUB_WITH_PARAMS => {
            if state.frames.len() >= 50 {
                return Err("stack overflow".to_string());
            }
            let id = state.int_operand();
            let proc = world.scripts.as_ref()
                .and_then(|s| s.get(id))
                .ok_or_else(|| format!("unable to find proc {id}"))?;
            state.gosub_frame(proc);
        }
        op::JUMP => {
            let id = state.pop_int();
            let label = world.scripts.as_ref()
                .and_then(|s| s.get(id))
                .ok_or_else(|| format!("unable to find label {id}"))?;
            state.goto_frame(label);
        }
        op::JUMP_WITH_PARAMS => {
            let id = state.int_operand();
            let label = world.scripts.as_ref()
                .and_then(|s| s.get(id))
                .ok_or_else(|| format!("unable to find label {id}"))?;
            state.goto_frame(label);
        }
        op::SWITCH => {
            let key = state.pop_int();
            let table = state.script.switch_tables
                .get(state.int_operand() as usize);
            if let Some(table) = table {
                if let Some(&offset) = table.get(&key) {
                    state.pc += offset;
                }
            }
        }
        op::DEFINE_ARRAY | op::PUSH_ARRAY_INT | op::POP_ARRAY_INT => {
            // Unimplemented in the reference server runtime too.
            return Err("unimplemented".to_string());
        }

        // ── Vars ──────────────────────────────────────────────────
        op::PUSH_VARP => {
            // Operand packs (secondary << 16) | varp id.
            let operand = state.int_operand();
            let secondary = (operand >> 16) & 0x1 == 1;
            let varp = (operand & 0xffff) as usize;
            let pid = if secondary { state.active_player2 } else { state.active_player };
            let pid = pid.ok_or("no active_player")?;
            let p = world.players.get(pid).and_then(|o| o.as_ref())
                .ok_or("active_player slot empty")?;
            let v = p.varps.get(varp).copied().unwrap_or(0);
            state.push_int(v);
        }
        op::POP_VARP => {
            let operand = state.int_operand();
            let secondary = (operand >> 16) & 0x1 == 1;
            let varp = (operand & 0xffff) as usize;
            let pid = if secondary { state.active_player2 } else { state.active_player };
            let pid = pid.ok_or("no active_player")?;
            let v = state.pop_int();
            let p = world.players.get_mut(pid).and_then(|o| o.as_mut())
                .ok_or("active_player slot empty")?;
            // set_var stores the value *and* syncs it to the client.
            p.set_var(varp, v);
        }
        op::PUSH_VARS => {
            let id = (state.int_operand() & 0xffff) as usize;
            let v = world.vars.get(id).copied().unwrap_or(0);
            state.push_int(v);
        }
        op::POP_VARS => {
            let id = (state.int_operand() & 0xffff) as usize;
            let v = state.pop_int();
            if id < world.vars.len() {
                world.vars[id] = v;
            }
        }
        op::PUSH_VARN | op::POP_VARN | op::PUSH_VARBIT | op::POP_VARBIT => {
            // varn needs npc var configs; varbit needs varbit configs —
            // both land with the config-type loaders.
            return Err("unimplemented (needs config types)".to_string());
        }

        // ── Number ops ────────────────────────────────────────────
        op::ADD => {
            let [a, b] = state.pop_ints::<2>();
            state.push_int(a.wrapping_add(b));
        }
        op::SUB => {
            let [a, b] = state.pop_ints::<2>();
            state.push_int(a.wrapping_sub(b));
        }
        op::MULTIPLY => {
            let [a, b] = state.pop_ints::<2>();
            state.push_int(a.wrapping_mul(b));
        }
        op::DIVIDE => {
            let [a, b] = state.pop_ints::<2>();
            // Engine-TS divides in JS doubles then ToInt32s the result, so a / 0
            // is ±Infinity (0/0 is NaN) → ToInt32 → 0, with no script abort. Match
            // that rather than erroring. (wrapping_div also covers the i32::MIN/-1
            // overflow, matching ToInt32 of the double 2^31.)
            state.push_int(if b == 0 { 0 } else { a.wrapping_div(b) });
        }
        op::MODULO => {
            let [a, b] = state.pop_ints::<2>();
            // Engine-TS: n1 % 0 is NaN → ToInt32 → 0 (no abort).
            state.push_int(if b == 0 { 0 } else { a.wrapping_rem(b) });
        }
        op::RANDOM => {
            let n = state.pop_int();
            state.push_int(if n <= 0 { 0 } else { (rand_unit() * n as f64) as i32 });
        }
        op::RANDOMINC => {
            let n = state.pop_int();
            state.push_int(if n < 0 { 0 } else { (rand_unit() * (n as f64 + 1.0)) as i32 });
        }
        op::INTERPOLATE => {
            let [y0, y1, x0, x1, x] = state.pop_ints::<5>();
            // Engine-TS divides the slope in JS doubles; x1 == x0 makes it
            // ±Infinity/NaN and the whole expression ToInt32s to 0 — no abort.
            if x1 == x0 {
                state.push_int(0);
            } else {
                let lerp = ((y1 - y0) as f64 / (x1 - x0) as f64).floor() as i32 * (x - x0) + y0;
                state.push_int(lerp);
            }
        }
        op::ADDPERCENT => {
            let [num, percent] = state.pop_ints::<2>();
            state.push_int(((num as i64 * percent as i64) / 100) as i32 + num);
        }
        op::SETBIT => {
            let [value, bit] = state.pop_ints::<2>();
            state.push_int(value | 1i32.wrapping_shl(bit as u32));
        }
        op::CLEARBIT => {
            let [value, bit] = state.pop_ints::<2>();
            state.push_int(value & !1i32.wrapping_shl(bit as u32));
        }
        op::TESTBIT => {
            let [value, bit] = state.pop_ints::<2>();
            state.push_int(i32::from(value & 1i32.wrapping_shl(bit as u32) != 0));
        }
        op::POW => {
            let [base, exp] = state.pop_ints::<2>();
            state.push_int((base as f64).powi(exp) as i32);
        }
        op::INVPOW => {
            let [n1, n2] = state.pop_ints::<2>();
            if n1 == 0 || n2 == 0 {
                state.push_int(0);
            } else {
                state.push_int(match n2 {
                    1 => n1,
                    2 => (n1 as f64).sqrt() as i32,
                    3 => (n1 as f64).cbrt() as i32,
                    4 => (n1 as f64).sqrt().sqrt() as i32,
                    _ => (n1 as f64).powf(1.0 / n2 as f64) as i32,
                });
            }
        }
        op::AND => {
            let [a, b] = state.pop_ints::<2>();
            state.push_int(a & b);
        }
        op::OR => {
            let [a, b] = state.pop_ints::<2>();
            state.push_int(a | b);
        }
        op::MIN => {
            let [a, b] = state.pop_ints::<2>();
            state.push_int(a.min(b));
        }
        op::MAX => {
            let [a, b] = state.pop_ints::<2>();
            state.push_int(a.max(b));
        }
        op::SCALE => {
            let [a, b, c] = state.pop_ints::<3>();
            // Engine-TS: (a*c)/0 → ±Infinity/NaN → ToInt32 → 0 (no abort).
            if b == 0 {
                state.push_int(0);
            } else {
                state.push_int(((a as i64 * c as i64) / b as i64) as i32);
            }
        }
        op::ABS => {
            let n = state.pop_int();
            state.push_int(n.wrapping_abs());
        }
        op::BITCOUNT => {
            let n = state.pop_int();
            state.push_int(n.count_ones() as i32);
        }
        op::TOGGLEBIT => {
            let [value, bit] = state.pop_ints::<2>();
            state.push_int(value ^ 1i32.wrapping_shl(bit as u32));
        }
        op::SETBIT_RANGE => {
            // Engine-TS setBitRange: OR a run of set bits [start..=end] into num.
            let [num, start_bit, end_bit] = state.pop_ints::<3>();
            let mask = bit_mask(end_bit - start_bit + 1);
            state.push_int(num | mask.wrapping_shl(start_bit as u32));
        }
        op::CLEARBIT_RANGE => {
            // Engine-TS clearBitRange: zero the bits [start..=end] in num.
            let [num, start_bit, end_bit] = state.pop_ints::<3>();
            let mask = bit_mask(end_bit - start_bit + 1);
            state.push_int(num & !mask.wrapping_shl(start_bit as u32));
        }
        op::GETBIT_RANGE => {
            // Engine-TS GETBIT_RANGE: extract bits [start..=end] of num as an
            // unsigned field. `a = 31 - end` left-aligns the field to bit 31,
            // then a logical (`>>>`) right shift drops the lower bits.
            let [num, start_bit, end_bit] = state.pop_ints::<3>();
            let a = 31 - end_bit;
            let shifted = num.wrapping_shl(a as u32) as u32;
            state.push_int(shifted.wrapping_shr((start_bit + a) as u32) as i32);
        }
        op::SETBIT_RANGE_TOINT => {
            // Engine-TS SETBIT_RANGE_TOINT: clear bits [start..=end] then write
            // `value` there, saturating at the field's max so it can't spill.
            let [num, value, start_bit, end_bit] = state.pop_ints::<4>();
            let mask = bit_mask(end_bit - start_bit + 1);
            let cleared = num & !mask.wrapping_shl(start_bit as u32);
            let assign = if value > mask { mask } else { value };
            state.push_int(cleared | assign.wrapping_shl(start_bit as u32));
        }
        op::SIN_DEG | op::COS_DEG | op::ATAN2_DEG
        | op::DATE_MINUTES | op::DATE_RUNEDAY => {
            return Err("unimplemented".to_string());
        }

        // ── String ops ────────────────────────────────────────────
        op::APPEND_NUM => {
            let text = state.pop_string();
            let num = state.pop_int();
            state.push_string(format!("{text}{num}"));
        }
        op::APPEND => {
            let parts = state.pop_strings(2);
            state.push_string(format!("{}{}", parts[0], parts[1]));
        }
        op::APPEND_SIGNNUM => {
            let text = state.pop_string();
            let num = state.pop_int();
            if num >= 0 {
                state.push_string(format!("{text}+{num}"));
            } else {
                state.push_string(format!("{text}{num}"));
            }
        }
        op::LOWERCASE => {
            let s = state.pop_string().to_lowercase();
            state.push_string(s);
        }
        op::TOSTRING => {
            let n = state.pop_int();
            state.push_string(n.to_string());
        }
        op::COMPARE => {
            let parts = state.pop_strings(2);
            // Engine-TS COMPARE is Java `String.compareTo`: the *unclamped*
            // char-code difference at the first mismatch (or the length difference
            // when one string is a prefix), not a -1/0/1 sign.
            state.push_int(java_string_compare(&parts[0], &parts[1]));
        }
        op::TEXT_SWITCH => {
            let parts = state.pop_strings(2);
            let condition = state.pop_int();
            state.push_string(if condition == 1 {
                parts[0].clone()
            } else {
                parts[1].clone()
            });
        }
        op::TEXT_GENDER => {
            let parts = state.pop_strings(2);
            let p = active_player(state, world)?;
            let gender = p.gender;
            state.push_string(if gender == 0 {
                parts[0].clone()
            } else {
                parts[1].clone()
            });
        }
        op::APPEND_CHAR => {
            let text = state.pop_string();
            let ch = state.pop_int();
            let c = char::from_u32(ch as u32).unwrap_or('?');
            state.push_string(format!("{text}{c}"));
        }
        op::STRING_LENGTH => {
            let s = state.pop_string();
            state.push_int(s.chars().count() as i32);
        }
        op::SUBSTRING => {
            let s = state.pop_string();
            let [start, end] = state.pop_ints::<2>();
            // JS `String.substring(start, end)`: clamp each index to [0, len]
            // independently, then swap them when start > end. OS previously took
            // `end - start` chars, returning empty for a reversed range instead.
            let len = s.chars().count() as i32;
            let a = start.clamp(0, len);
            let b = end.clamp(0, len);
            let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
            let sub: String = s.chars().skip(lo as usize).take((hi - lo) as usize).collect();
            state.push_string(sub);
        }
        op::STRING_INDEXOF_CHAR => {
            let s = state.pop_string();
            let ch = state.pop_int();
            let c = char::from_u32(ch as u32);
            state.push_int(match c.and_then(|c| s.chars().position(|x| x == c)) {
                Some(i) => i as i32,
                None => -1,
            });
        }
        op::STRING_INDEXOF_STRING => {
            // 1:1 with Engine-TS: `text = pop(); find = pop(); text.indexOf(find)` —
            // the first-popped (top) string is searched for the second-popped one.
            // OS previously searched the other way round. `indexOf` is a character
            // index, but `str::find` is a byte offset, so convert.
            let text = state.pop_string();
            let find = state.pop_string();
            state.push_int(
                text.find(&find)
                    .map_or(-1, |byte| text[..byte].chars().count() as i32),
            );
        }

        // ── Server ops ────────────────────────────────────────────
        op::MAP_CLOCK => {
            state.push_int(world.tick as i32);
        }
        op::COORDX => {
            let coord = state.pop_int();
            state.push_int((coord >> 14) & 0x3fff);
        }
        op::COORDY => {
            let coord = state.pop_int();
            state.push_int((coord >> 28) & 0x3);
        }
        op::COORDZ => {
            let coord = state.pop_int();
            state.push_int(coord & 0x3fff);
        }
        op::DISTANCE => {
            let [c1, c2] = state.pop_ints::<2>();
            let (x1, z1) = ((c1 >> 14) & 0x3fff, c1 & 0x3fff);
            let (x2, z2) = ((c2 >> 14) & 0x3fff, c2 & 0x3fff);
            state.push_int((x1 - x2).abs().max((z1 - z2).abs()));
        }
        op::MOVECOORD => {
            let [coord, dx, dy, dz] = state.pop_ints::<4>();
            // Engine-TS MOVECOORD → packCoord(level+dy, x+dx, z+dz), and packCoord
            // masks each field — an offset that overflows a component wraps within
            // its own bits instead of corrupting the adjacent level/x bits.
            let x = ((coord >> 14) & 0x3fff) + dx;
            let y = ((coord >> 28) & 0x3) + dy;
            let z = (coord & 0x3fff) + dz;
            state.push_int(((y & 0x3) << 28) | ((x & 0x3fff) << 14) | (z & 0x3fff));
        }
        op::PLAYERCOUNT => {
            let count = world.players.iter().filter(|p| p.is_some()).count();
            state.push_int(count as i32);
        }
        op::MAP_PLAYERCOUNT => {
            // Engine-TS MAP_PLAYERCOUNT: count players within the [from..to]
            // tile rectangle on `from`'s level. (Engine-TS walks the covered
            // zones for speed; iterating players is equivalent for correctness.)
            let [c1, c2] = state.pop_ints::<2>();
            let flevel = (c1 >> 28) & 0x3;
            let (fx, fz) = ((c1 >> 14) & 0x3fff, c1 & 0x3fff);
            let (tx, tz) = ((c2 >> 14) & 0x3fff, c2 & 0x3fff);
            let count = world.players.iter().flatten().filter(|p| {
                let e = &p.entity;
                e.level == flevel && e.x >= fx && e.x <= tx && e.z >= fz && e.z <= tz
            }).count();
            state.push_int(count as i32);
        }
        op::MAP_MEMBERS => {
            state.push_int(1);
        }
        op::MAP_LIVE => {
            // Engine-TS MAP_LIVE: `NODE_PRODUCTION ? 1 : 0`. OS has no
            // production/dev environment split (like MAP_MEMBERS, which is
            // hardcoded above) and runs as a dev server, so it's never live.
            state.push_int(0);
        }
        op::MAP_BLOCKED => {
            // Engine-TS isMapBlocked: can't stand on the tile (floor / loc /
            // ground-decor collision). Members-gate is omitted (members=1).
            let coord = state.pop_int();
            let (x, z, level) = unpack_coord(coord);
            state.push_int(i32::from(world.collision.is_blocked(x, z, level)));
        }
        op::MAP_INDOORS => {
            // Engine-TS isIndoors: the tile carries the ROOF flag.
            let coord = state.pop_int();
            let (x, z, level) = unpack_coord(coord);
            state.push_int(i32::from(world.collision.is_indoors(x, z, level)));
        }
        op::MAP_MULTIWAY => {
            // Engine-TS isMulti: the coord's 8×8 zone is flagged multiway. Our
            // 2007 cache has no multiway data source yet, so this is always 0
            // until `world.collision.multiway` is populated.
            let coord = state.pop_int();
            let (x, z, level) = unpack_coord(coord);
            state.push_int(i32::from(world.collision.is_multiway(x, z, level)));
        }
        op::LINEOFSIGHT => {
            // Engine-TS isLineOfSight: false across different levels; else a
            // projectile-flag ray-walk between the two tiles.
            let [c1, c2] = state.pop_ints::<2>();
            let (x1, z1, l1) = unpack_coord(c1);
            let (x2, z2, l2) = unpack_coord(c2);
            let ok = l1 == l2 && world.collision.line_of_sight(l1, x1, z1, x2, z2);
            state.push_int(i32::from(ok));
        }
        op::LINEOFWALK => {
            // Engine-TS isLineOfWalk: false across levels; else a walk-flag
            // ray-walk between the two tiles.
            let [c1, c2] = state.pop_ints::<2>();
            let (x1, z1, l1) = unpack_coord(c1);
            let (x2, z2, l2) = unpack_coord(c2);
            let ok = l1 == l2 && world.collision.line_of_walk(l1, x1, z1, x2, z2);
            state.push_int(i32::from(ok));
        }
        op::SEQLENGTH => {
            // Engine-TS SEQLENGTH: the seq's duration (sum of frame delays).
            let seq = state.pop_int();
            state.push_int(world.seq_lengths.get(&seq).copied().unwrap_or(0));
        }
        op::MAP_FINDSQUARE => {
            // Engine-TS MAP_FINDSQUARE: a walkable tile in [minRadius, maxRadius]
            // of the origin. kind 0 = any open tile, 1 = reachable by
            // line-of-walk, 2 = by line-of-sight. maxRadius < 10 → 50 random
            // tries; maxRadius >= 10 → a west-bias column scan (one random z per
            // column, with an extra chebyshev >= minRadius gate). Falls back to
            // the origin coord when nothing is found.
            let [coord, min_radius, max_radius, kind] = state.pop_ints::<4>();
            let (ox, oz, level) = unpack_coord(coord);
            let pack = |x: i32, z: i32| ((level & 0x3) << 28) | ((x & 0x3fff) << 14) | (z & 0x3fff);
            let reachable = |w: &World, rx: i32, rz: i32| match kind {
                1 => w.collision.line_of_walk(level, rx, rz, ox, oz),
                2 => w.collision.line_of_sight(level, rx, rz, ox, oz),
                _ => true,
            };
            let mut result = coord;
            if max_radius < 10 {
                for _ in 0..50 {
                    let dist_x = (rand_unit() * (2 * max_radius + 1) as f64) as i32 - max_radius;
                    let dist_z = (rand_unit() * (2 * max_radius + 1) as f64) as i32 - max_radius;
                    let dist = dist_x.abs().max(dist_z.abs());
                    if dist < min_radius || dist > max_radius {
                        continue;
                    }
                    let (rx, rz) = (ox + dist_x, oz + dist_z);
                    if !world.collision.is_blocked(rx, rz, level) && reachable(world, rx, rz) {
                        result = pack(rx, rz);
                        break;
                    }
                }
            } else {
                // West-bias scan (Engine-TS "imps").
                'scan: for rx in (ox - max_radius)..=(ox + max_radius) {
                    let dist_x = rx - ox;
                    let dist_z = (rand_unit() * (2 * max_radius + 1) as f64) as i32 - max_radius;
                    let dist = dist_x.abs().max(dist_z.abs());
                    if dist < min_radius || dist > max_radius {
                        continue;
                    }
                    let rz = oz + dist_z;
                    // !isWithinDistanceSW(..., minRadius): at least minRadius away.
                    let within_min = (rx - ox).abs() <= min_radius && (rz - oz).abs() <= min_radius;
                    if within_min {
                        continue;
                    }
                    if !world.collision.is_blocked(rx, rz, level) && reachable(world, rx, rz) {
                        result = pack(rx, rz);
                        break 'scan;
                    }
                }
            }
            state.push_int(result);
        }
        op::MAP_LOCADDUNSAFE => {
            // Engine-TS MAP_LOCADDUNSAFE: is any active loc's footprint already
            // on this tile? We test the dynamic locs in the coord's zone against
            // each loc's config footprint (rotation-swapped), matching the
            // wall/ground/decor layer rules.
            let coord = state.pop_int();
            let (x, z, level) = unpack_coord(coord);
            let occupied = loc_occupies_tile(world, x, z, level);
            state.push_int(i32::from(occupied));
        }

        // ── Config-query ops (LC_* / OC_* / NC_*) ─────────────────
        // Pure readers over the loc/obj/npc config loaded into World. Fields not
        // present in our 2007 config decode (param / category / desc / weight /
        // wearpos / tradeable) stay unimplemented — there's no data to source.
        op::LC_NAME => {
            let id = state.pop_int();
            let n = config_name(world.loc_info.get(&id).map(|i| &i.name));
            state.push_string(n);
        }
        op::LC_WIDTH => {
            let id = state.pop_int();
            state.push_int(world.loc_info.get(&id).map_or(1, |i| i.width));
        }
        op::LC_LENGTH => {
            let id = state.pop_int();
            state.push_int(world.loc_info.get(&id).map_or(1, |i| i.length));
        }
        op::OC_NAME => {
            let id = state.pop_int();
            let n = config_name(world.obj_info.get(&id).map(|i| &i.name));
            state.push_string(n);
        }
        op::OC_COST => {
            let id = state.pop_int();
            state.push_int(world.obj_info.get(&id).map_or(0, |i| i.cost));
        }
        op::OC_MEMBERS => {
            let id = state.pop_int();
            state.push_int(i32::from(world.obj_info.get(&id).is_some_and(|i| i.members)));
        }
        op::OC_STACKABLE => {
            let id = state.pop_int();
            state.push_int(i32::from(world.obj_info.get(&id).is_some_and(|i| i.stackable != 0)));
        }
        op::OC_CERT => {
            // Engine-TS OC_CERT: the noted form's id (certlink) when this is an
            // un-noted item with a note link, else the id itself.
            let id = state.pop_int();
            let r = match world.obj_info.get(&id) {
                Some(o) if o.certtemplate == -1 && o.certlink >= 0 => o.certlink,
                _ => id,
            };
            state.push_int(r);
        }
        op::OC_UNCERT => {
            // Engine-TS OC_UNCERT: the un-noted form's id when this is a note.
            let id = state.pop_int();
            let r = match world.obj_info.get(&id) {
                Some(o) if o.certtemplate >= 0 && o.certlink >= 0 => o.certlink,
                _ => id,
            };
            state.push_int(r);
        }
        op::NC_NAME => {
            let id = state.pop_int();
            let n = config_name(world.npc_info.get(&id).map(|i| &i.name));
            state.push_string(n);
        }
        op::NC_SIZE => {
            let id = state.pop_int();
            state.push_int(world.npc_info.get(&id).map_or(1, |i| i.size));
        }
        op::NC_VISLEVEL => {
            let id = state.pop_int();
            state.push_int(world.npc_info.get(&id).map_or(-1, |i| i.vislevel));
        }
        op::NC_OP => {
            // Engine-TS NC_OP: the 1-based op label, or "" when absent.
            let [id, idx] = state.pop_ints::<2>();
            let s = world
                .npc_info
                .get(&id)
                .and_then(|i| usize::try_from(idx - 1).ok().and_then(|k| i.op.get(k)))
                .and_then(|o| o.clone())
                .unwrap_or_default();
            state.push_string(s);
        }
        op::LC_OP => {
            // The loc's 1-based op label (analogous to NC_OP). "" when absent.
            let [id, idx] = state.pop_ints::<2>();
            let s = world
                .loc_info
                .get(&id)
                .and_then(|i| usize::try_from(idx - 1).ok().and_then(|k| i.op.get(k)))
                .and_then(|o| o.clone())
                .unwrap_or_default();
            state.push_string(s);
        }
        op::OC_OP => {
            let [id, idx] = state.pop_ints::<2>();
            let s = world
                .obj_info
                .get(&id)
                .and_then(|i| usize::try_from(idx - 1).ok().and_then(|k| i.op.get(k)))
                .and_then(|o| o.clone())
                .unwrap_or_default();
            state.push_string(s);
        }
        op::OC_IOP => {
            // Obj inventory-op label.
            let [id, idx] = state.pop_ints::<2>();
            let s = world
                .obj_info
                .get(&id)
                .and_then(|i| usize::try_from(idx - 1).ok().and_then(|k| i.iop.get(k)))
                .and_then(|o| o.clone())
                .unwrap_or_default();
            state.push_string(s);
        }
        op::NPC_NAME => {
            // Engine-TS NPC_NAME: the active npc's type name.
            let type_id = active_npc(state, world)?.type_id;
            let n = config_name(world.npc_info.get(&type_id).map(|i| &i.name));
            state.push_string(n);
        }
        op::LOC_NAME => {
            // Engine-TS LOC_NAME: the active loc's type name.
            let (.., id, _, _) = state.active_loc.ok_or("no active_loc")?;
            let n = config_name(world.loc_info.get(&id).map(|i| &i.name));
            state.push_string(n);
        }
        op::OBJ_NAME => {
            // Engine-TS OBJ_NAME: the active obj's type name.
            let (.., id) = state.active_obj.ok_or("no active_obj")?;
            let n = config_name(world.obj_info.get(&id).map(|i| &i.name));
            state.push_string(n);
        }
        op::ENUM => {
            // Engine-TS ENUM: look up `key` in enum `enum_id`, pushing the
            // matching value or the enum default. The popped input/output type
            // args drive Engine-TS's type validation; OS instead decides
            // int-vs-string from the enum's stored data (the two agree for any
            // well-formed enum) so a type-arg convention mismatch can't abort a
            // script.
            let [_input_type, _output_type, enum_id, key] = state.pop_ints::<4>();
            let e = world.enums.get(&enum_id).ok_or_else(|| format!("enum {enum_id} not found"))?;
            let idx = e.keys.iter().position(|&k| k == key);
            if e.is_string {
                let v = idx
                    .and_then(|i| e.string_values.get(i))
                    .cloned()
                    .unwrap_or_else(|| e.default_string.clone());
                state.push_string(v);
            } else {
                let v = idx.and_then(|i| e.int_values.get(i)).copied().unwrap_or(e.default_int);
                state.push_int(v);
            }
        }
        op::ENUM_GETOUTPUTCOUNT => {
            // Engine-TS ENUM_GETOUTPUTCOUNT: the number of key→value entries.
            let id = state.pop_int();
            state.push_int(world.enums.get(&id).map_or(0, |e| e.keys.len() as i32));
        }

        // ── Inventory ops (INV_*) ─────────────────────────────────
        // Stack behaviour from ObjType.stackable; the protected-access / dummyitem
        // / scope checks Engine-TS does need InvType fields our 2007 config lacks,
        // so they're omitted. Stock / category / param / transmit ops are
        // data- or protocol-blocked and stay unimplemented.
        op::INV_ADD => {
            // Engine-TS INV_ADD: insert what fits; drop the overflow at the
            // player's feet (one obj each for non-stackables / a 1-overflow,
            // else a single stack), owned by the player for 200 ticks.
            state.pointer_check(Pointer::ActivePlayer)?;
            let [inv, obj_id, count] = state.pop_ints::<3>();
            let pid = state.active_player.ok_or("no active_player")?;
            let added = world.inv_add(pid, inv, obj_id, count);
            let overflow = count - added;
            if overflow > 0 {
                drop_overflow(world, pid, obj_id, overflow);
            }
        }
        op::INV_DEL => {
            state.pointer_check(Pointer::ActivePlayer)?;
            let [inv, obj_id, count] = state.pop_ints::<3>();
            let pid = state.active_player.ok_or("no active_player")?;
            world.inv_del(pid, inv, obj_id, count);
        }
        op::INV_TOTAL => {
            state.pointer_check(Pointer::ActivePlayer)?;
            let [inv, obj_id] = state.pop_ints::<2>();
            let pid = state.active_player.ok_or("no active_player")?;
            // Engine-TS treats obj -1 as a count of 0 rather than erroring.
            let total = if obj_id == -1 { 0 } else { world.inv_total(pid, inv, obj_id) };
            state.push_int(total);
        }
        op::INV_GETOBJ => {
            state.pointer_check(Pointer::ActivePlayer)?;
            let [inv, slot] = state.pop_ints::<2>();
            let pid = state.active_player.ok_or("no active_player")?;
            state.push_int(world.inv_get_slot(pid, inv, slot).map_or(-1, |(id, _)| id));
        }
        op::INV_GETNUM => {
            state.pointer_check(Pointer::ActivePlayer)?;
            let [inv, slot] = state.pop_ints::<2>();
            let pid = state.active_player.ok_or("no active_player")?;
            state.push_int(world.inv_get_slot(pid, inv, slot).map_or(0, |(_, c)| c));
        }
        op::INV_FREESPACE => {
            state.pointer_check(Pointer::ActivePlayer)?;
            let inv = state.pop_int();
            let pid = state.active_player.ok_or("no active_player")?;
            state.push_int(world.inv_free_space(pid, inv));
        }
        op::INV_SIZE => {
            // Engine-TS INV_SIZE: the InvType's configured size (no player).
            let inv = state.pop_int();
            state.push_int(world.inv_sizes.get(&inv).copied().unwrap_or(0));
        }
        op::INV_CLEAR => {
            state.pointer_check(Pointer::ActivePlayer)?;
            let inv = state.pop_int();
            let pid = state.active_player.ok_or("no active_player")?;
            world.inv_clear(pid, inv);
        }
        op::INV_SETSLOT => {
            state.pointer_check(Pointer::ActivePlayer)?;
            let [inv, slot, obj_id, count] = state.pop_ints::<4>();
            let pid = state.active_player.ok_or("no active_player")?;
            world.inv_set(pid, inv, obj_id, count, slot);
        }
        op::INV_DELSLOT => {
            state.pointer_check(Pointer::ActivePlayer)?;
            let [inv, slot] = state.pop_ints::<2>();
            let pid = state.active_player.ok_or("no active_player")?;
            world.inv_set(pid, inv, -1, 0, slot);
        }
        op::INV_ITEMSPACE => {
            // Engine-TS INV_ITEMSPACE: 1 if `count` of `obj` fits within the
            // first `size` slots, else 0.
            state.pointer_check(Pointer::ActivePlayer)?;
            let [inv, obj_id, count, size] = state.pop_ints::<4>();
            if count == 0 {
                state.push_int(0);
            } else {
                let inv_size = world.inv_sizes.get(&inv).copied().unwrap_or(0);
                if size < 0 || size > inv_size {
                    return Err(format!("inv_itemspace size out of range: {size}"));
                }
                let pid = state.active_player.ok_or("no active_player")?;
                let overflow = world.inv_item_space(pid, inv, obj_id, count, size);
                state.push_int(i32::from(overflow == 0));
            }
        }
        op::INV_MOVETOSLOT => {
            // Engine-TS INV_MOVETOSLOT: swap two slots (UI drag).
            state.pointer_check(Pointer::ActivePlayer)?;
            let [from_inv, to_inv, from_slot, to_slot] = state.pop_ints::<4>();
            let pid = state.active_player.ok_or("no active_player")?;
            world.inv_move_to_slot(pid, from_inv, to_inv, from_slot, to_slot);
        }
        op::INV_MOVEFROMSLOT => {
            // Engine-TS INV_MOVEFROMSLOT: move a whole slot into another inv,
            // dropping any overflow at the player's feet.
            state.pointer_check(Pointer::ActivePlayer)?;
            let [from_inv, to_inv, from_slot] = state.pop_ints::<3>();
            let pid = state.active_player.ok_or("no active_player")?;
            let (overflow, obj) = world.inv_move_from_slot(pid, from_inv, to_inv, from_slot);
            if overflow > 0 {
                drop_overflow(world, pid, obj, overflow);
            }
        }
        op::INV_CHANGESLOT => {
            // Engine-TS INV_CHANGESLOT: replace the first slot holding `find`
            // with `replace` × `replace_count`.
            state.pointer_check(Pointer::ActivePlayer)?;
            let [inv, find, replace, replace_count] = state.pop_ints::<4>();
            let pid = state.active_player.ok_or("no active_player")?;
            let _ = world.inv_free_space(pid, inv); // ensure the inv exists
            let cap = world.inv_sizes.get(&inv).copied().unwrap_or(0);
            let mut target = None;
            for s in 0..cap {
                if let Some((id, _)) = world.inv_get_slot(pid, inv, s) {
                    if id == find {
                        target = Some(s);
                        break;
                    }
                }
            }
            if let Some(slot) = target {
                world.inv_set(pid, inv, replace, replace_count, slot);
            }
        }
        op::INV_MOVEITEM | op::INV_MOVEITEM_CERT | op::INV_MOVEITEM_UNCERT => {
            // Engine-TS INV_MOVEITEM(_CERT/_UNCERT): delete `count` of `obj` from
            // one inv and add it to another (overflow drops). The CERT/UNCERT
            // variants re-map the obj to its noted / un-noted form before adding.
            state.pointer_check(Pointer::ActivePlayer)?;
            let [from_inv, to_inv, obj, count] = state.pop_ints::<4>();
            let pid = state.active_player.ok_or("no active_player")?;
            let completed = world.inv_del(pid, from_inv, obj, count);
            if completed > 0 {
                let final_obj = match opcode {
                    op::INV_MOVEITEM_CERT => match world.obj_info.get(&obj) {
                        Some(o) if o.certtemplate == -1 && o.certlink >= 0 => o.certlink,
                        _ => obj,
                    },
                    op::INV_MOVEITEM_UNCERT => match world.obj_info.get(&obj) {
                        Some(o) if o.certtemplate >= 0 && o.certlink >= 0 => o.certlink,
                        _ => obj,
                    },
                    _ => obj,
                };
                let added = world.inv_add(pid, to_inv, final_obj, completed);
                let overflow = completed - added;
                if overflow > 0 {
                    drop_overflow(world, pid, final_obj, overflow);
                }
            }
        }
        op::INV_DROPSLOT => {
            // Engine-TS INV_DROPSLOT: drop the slot's contents at `coord`.
            state.pointer_check(Pointer::ActivePlayer)?;
            let [inv, coord, slot, duration] = state.pop_ints::<4>();
            let pid = state.active_player.ok_or("no active_player")?;
            if let Some((id, count)) = world.inv_get_slot(pid, inv, slot) {
                world.inv_set(pid, inv, -1, 0, slot);
                let (x, z, level) = unpack_coord(coord);
                world.add_ground_obj(id, count, x, z, level, pid as i32, duration);
            } else {
                return Err("inv_dropslot: slot is empty".to_string());
            }
        }
        op::INV_DROPITEM => {
            // Engine-TS INV_DROPITEM: delete `count` of `obj` and drop what was
            // removed at `coord`, making it the active obj.
            state.pointer_check(Pointer::ActivePlayer)?;
            let [inv, coord, obj, count, duration] = state.pop_ints::<5>();
            let pid = state.active_player.ok_or("no active_player")?;
            let completed = world.inv_del(pid, inv, obj, count);
            if completed > 0 {
                let (x, z, level) = unpack_coord(coord);
                world.add_ground_obj(obj, completed, x, z, level, pid as i32, duration);
                state.active_obj = Some((x, z, level, obj));
                state.pointer_add(Pointer::ActiveObj);
            }
        }
        op::INV_DROPITEM_DELAYED => {
            // Engine-TS INV_DROPITEM_DELAYED: like INV_DROPITEM but the ground
            // obj spawns after `delay` ticks (then despawns after `duration`).
            state.pointer_check(Pointer::ActivePlayer)?;
            let [inv, coord, obj, count, duration, delay] = state.pop_ints::<6>();
            let pid = state.active_player.ok_or("no active_player")?;
            let completed = world.inv_del(pid, inv, obj, count);
            if completed > 0 {
                let (x, z, level) = unpack_coord(coord);
                world.add_ground_obj_delayed(obj, completed, x, z, level, pid as i32, duration, delay);
            }
        }
        op::INV_DROPALL => {
            // Engine-TS INV_DROPALL: drop every slot's contents at `coord` and
            // empty the inventory.
            state.pointer_check(Pointer::ActivePlayer)?;
            let [inv, coord, duration] = state.pop_ints::<3>();
            let pid = state.active_player.ok_or("no active_player")?;
            let (x, z, level) = unpack_coord(coord);
            let _ = world.inv_free_space(pid, inv); // ensure the inv exists
            let cap = world.inv_sizes.get(&inv).copied().unwrap_or(0);
            let items: Vec<(i32, i32)> =
                (0..cap).filter_map(|s| world.inv_get_slot(pid, inv, s)).collect();
            world.inv_clear(pid, inv);
            for (id, count) in items {
                world.add_ground_obj(id, count, x, z, level, pid as i32, duration);
            }
        }
        op::INV_TRANSMIT => {
            // Engine-TS INV_TRANSMIT: show this player's inv `inv` on component
            // `com` (UPDATE_INV_FULL is sent in the player flush each tick).
            state.pointer_check(Pointer::ActivePlayer)?;
            let [inv, com] = state.pop_ints::<2>();
            let pid = state.active_player.ok_or("no active_player")?;
            let uid = world.players[pid].as_ref().map_or(0, |p| p.uid());
            world.inv_listen_on_com(pid, inv, com, uid);
        }
        op::INVOTHER_TRANSMIT => {
            // Engine-TS INVOTHER_TRANSMIT: show another player's (`uid`) inv on
            // this player's component `com`.
            state.pointer_check(Pointer::ActivePlayer)?;
            let [uid, inv, com] = state.pop_ints::<3>();
            let pid = state.active_player.ok_or("no active_player")?;
            world.inv_listen_on_com(pid, inv, com, uid);
        }
        op::INV_STOPTRANSMIT => {
            // Engine-TS INV_STOPTRANSMIT: stop showing an inv on component `com`.
            state.pointer_check(Pointer::ActivePlayer)?;
            let com = state.pop_int();
            let pid = state.active_player.ok_or("no active_player")?;
            world.inv_stop_listen_on_com(pid, com);
        }
        op::OBJ_TAKEITEM => {
            // Engine-TS OBJ_TAKEITEM: pick up the active ground obj into `inv`.
            let inv = state.pop_int();
            let (x, z, level, id) = state.active_obj.ok_or("no active_obj")?;
            let pid = state.active_player.ok_or("no active_player")?;
            if let Some(count) = world.find_obj(x, z, level, id, pid) {
                world.inv_add(pid, inv, id, count);
                world.remove_obj_broadcast(x, z, level, id);
            }
        }
        op::BOTH_MOVEINV => {
            // Engine-TS BOTH_MOVEINV: move every item from one player's `from`
            // inv into another player's `to` inv (overflow drops at the receiver's
            // feet). `.both_moveinv` (secondary) swaps which player is from/to:
            // normal → primary→secondary, secondary → secondary→primary.
            state.pointer_check(Pointer::ActivePlayer)?;
            let [from, to] = state.pop_ints::<2>();
            let secondary = state.secondary();
            let (from_pid, to_pid) = if secondary {
                (state.active_player2, state.active_player)
            } else {
                (state.active_player, state.active_player2)
            };
            let from_pid = from_pid.ok_or("BOTH_MOVEINV: missing from player")?;
            let to_pid = to_pid.ok_or("BOTH_MOVEINV: missing to player")?;
            let _ = world.inv_free_space(from_pid, from); // ensure it exists
            let cap = world.inv_sizes.get(&from).copied().unwrap_or(0);
            let items: Vec<(i32, i32)> =
                (0..cap).filter_map(|s| world.inv_get_slot(from_pid, from, s)).collect();
            world.inv_clear(from_pid, from);
            for (id, count) in items {
                let added = world.inv_add(to_pid, to, id, count);
                let overflow = count - added;
                if overflow > 0 {
                    drop_overflow(world, to_pid, id, overflow);
                }
            }
        }
        op::BOTH_DROPSLOT => {
            // Engine-TS BOTH_DROPSLOT: drop one player's slot at `coord`, owned
            // by the *other* player (the PvP-drop recipient). Secondary swaps
            // from/to like BOTH_MOVEINV.
            state.pointer_check(Pointer::ActivePlayer)?;
            let [inv, coord, slot, duration] = state.pop_ints::<4>();
            let secondary = state.secondary();
            let (from_pid, to_pid) = if secondary {
                (state.active_player2, state.active_player)
            } else {
                (state.active_player, state.active_player2)
            };
            let from_pid = from_pid.ok_or("BOTH_DROPSLOT: missing from player")?;
            let to_pid = to_pid.ok_or("BOTH_DROPSLOT: missing to player")?;
            if let Some((id, count)) = world.inv_get_slot(from_pid, inv, slot) {
                world.inv_set(from_pid, inv, -1, 0, slot);
                let (x, z, level) = unpack_coord(coord);
                world.add_ground_obj(id, count, x, z, level, to_pid as i32, duration);
            } else {
                return Err("both_dropslot: slot is empty".to_string());
            }
        }

        // ── Player ops ────────────────────────────────────────────
        op::MES => {
            state.pointer_check(Pointer::ActivePlayer)?;
            let text = state.pop_string();
            let p = active_player(state, world)?;
            p.write(msg::message_game(&text));
        }
        op::ANIM => {
            let [seq, delay] = state.pop_ints::<2>();
            let p = active_player(state, world)?;
            // A protected animation (P_ANIMPROTECT) can't be changed — 1:1 with
            // Engine-TS playAnimation's `if (... || this.animProtect) return`.
            if p.anim_protect == 0 {
                p.entity.anim_id = seq;
                p.entity.anim_delay = delay;
                p.entity.masks |= player::MASK_ANIM;
            }
        }
        op::P_ANIMPROTECT => {
            // Engine-TS P_ANIMPROTECT: protect (1) or release (0) the player's
            // animation from being changed by the ANIM op.
            state.pointer_check(Pointer::ProtectedActivePlayer)?;
            let v = state.pop_int();
            active_player(state, world)?.anim_protect = v;
        }
        op::SAY => {
            let text = state.pop_string();
            let p = active_player(state, world)?;
            p.entity.chat = Some(text);
            p.entity.masks |= player::MASK_SAY;
        }
        op::SPOTANIM_PL => {
            let [spot, height, delay] = state.pop_ints::<3>();
            let p = active_player(state, world)?;
            p.entity.spotanim_id = spot;
            p.entity.spotanim_height = height;
            p.entity.spotanim_delay = delay;
            p.entity.masks |= player::MASK_SPOTANIM;
        }
        op::FACESQUARE => {
            let coord = state.pop_int();
            let tx = (coord >> 14) & 0x3fff;
            let tz = coord & 0x3fff;
            let p = active_player(state, world)?;
            p.entity.face_x = tx;
            p.entity.face_z = tz;
            // Engine-TS focus(client=true) also updates the persistent
            // orientation, so a newly-observing client sees the entity already
            // turned toward the coord (not just the one-shot FACE_COORD mask).
            p.entity.face_angle_x = tx;
            p.entity.face_angle_z = tz;
            p.entity.masks |= player::MASK_FACE_COORD;
        }
        op::CAM_LOOKAT => {
            let [coord, height, rate, rate2] = state.pop_ints::<4>();
            let p = active_player(state, world)?;
            p.cam_lookat((coord >> 14) & 0x3fff, coord & 0x3fff, height, rate, rate2);
        }
        op::CAM_MOVETO => {
            let [coord, height, rate, rate2] = state.pop_ints::<4>();
            let p = active_player(state, world)?;
            p.cam_moveto((coord >> 14) & 0x3fff, coord & 0x3fff, height, rate, rate2);
        }
        op::CAM_SHAKE => {
            let [slot, axis, random, amplitude] = state.pop_ints::<4>();
            let p = active_player(state, world)?;
            p.cam_shake(slot, axis, random, amplitude);
        }
        op::CAM_RESET => {
            let p = active_player(state, world)?;
            p.cam_reset();
        }
        op::HINT_NPC => {
            let nid = state.active_npc.ok_or("no active_npc")?;
            let p = active_player(state, world)?;
            p.hint_npc(nid as i32);
        }
        op::HINT_PL => {
            let slot = state.active_player2.ok_or("no active_player2")?;
            let p = active_player(state, world)?;
            p.hint_player(slot as i32);
        }
        op::HINT_COORD => {
            let [offset, coord, height] = state.pop_ints::<3>();
            let p = active_player(state, world)?;
            p.hint_tile(offset, (coord >> 14) & 0x3fff, coord & 0x3fff, height);
        }
        op::HINT_STOP => {
            let p = active_player(state, world)?;
            p.hint_stop();
        }
        op::IF_OPENTOP => {
            // Set the fullscreen/root toplevel (welcome screen, game frame, …).
            let interface = state.pop_int();
            let p = active_player(state, world)?;
            p.write(msg::if_opentop(interface));
        }
        op::IF_OPENSUB => {
            // Open interface `sub` as a child at `component`, kind 0 = modal,
            // 1 = overlay. `component` is a single packed (parent<<16)|child
            // int — the compiler resolves `if_549:com_2` to that packed form
            // (matching component trigger subjects), so we pass it through.
            let [component, sub, kind] = state.pop_ints::<3>();
            let p = active_player(state, world)?;
            p.write(msg::if_opensub(component, sub, kind));
        }
        op::IF_CLOSE => {
            // Engine-TS IF_CLOSE → closeModal: end any open dialog/modal
            // server-side now (clear the weak queue, abandon a pause-button /
            // count-dialog wait). The script-initiated parallel of the client's
            // CLOSE_MODAL packet, sharing the same `close_modal` primitive. (The
            // interface-close transmission awaits the modal-interface tracking —
            // the same gap `close_modal` already documents.)
            state.pointer_check(Pointer::ActivePlayer)?;
            let pid = state.active_player.ok_or("no active_player")?;
            world.close_modal(pid);
        }
        op::IF_SETTEXT => {
            // Engine-TS IF_SETTEXT: set a component's text (message of the week,
            // stat displays, …). Pops text then the component id.
            let text = state.pop_string();
            let com = state.pop_int();
            let p = active_player(state, world)?;
            p.write(msg::if_settext(com, &text));
        }
        op::IF_SETHIDE => {
            // Engine-TS IF_SETHIDE: show/hide a component. Pops (com, hide).
            let [com, hide] = state.pop_ints::<2>();
            let p = active_player(state, world)?;
            p.write(msg::if_sethide(com, hide == 1));
        }
        op::IF_SETANIM => {
            // Engine-TS IF_SETANIM: play a seq on a component's model.
            let [com, seq] = state.pop_ints::<2>();
            let p = active_player(state, world)?;
            p.write(msg::if_setanim(com, seq));
        }
        op::IF_SETCOLOUR => {
            // Engine-TS IF_SETCOLOUR: the script passes 24-bit RGB; the client
            // wants 15-bit (5/5/5), so convert before sending.
            let [com, colour] = state.pop_ints::<2>();
            let p = active_player(state, world)?;
            p.write(msg::if_setcolour(com, rgb24_to_15(colour)));
        }
        op::IF_SETMODEL => {
            // Engine-TS IF_SETMODEL: set a component's model.
            let [com, model] = state.pop_ints::<2>();
            let p = active_player(state, world)?;
            p.write(msg::if_setmodel(com, model));
        }
        op::IF_SETOBJECT => {
            // Engine-TS IF_SETOBJECT: show item `obj` at `scale` on a component.
            let [com, obj, scale] = state.pop_ints::<3>();
            let p = active_player(state, world)?;
            p.write(msg::if_setobject(com, obj, scale));
        }
        op::IF_SETPOSITION => {
            // Engine-TS IF_SETPOSITION: move a component. Pops (com, x, y).
            let [com, x, y] = state.pop_ints::<3>();
            let p = active_player(state, world)?;
            p.write(msg::if_setposition(com, x, y));
        }
        op::IF_SETNPCHEAD => {
            // Engine-TS IF_SETNPCHEAD: npc chathead on a component. Pops (com, npc).
            let [com, npc] = state.pop_ints::<2>();
            let p = active_player(state, world)?;
            p.write(msg::if_setnpchead(com, npc));
        }
        op::IF_SETPLAYERHEAD => {
            // Engine-TS IF_SETPLAYERHEAD: local player's chathead. Pops (com).
            let com = state.pop_int();
            let p = active_player(state, world)?;
            p.write(msg::if_setplayerhead(com));
        }
        op::IF_SETSCROLLPOS => {
            // Engine-TS IF_SETSCROLLPOS: scroll a component. Pops (com, y).
            let [com, y] = state.pop_ints::<2>();
            let p = active_player(state, world)?;
            p.write(msg::if_setscrollpos(com, y));
        }
        op::IF_SETANGLE => {
            // Engine-TS IF_SETANGLE: component model angle + zoom. Pops
            // (com, xan, yan, zoom).
            let [com, xan, yan, zoom] = state.pop_ints::<4>();
            let p = active_player(state, world)?;
            p.write(msg::if_setangle(com, xan, yan, zoom));
        }
        op::IF_SETROTATION => {
            // Engine-TS IF_SETROTATION: set a component model's auto-spin speed.
            // Pops (com, x_angle_speed, y_angle_speed).
            let [com, x_angle, y_angle] = state.pop_ints::<3>();
            active_player(state, world)?.write(msg::if_setrotation(com, x_angle, y_angle));
        }
        op::IF_SETRESUMEBUTTONS => {
            // Engine-TS IF_SETRESUMEBUTTONS: register the five components that
            // resume a pause-button dialog (server-side state, no packet).
            let buttons = state.pop_ints::<5>();
            active_player(state, world)?.resume_buttons = buttons;
        }
        op::SET_PLAYER_OP => {
            // Engine-TS SET_PLAYER_OP: set one of the player's 1-8 right-click
            // menu options. Pops string(label) then (index, primary). A "null"
            // label clears the slot client-side.
            let text = state.pop_string();
            let [index, primary] = state.pop_ints::<2>();
            if !(1..=8).contains(&index) {
                return Err(format!("SET_PLAYER_OP index out of range: {index}"));
            }
            active_player(state, world)?.write(msg::set_player_op(index, &text, primary));
        }
        // Base ("bas") stance animations sent in the appearance block — set by
        // content to match the worn weapon / active spellbook. Engine-TS
        // READYANIM/TURNANIM/WALKANIM/WALKANIM_B/L/R; RUNANIM additionally
        // allows -1 (no run animation → the player is forced to walk).
        op::READYANIM => { let v = state.pop_int(); active_player(state, world)?.ready_anim = v; }
        op::TURNANIM => { let v = state.pop_int(); active_player(state, world)?.turn_anim = v; }
        op::WALKANIM => { let v = state.pop_int(); active_player(state, world)?.walk_anim = v; }
        op::WALKANIM_B => { let v = state.pop_int(); active_player(state, world)?.walk_anim_b = v; }
        op::WALKANIM_L => { let v = state.pop_int(); active_player(state, world)?.walk_anim_l = v; }
        op::WALKANIM_R => { let v = state.pop_int(); active_player(state, world)?.walk_anim_r = v; }
        op::RUNANIM => { let v = state.pop_int(); active_player(state, world)?.run_anim = v; }
        op::NPC_FINDUID => {
            // Engine-TS NPC_FINDUID: resolve an npc by uid (slot + type) and set
            // the active npc (primary or .secondary). Pushes 1 if it's found and
            // still the expected type, else 0.
            let uid = state.pop_int();
            match world.get_npc_by_uid(uid) {
                Some(nid) => {
                    let secondary = state.secondary();
                    if secondary {
                        state.active_npc2 = Some(nid);
                    } else {
                        state.active_npc = Some(nid);
                    }
                    state.pointer_add(crate::script::state::ACTIVE_NPC[secondary as usize]);
                    state.push_int(1);
                }
                None => state.push_int(0),
            }
        }
        op::NPC_FINDALL => {
            // Engine-TS NPC_FINDALL: set up an iterator over npcs of `type`
            // within `distance` of a coord (nearest first), walked by
            // NPC_FINDNEXT. The huntvis (line-of-sight) check awaits collision.
            let [coord, type_id, distance, _checkvis] = state.pop_ints::<4>();
            let (cx, cz, clevel) = ((coord >> 14) & 0x3fff, coord & 0x3fff, (coord >> 28) & 0x3);
            let mut found: Vec<(usize, i32)> = world.npcs_within(cx, cz, clevel, distance)
                .into_iter()
                .filter_map(|nid| world.npcs[nid].as_ref().and_then(|n| {
                    if n.active && n.type_id == type_id && n.entity.level == clevel {
                        let d = (n.entity.x - cx).abs().max((n.entity.z - cz).abs());
                        (d <= distance).then_some((nid, d))
                    } else {
                        None
                    }
                }))
                .collect();
            found.sort_by_key(|&(_, d)| d);
            // Store farthest-first so FINDNEXT's pop() yields nearest first.
            state.npc_iterator = found.into_iter().rev().map(|(nid, _)| nid).collect();
        }
        op::NPC_FINDALLANY => {
            // Engine-TS NPC_FINDALLANY: like NPC_FINDALL but matches any type.
            let [coord, distance, _checkvis] = state.pop_ints::<3>();
            let (cx, cz, clevel) = ((coord >> 14) & 0x3fff, coord & 0x3fff, (coord >> 28) & 0x3);
            let mut found: Vec<(usize, i32)> = world.npcs_within(cx, cz, clevel, distance)
                .into_iter()
                .filter_map(|nid| world.npcs[nid].as_ref().and_then(|n| {
                    if n.active && n.entity.level == clevel {
                        let d = (n.entity.x - cx).abs().max((n.entity.z - cz).abs());
                        (d <= distance).then_some((nid, d))
                    } else {
                        None
                    }
                }))
                .collect();
            found.sort_by_key(|&(_, d)| d);
            state.npc_iterator = found.into_iter().rev().map(|(nid, _)| nid).collect();
        }
        op::NPC_FINDALLZONE => {
            // Engine-TS NPC_FINDALLZONE: iterate every npc in the coord's zone.
            let coord = state.pop_int();
            let (cx, cz, clevel) = ((coord >> 14) & 0x3fff, coord & 0x3fff, (coord >> 28) & 0x3);
            let mut nids = world.npcs_in_zone(cx, cz, clevel);
            nids.reverse(); // pop() then yields them in zone order
            state.npc_iterator = nids;
        }
        op::OBJ_FIND => {
            // Engine-TS OBJ_FIND: set the active obj to a ground item of `obj_id`
            // at the coord (visible to the active player), push 1; else 0.
            let [coord, obj_id] = state.pop_ints::<2>();
            let (cx, cz, clevel) = ((coord >> 14) & 0x3fff, coord & 0x3fff, (coord >> 28) & 0x3);
            let pid = state.active_player.unwrap_or(usize::MAX);
            if world.find_obj(cx, cz, clevel, obj_id, pid).is_some() {
                state.active_obj = Some((cx, cz, clevel, obj_id));
                state.pointer_add(Pointer::ActiveObj);
                state.push_int(1);
            } else {
                state.push_int(0);
            }
        }
        op::OBJ_COORD => {
            // Engine-TS OBJ_COORD: the active obj's packed coord.
            let (x, z, level, _) = state.active_obj.ok_or("no active_obj")?;
            state.push_int((level << 28) | (x << 14) | z);
        }
        op::OBJ_DEL => {
            // Engine-TS OBJ_DEL: remove the active ground item. OS's objs are
            // DESPAWN-lifecycle, so there's no config respawn rate to schedule.
            let (x, z, level, id) = state.active_obj.ok_or("no active_obj")?;
            world.remove_obj_broadcast(x, z, level, id);
        }
        op::OBJ_ADD => {
            // Engine-TS OBJ_ADD: drop a ground item owned by the active player
            // with a despawn duration, and make it the active obj. (The
            // stackable split for non-stackable count > 1 and the members /
            // dummyitem gates need obj config; OS drops the one-pile model.)
            let [coord, obj_id, count, duration] = state.pop_ints::<4>();
            if obj_id != -1 && count != -1 {
                let (cx, cz, clevel) = ((coord >> 14) & 0x3fff, coord & 0x3fff, (coord >> 28) & 0x3);
                let receiver = state.active_player.map_or(-1, |p| p as i32);
                world.add_ground_obj(obj_id, count, cx, cz, clevel, receiver, duration);
                state.active_obj = Some((cx, cz, clevel, obj_id));
                state.pointer_add(Pointer::ActiveObj);
            }
        }
        op::OBJ_ADDALL => {
            // Engine-TS OBJ_ADDALL: like OBJ_ADD but the item is public the moment
            // it drops (Obj.NO_RECEIVER) — every nearby player sees it immediately,
            // with no private-reveal window — and it still becomes the active obj.
            // (Stackable-split / members / dummyitem gates need obj config; OS
            // keeps the one-pile model.)
            let [coord, obj_id, count, duration] = state.pop_ints::<4>();
            if obj_id != -1 && count != -1 {
                let (cx, cz, clevel) = ((coord >> 14) & 0x3fff, coord & 0x3fff, (coord >> 28) & 0x3);
                world.add_ground_obj(obj_id, count, cx, cz, clevel, -1, duration);
                state.active_obj = Some((cx, cz, clevel, obj_id));
                state.pointer_add(Pointer::ActiveObj);
            }
        }
        op::OBJ_TYPE => {
            // Engine-TS OBJ_TYPE: the active obj's type id.
            let (_, _, _, id) = state.active_obj.ok_or("no active_obj")?;
            state.push_int(id);
        }
        op::OBJ_COUNT => {
            // Engine-TS OBJ_COUNT: the active obj's stack count (0 if it's gone
            // or not visible to the active player).
            let (x, z, level, id) = state.active_obj.ok_or("no active_obj")?;
            let pid = state.active_player.unwrap_or(usize::MAX);
            let count = world.find_obj(x, z, level, id, pid).unwrap_or(0);
            state.push_int(count);
        }
        op::LOC_FIND => {
            // Engine-TS LOC_FIND: set the active loc to a spawned map object of
            // `loc_id` at the coord, push 1; else 0.
            let [coord, loc_id] = state.pop_ints::<2>();
            let (cx, cz, clevel) = ((coord >> 14) & 0x3fff, coord & 0x3fff, (coord >> 28) & 0x3);
            match world.find_loc(cx, cz, clevel, loc_id) {
                Some((shape, angle)) => {
                    state.active_loc = Some((cx, cz, clevel, loc_id, shape, angle));
                    state.pointer_add(Pointer::ActiveLoc);
                    state.push_int(1);
                }
                None => state.push_int(0),
            }
        }
        op::LOC_COORD => {
            let (x, z, level, ..) = state.active_loc.ok_or("no active_loc")?;
            state.push_int((level << 28) | (x << 14) | z);
        }
        op::LOC_TYPE => {
            let (_, _, _, id, _, _) = state.active_loc.ok_or("no active_loc")?;
            state.push_int(id);
        }
        op::LOC_SHAPE => {
            let (_, _, _, _, shape, _) = state.active_loc.ok_or("no active_loc")?;
            state.push_int(shape);
        }
        op::LOC_ANGLE => {
            let (_, _, _, _, _, angle) = state.active_loc.ok_or("no active_loc")?;
            state.push_int(angle);
        }
        op::LOC_ADD => {
            // Engine-TS LOC_ADD: spawn a map object (reverting after `duration`,
            // or -1 = permanent) and make it the active loc. (OS keeps a flat
            // per-tile change list; the shape→layer replacement awaits the loc
            // config + base map.)
            let [coord, type_id, angle, shape, duration] = state.pop_ints::<5>();
            let (cx, cz, clevel) = ((coord >> 14) & 0x3fff, coord & 0x3fff, (coord >> 28) & 0x3);
            world.add_loc_timed(type_id, shape, angle, cx, cz, clevel, duration);
            state.active_loc = Some((cx, cz, clevel, type_id, shape, angle));
            state.pointer_add(Pointer::ActiveLoc);
        }
        op::LOC_DEL => {
            // Engine-TS LOC_DEL: remove the active loc. The `duration` (how long
            // until the base map loc respawns) awaits map loading — the change
            // itself is removed now.
            let _duration = state.pop_int();
            let (x, z, level, _id, shape, _angle) = state.active_loc.ok_or("no active_loc")?;
            world.del_loc(x, z, level, shape);
        }
        op::LOC_CHANGE => {
            // Engine-TS LOC_CHANGE: retype the active loc (keeping shape/angle),
            // reverting after `duration`. Updates the active-loc handle's id.
            let [id, duration] = state.pop_ints::<2>();
            let (x, z, level, _old, shape, angle) = state.active_loc.ok_or("no active_loc")?;
            world.change_loc(x, z, level, shape, id, duration);
            state.active_loc = Some((x, z, level, id, shape, angle));
        }
        op::LOC_ANIM => {
            // Engine-TS LOC_ANIM: play an animation on the active loc.
            let seq = state.pop_int();
            let (x, z, level, _id, shape, angle) = state.active_loc.ok_or("no active_loc")?;
            world.anim_loc(x, z, level, shape, angle, seq);
        }
        op::LOC_FINDALLZONE => {
            // Engine-TS LOC_FINDALLZONE: iterate the spawned map objects in the
            // coord's zone, walked by LOC_FINDNEXT.
            let coord = state.pop_int();
            let (cx, cz, clevel) = ((coord >> 14) & 0x3fff, coord & 0x3fff, (coord >> 28) & 0x3);
            let mut locs = world.locs_in_zone(cx, cz, clevel);
            locs.reverse(); // pop() then yields them in zone order
            state.loc_iterator = locs;
        }
        op::LOC_FINDNEXT => {
            // Engine-TS LOC_FINDNEXT: advance the iterator, setting the next loc
            // active (push 1), or push 0 when exhausted.
            match state.loc_iterator.pop() {
                Some(loc) => {
                    state.active_loc = Some(loc);
                    state.pointer_add(Pointer::ActiveLoc);
                    state.push_int(1);
                }
                None => state.push_int(0),
            }
        }
        op::OBJ_FINDALLZONE => {
            // Engine-TS OBJ_FINDALLZONE: iterate the ground items in the coord's
            // zone (visible to the active player), walked by OBJ_FINDNEXT.
            let coord = state.pop_int();
            let (cx, cz, clevel) = ((coord >> 14) & 0x3fff, coord & 0x3fff, (coord >> 28) & 0x3);
            let pid = state.active_player.unwrap_or(usize::MAX);
            let mut objs = world.objs_in_zone(cx, cz, clevel, pid);
            objs.reverse(); // pop() then yields them in zone order
            state.obj_iterator = objs;
        }
        op::OBJ_FINDNEXT => {
            // Engine-TS OBJ_FINDNEXT: advance the iterator, setting the next obj
            // active (push 1), or push 0 when exhausted.
            match state.obj_iterator.pop() {
                Some(loc) => {
                    state.active_obj = Some(loc);
                    state.pointer_add(Pointer::ActiveObj);
                    state.push_int(1);
                }
                None => state.push_int(0),
            }
        }
        op::NPC_FINDNEXT => {
            // Engine-TS NPC_FINDNEXT: advance the iterator, setting the next npc
            // active (push 1), or push 0 when exhausted.
            match state.npc_iterator.pop() {
                Some(nid) => {
                    let secondary = state.secondary();
                    if secondary {
                        state.active_npc2 = Some(nid);
                    } else {
                        state.active_npc = Some(nid);
                    }
                    state.pointer_add(crate::script::state::ACTIVE_NPC[secondary as usize]);
                    state.push_int(1);
                }
                None => state.push_int(0),
            }
        }
        op::P_FINDUID => {
            // Engine-TS P_FINDUID: resolve a player by uid and take protected
            // access, setting the active player. A no-op success if already
            // running on that player with protected access; fails (0) if the
            // target is gone or busy (can't act on a player mid-action).
            let uid = state.pop_int();
            let secondary = state.secondary();
            let ptr = crate::script::state::PROTECTED_ACTIVE_PLAYER[secondary as usize];
            let cur = if secondary { state.active_player2 } else { state.active_player };
            let already = state.pointer_get(ptr)
                && cur.and_then(|pid| world.players.get(pid).and_then(|o| o.as_ref()))
                      .map_or(false, |p| p.uid() == uid);
            if already {
                state.push_int(1);
            } else {
                let target = world.get_player_by_uid(uid)
                    .filter(|&pid| world.player_can_access(pid));
                match target {
                    Some(pid) => {
                        if secondary {
                            state.active_player2 = Some(pid);
                        } else {
                            state.active_player = Some(pid);
                        }
                        state.pointer_add(ptr);
                        state.push_int(1);
                    }
                    None => state.push_int(0),
                }
            }
        }
        op::LAST_COM => {
            // Engine-TS LAST_COM: the component the active player last clicked.
            let p = active_player(state, world)?;
            let com = p.last_com;
            state.push_int(com);
        }
        op::LAST_INT => {
            // Engine-TS LAST_INT: this invocation's active value (e.g. a
            // pause-button / count-dialog resume value).
            state.push_int(state.last_int);
        }
        op::LAST_ITEM => {
            // Engine-TS LAST_ITEM: the item last acted on. Only valid in the
            // held/inv-button triggers.
            require_trigger(state, &OPHELD_OR_INVBUTTON, "last_item")?;
            state.push_int(active_player(state, world)?.last_item);
        }
        op::LAST_SLOT => {
            require_trigger(state, &OPHELD_OR_INVBUTTON_D, "last_slot")?;
            state.push_int(active_player(state, world)?.last_slot);
        }
        op::LAST_USEITEM => {
            require_trigger(state, &USE_TRIGGERS, "last_useitem")?;
            state.push_int(active_player(state, world)?.last_use_item);
        }
        op::LAST_USESLOT => {
            require_trigger(state, &USE_TRIGGERS, "last_useslot")?;
            state.push_int(active_player(state, world)?.last_use_slot);
        }
        op::LAST_TARGETSLOT => {
            require_trigger(state, &[trigger::INV_BUTTOND], "last_targetslot")?;
            state.push_int(active_player(state, world)?.last_target_slot);
        }
        op::ALLOWDESIGN => {
            // Engine-TS ALLOWDESIGN: gate the character-design interface.
            let v = state.pop_int();
            active_player(state, world)?.allow_design = v == 1;
        }
        op::P_APRANGE => {
            // Engine-TS P_APRANGE: set the interaction approach range and mark
            // it script-set this tick. Requires protected access.
            state.pointer_check(Pointer::ProtectedActivePlayer)?;
            let range = state.pop_int();
            let p = active_player(state, world)?;
            p.ap_range = range;
            p.ap_range_called = true;
        }
        op::P_TRANSMOGRIFY => {
            // Engine-TS P_TRANSMOGRIFY: render the player as npc `id` (-1 = off).
            // Engine-TS bounds-checks against NpcType.count; we accept -1 plus
            // any id with loaded npc config.
            let id = state.pop_int();
            if id < -1 {
                return Err(format!("P_TRANSMOGRIFY invalid npc: {id}"));
            }
            let p = active_player(state, world)?;
            p.transmog = id;
            p.appearance_seq = p.appearance_seq.wrapping_add(1);
        }
        op::SESSION_LOG => {
            // Engine-TS SESSION_LOG: record a session event. OS has no session
            // DB, so log it (eventType is offset by +2 like the reference).
            let event = state.pop_string();
            let event_type = state.pop_int() + 2;
            let p = active_player(state, world)?;
            dbg_log!("[session] {} type={event_type}: {event}", p.username);
        }
        op::WEALTH_EVENT => {
            // Engine-TS WEALTH_EVENT: record a wealth-transfer event. No wealth
            // DB in OS → log it.
            let name = state.pop_string();
            let [event_type, count, value] = state.pop_ints::<3>();
            let p = active_player(state, world)?;
            dbg_log!(
                "[wealth] {} type={event_type} {count}x{name} value={value}",
                p.username
            );
        }
        op::LAST_LOGIN_INFO => {
            // Engine-TS LAST_LOGIN_INFO: send the welcome-screen last-login info.
            // The rev1 packet is just the previous-login IP (0 until the conn
            // layer records it).
            let p = active_player(state, world)?;
            let ip = p.last_login_ip;
            p.write(msg::last_login_info(ip));
        }
        op::HUNTALL => {
            // Engine-TS HUNTALL: gather every player within `distance` (chebyshev)
            // of the coord on its level, optionally filtered by line-of-sight
            // (checkVis 1) or line-of-walk (2). Stored nearest-last so HUNTNEXT's
            // pop() yields nearest first.
            let [coord, distance, check_vis] = state.pop_ints::<3>();
            let (cx, cz, clevel) = unpack_coord(coord);
            let mut found: Vec<(usize, i32)> = world
                .players
                .iter()
                .enumerate()
                .filter_map(|(pid, slot)| {
                    let p = slot.as_ref()?;
                    let e = &p.entity;
                    if e.level != clevel {
                        return None;
                    }
                    let d = (e.x - cx).abs().max((e.z - cz).abs());
                    if d > distance {
                        return None;
                    }
                    let vis_ok = match check_vis {
                        1 => world.collision.line_of_sight(clevel, e.x, e.z, cx, cz),
                        2 => world.collision.line_of_walk(clevel, e.x, e.z, cx, cz),
                        _ => true,
                    };
                    vis_ok.then_some((pid, d))
                })
                .collect();
            found.sort_by_key(|&(_, d)| d);
            state.player_iterator = found.into_iter().rev().map(|(pid, _)| pid).collect();
        }
        op::HUNTNEXT => {
            // Engine-TS HUNTNEXT: advance the player iterator, setting the next
            // player active (push 1), or push 0 when exhausted.
            match state.player_iterator.pop() {
                Some(pid) => {
                    let secondary = state.secondary();
                    if secondary {
                        state.active_player2 = Some(pid);
                    } else {
                        state.active_player = Some(pid);
                    }
                    state.pointer_add(crate::script::state::ACTIVE_PLAYER[secondary as usize]);
                    state.push_int(1);
                }
                None => state.push_int(0),
            }
        }
        op::STAT => {
            let stat = state.pop_int() as usize;
            let p = active_player(state, world)?;
            let v = p.levels.get(stat).copied().unwrap_or(0);
            state.push_int(v);
        }
        op::STAT_BASE => {
            let stat = state.pop_int() as usize;
            let p = active_player(state, world)?;
            let v = p.base_levels.get(stat).copied().unwrap_or(0);
            state.push_int(v);
        }
        op::STAT_TOTAL => {
            let p = active_player(state, world)?;
            let total: i32 = p.base_levels.iter().sum();
            state.push_int(total);
        }
        // STAT_ADD/SUB/BOOST/DRAIN/HEAL: apply the op to the current level and,
        // when it changes the level, fire the stat's [changestat] script — 1:1
        // with Engine-TS, whose stat ops call `changeStat` on a change.
        op::STAT_ADD | op::STAT_SUB | op::STAT_BOOST | op::STAT_DRAIN | op::STAT_HEAL => {
            let [stat, constant, percent] = state.pop_ints::<3>();
            let pid = state.active_player.ok_or("no active_player")?;
            if (stat as usize) < player::STAT_COUNT {
                let s = stat as usize;
                let changed = world.players[pid].as_mut().is_some_and(|p| match opcode {
                    op::STAT_ADD => p.stat_add(s, constant, percent),
                    op::STAT_SUB => p.stat_sub(s, constant, percent),
                    op::STAT_BOOST => p.stat_boost(s, constant, percent),
                    op::STAT_DRAIN => p.stat_drain(s, constant, percent),
                    _ => p.stat_heal(s, constant, percent),
                });
                if changed {
                    world.fire_changestat(pid, stat);
                }
            }
        }
        op::STAT_RANDOM => {
            // Skill success roll — 1:1 with Engine-TS STAT_RANDOM. Reads the
            // player's CURRENT level (boosts/drains count), interpolates the
            // success value between the low/high bounds, and compares it to a
            // 0..=255 roll: succeeds (pushes 1) when value > roll.
            let [stat, low, high] = state.pop_ints::<3>();
            let p = active_player(state, world)?;
            let level = *p
                .levels
                .get(stat as usize)
                .ok_or("stat_random: stat out of range")?;
            let value = stat_random_value(level, low, high);
            let chance = (rand_unit() * 256.0) as i32;
            state.push_int(if value > chance { 1 } else { 0 });
        }
        op::SET_SKILL_LEVEL => {
            // Engine-TS SET_SKILL_LEVEL: set the appearance "skill level" shown
            // in the right-click name ("Name (skill-N)"). Just sets the field —
            // the deob does not rebuild the appearance here.
            let level = state.pop_int();
            let p = active_player(state, world)?;
            p.skill_level = level;
        }
        op::STAT_ADVANCE => {
            // Engine-TS STAT_ADVANCE: award experience to a skill (the XP path,
            // distinct from STAT_ADD which only nudges the current level). Goes
            // through World::give_xp so a level-up fires the ADVANCESTAT /
            // CHANGESTAT engine-queue scripts.
            let [stat, xp] = state.pop_ints::<2>();
            let pid = state.active_player.ok_or("no active_player")?;
            world.give_xp(pid, stat, xp);
        }
        op::DAMAGE => {
            // Engine-TS DAMAGE: resolve the target player by uid (not the active
            // player) and apply a hit, flagging the DAMAGE/DAMAGE2 mask.
            let [uid, dtype, amount] = state.pop_ints::<3>();
            if let Some(pid) = world.get_player_by_uid(uid) {
                if let Some(p) = world.players[pid].as_mut() {
                    p.apply_damage(amount, dtype);
                }
            }
        }
        op::HEALENERGY => {
            let amount = state.pop_int();
            let p = active_player(state, world)?;
            p.heal_energy(amount);
        }
        op::P_DELAY => {
            // Engine-TS P_DELAY: action-lock the player for N ticks and suspend
            // this script — the world resumes it once the lock elapses.
            let n = state.pop_int();
            let pid = state.active_player.ok_or("no active_player")?;
            let resume = world.tick as i32 + 1 + n.max(0);
            if let Some(p) = world.players[pid].as_mut() {
                p.delayed_until = resume;
            }
            state.delay = n;
            state.execution = crate::script::state::Execution::Suspended;
        }
        op::P_ARRIVEDELAY => {
            // Engine-TS P_ARRIVEDELAY: if the player took no step this tick there's
            // no arrival to wait on, so the script falls through; otherwise lock
            // for one tick and suspend so the just-finished step settles before the
            // script continues. https://x.com/JagexAsh/status/1648254846686904321
            state.pointer_check(Pointer::ProtectedActivePlayer)?;
            let pid = state.active_player.ok_or("no active_player")?;
            let tick = world.tick as i32;
            let moved = world.players[pid].as_ref()
                .map_or(false, |p| p.entity.last_movement >= tick);
            if moved {
                if let Some(p) = world.players[pid].as_mut() {
                    p.delayed_until = tick + 1;
                }
                state.delay = 0;
                state.execution = crate::script::state::Execution::Suspended;
            }
        }
        op::NPC_ARRIVEDELAY => {
            // Engine-TS NPC_ARRIVEDELAY: an npc that stepped within the last tick
            // waits for it to settle — 1 tick if it moved a tick ago, 2 if it moved
            // this tick; otherwise no delay.
            // https://x.com/JagexAsh/status/1432296606376906752
            state.pointer_check(Pointer::ActiveNpc)?;
            let nid = state.active_npc.ok_or("no active_npc")?;
            let tick = world.tick as i32;
            let lm = world.npcs[nid].as_ref().map_or(i32::MIN, |n| n.entity.last_movement);
            if lm >= tick - 1 {
                let resume = if lm == tick - 1 { tick + 1 } else { tick + 2 };
                if let Some(n) = world.npcs[nid].as_mut() {
                    n.delayed_until = resume;
                }
                state.delay = resume - tick - 1;
                state.execution = crate::script::state::Execution::NpcSuspended;
            }
        }
        op::WORLD_DELAY => {
            // Engine-TS WORLD_DELAY: suspend this world script. The delay stays
            // on the stack; process_world_queue pops it when re-queuing.
            state.execution = crate::script::state::Execution::WorldSuspended;
        }
        op::NPC_SETTIMER => {
            // Engine-TS NPC_SETTIMER: (re)arm the active npc's [ai_timer] cadence.
            let interval = state.pop_int();
            active_npc(state, world)?.set_timer(interval);
        }
        op::NPC_RANGE => {
            // Engine-TS NPC_RANGE: Chebyshev distance from the active npc to a
            // coord, or -1 across planes. (Npcs are 1x1 without config, so the
            // footprint-aware distanceTo reduces to max(|dx|, |dz|).)
            let coord = state.pop_int();
            let (cx, cz, clevel) = ((coord >> 14) & 0x3fff, coord & 0x3fff, (coord >> 28) & 0x3);
            let (nx, nz, nlevel) = {
                let npc = active_npc(state, world)?;
                (npc.entity.x, npc.entity.z, npc.entity.level)
            };
            let dist = if clevel != nlevel {
                -1
            } else {
                (nx - cx).abs().max((nz - cz).abs())
            };
            state.push_int(dist);
        }
        op::NPC_FINDEXACT => {
            // Engine-TS NPC_FINDEXACT: set the active npc to one of `type` at the
            // exact coord (push 1), else 0.
            let [coord, type_id] = state.pop_ints::<2>();
            let (cx, cz, clevel) = ((coord >> 14) & 0x3fff, coord & 0x3fff, (coord >> 28) & 0x3);
            let found = world.nearby_npc_ids(cx, cz, clevel).into_iter().find(|&nid| {
                world.npcs[nid].as_ref().map_or(false, |n| {
                    n.active && n.type_id == type_id
                        && n.entity.x == cx && n.entity.z == cz && n.entity.level == clevel
                })
            });
            match found {
                Some(nid) => {
                    let secondary = state.secondary();
                    if secondary {
                        state.active_npc2 = Some(nid);
                    } else {
                        state.active_npc = Some(nid);
                    }
                    state.pointer_add(crate::script::state::ACTIVE_NPC[secondary as usize]);
                    state.push_int(1);
                }
                None => state.push_int(0),
            }
        }
        op::NPC_FIND => {
            // Engine-TS NPC_FIND: the single closest active npc of `type` within
            // `distance` of a coord, by euclidean-squared distance; sets the active
            // npc and pushes 1, else pushes 0. The huntvis (line-of-sight) check
            // awaits collision, as with NPC_FINDALL.
            let [coord, type_id, distance, _checkvis] = state.pop_ints::<4>();
            let (cx, cz, clevel) = ((coord >> 14) & 0x3fff, coord & 0x3fff, (coord >> 28) & 0x3);
            let closest = world
                .npcs_within(cx, cz, clevel, distance)
                .into_iter()
                .filter_map(|nid| {
                    world.npcs[nid].as_ref().and_then(|n| {
                        let (dx, dz) = (n.entity.x - cx, n.entity.z - cz);
                        (n.active
                            && n.type_id == type_id
                            && n.entity.level == clevel
                            && dx.abs().max(dz.abs()) <= distance)
                            .then_some((nid, dx * dx + dz * dz))
                    })
                })
                .min_by_key(|&(_, d2)| d2)
                .map(|(nid, _)| nid);
            match closest {
                Some(nid) => {
                    let secondary = state.secondary();
                    if secondary {
                        state.active_npc2 = Some(nid);
                    } else {
                        state.active_npc = Some(nid);
                    }
                    state.pointer_add(crate::script::state::ACTIVE_NPC[secondary as usize]);
                    state.push_int(1);
                }
                None => state.push_int(0),
            }
        }
        op::NPC_WALKTRIGGER => {
            // Engine-TS NPC_WALKTRIGGER: arm an [ai_queue<N>] to fire when the
            // npc next walks. Stack: queue_id (1-20), arg.
            let [queue_id, arg] = state.pop_ints::<2>();
            if !(1..=20).contains(&queue_id) {
                return Err(format!("NPC_WALKTRIGGER queue id out of range: {queue_id}"));
            }
            let npc = active_npc(state, world)?;
            npc.walk_trigger = queue_id - 1;
            npc.walktrigger_arg = arg;
        }
        op::NPC_QUEUE => {
            // Engine-TS NPC_QUEUE: queue an [ai_queue<N>] script on the active
            // npc. Stack: queue_id (1-20), arg, delay.
            let delay = state.pop_int();
            let arg = state.pop_int();
            let queue_id = state.pop_int();
            if !(1..=20).contains(&queue_id) {
                return Err(format!("NPC_QUEUE queue id out of range: {queue_id}"));
            }
            let trigger = (crate::script::trigger::AI_QUEUE1 as i32 + queue_id - 1) as u16;
            active_npc(state, world)?.enqueue_script(trigger, delay, arg);
        }
        op::NPC_DELAY => {
            // Engine-TS NPC_DELAY: action-lock the active npc for N ticks and
            // suspend this AI script — the world resumes it once the lock elapses
            // (the npc parallel of P_DELAY).
            let n = state.pop_int();
            let nid = state.active_npc.ok_or("no active_npc")?;
            let resume = world.tick as i32 + 1 + n.max(0);
            if let Some(npc) = world.npcs.get_mut(nid).and_then(|o| o.as_mut()) {
                npc.delayed_until = resume;
            }
            state.delay = n;
            state.execution = crate::script::state::Execution::NpcSuspended;
        }
        op::P_PAUSEBUTTON => {
            // Engine-TS P_PAUSEBUTTON: pause the script until the player clicks
            // a continue button (RESUME_PAUSEBUTTON resumes it). No tick delay.
            state.execution = crate::script::state::Execution::PauseButton;
        }
        op::P_STOPACTION => {
            // Engine-TS P_STOPACTION → stopAction(): end the current interaction,
            // close any open modal, and drop the client's minimap walk flag.
            state.pointer_check(Pointer::ProtectedActivePlayer)?;
            let pid = state.active_player.ok_or("no active_player")?;
            if let Some(p) = world.players.get_mut(pid).and_then(|o| o.as_mut()) {
                p.interaction = None;
                p.entity.clear_interaction();
                p.unset_map_flag();
            }
            world.close_modal(pid);
        }
        op::P_CLEARPENDINGACTION => {
            // Engine-TS P_CLEARPENDINGACTION → clearPendingAction(): end the
            // interaction and close any modal, but leave the walk queue intact
            // (P_STOPACTION additionally drops the minimap flag).
            state.pointer_check(Pointer::ProtectedActivePlayer)?;
            let pid = state.active_player.ok_or("no active_player")?;
            if let Some(p) = world.players.get_mut(pid).and_then(|o| o.as_mut()) {
                p.interaction = None;
                p.entity.clear_interaction();
            }
            world.close_modal(pid);
        }
        op::P_OPLOC | op::P_OPNPC | op::P_OPOBJ | op::P_OPPLAYER => {
            // Engine-TS P_OPLOC/OPNPC/OPOBJ/OPPLAYER: begin an op-N interaction
            // with the active loc/npc/obj/secondary-player. `type` is 1-based
            // (1..5). Locs/objs/npcs only interact if the type has that op slot.
            // Locs and objs queue an initial waypoint toward the target.
            state.pointer_check(Pointer::ProtectedActivePlayer)?;
            let ty = state.pop_int() - 1;
            if !(0..5).contains(&ty) {
                return Err(format!("invalid op index: {}", ty + 1));
            }
            let pid = state.active_player.ok_or("no active_player")?;
            match opcode {
                op::P_OPLOC => {
                    let (x, z, level, id, shape, angle) = state.active_loc.ok_or("no active_loc")?;
                    let has = world.loc_info.get(&id).and_then(|i| i.op.get(ty as usize)).is_some_and(|o| o.is_some());
                    if has {
                        world.stop_action(pid);
                        let target = player::InteractTarget::Loc { x, z, level, id, shape, angle };
                        if !world.in_operable_distance(pid, target) {
                            if let Some(p) = world.players[pid].as_mut() {
                                p.entity.queue_waypoints(&[(x, z)]);
                            }
                        }
                        world.set_interaction(pid, target, trigger::APLOC1 as i32 + ty, -1);
                    }
                }
                op::P_OPOBJ => {
                    let (x, z, level, id) = state.active_obj.ok_or("no active_obj")?;
                    let has = world.obj_info.get(&id).and_then(|i| i.op.get(ty as usize)).is_some_and(|o| o.is_some());
                    if has {
                        world.stop_action(pid);
                        let target = player::InteractTarget::Obj { x, z, level, id };
                        if !world.in_operable_distance(pid, target) {
                            if let Some(p) = world.players[pid].as_mut() {
                                p.entity.queue_waypoints(&[(x, z)]);
                            }
                        }
                        world.set_interaction(pid, target, trigger::APOBJ1 as i32 + ty, -1);
                    }
                }
                op::P_OPNPC => {
                    let nid = state.active_npc.ok_or("no active_npc")?;
                    let type_id = world.npcs.get(nid).and_then(|o| o.as_ref()).map(|n| n.type_id);
                    let has = type_id
                        .and_then(|t| world.npc_info.get(&t))
                        .and_then(|i| i.op.get(ty as usize))
                        .is_some_and(|o| o.is_some());
                    if has {
                        world.stop_action(pid);
                        world.set_interaction(pid, player::InteractTarget::Npc(nid), trigger::APNPC1 as i32 + ty, -1);
                    }
                }
                _ => {
                    // P_OPPLAYER
                    let p2 = state.active_player2.ok_or("no active_player2")?;
                    world.stop_action(pid);
                    world.set_interaction(pid, player::InteractTarget::Player(p2), trigger::APPLAYER1 as i32 + ty, -1);
                }
            }
        }
        op::P_OPNPCT => {
            // Engine-TS P_OPNPCT: cast spell `com` on the active npc.
            state.pointer_check(Pointer::ProtectedActivePlayer)?;
            let com = state.pop_int();
            let nid = state.active_npc.ok_or("no active_npc")?;
            let pid = state.active_player.ok_or("no active_player")?;
            world.stop_action(pid);
            world.set_interaction(pid, player::InteractTarget::Npc(nid), trigger::APNPCT as i32, com);
        }
        op::P_OPPLAYERT => {
            // Engine-TS P_OPPLAYERT: cast spell `com` on the secondary player.
            state.pointer_check(Pointer::ProtectedActivePlayer)?;
            let com = state.pop_int();
            let p2 = state.active_player2.ok_or("no active_player2")?;
            let pid = state.active_player.ok_or("no active_player")?;
            world.stop_action(pid);
            world.set_interaction(pid, player::InteractTarget::Player(p2), trigger::APPLAYERT as i32, com);
        }
        op::WEIGHT => {
            // Engine-TS WEIGHT: push the player's run weight (grams).
            let w = active_player(state, world)?.run_weight;
            state.push_int(w);
        }
        op::AFK_EVENT => {
            // Engine-TS AFK_EVENT: push 1 once the world's periodic roll has armed
            // a random event for this (non-staff) player, then clear the flag so it
            // fires only once. Staff at level >= 2 are exempt.
            let p = active_player(state, world)?;
            let ready = p.staff_mod_level < 2 && p.afk_event_ready;
            p.afk_event_ready = false;
            state.push_int(ready as i32);
        }
        op::BUSY => {
            // Engine-TS BUSY: 1 if the player is busy() or logging out.
            let pid = state.active_player.ok_or("no active_player")?;
            state.push_int(world.player_busy(pid) as i32);
        }
        op::BUSY2 => {
            // Engine-TS BUSY2: 1 if the player has a pending interaction or is
            // still walking (hasInteraction() || hasWaypoints()).
            let p = active_player(state, world)?;
            let busy = p.entity.target != -1 || !p.entity.waypoints.is_empty();
            state.push_int(busy as i32);
        }
        op::FINDHERO => {
            // Engine-TS FINDHERO: resolve the active player's top PvP damage
            // dealer as the secondary active player (push 1), else 0.
            let pid = state.active_player.ok_or("no active_player")?;
            let hero = world.players.get(pid).and_then(|o| o.as_ref())
                .and_then(|p| p.find_hero());
            match hero {
                Some(hp) if world.players.get(hp).is_some_and(|p| p.is_some()) => {
                    state.active_player2 = Some(hp);
                    state.pointer_add(Pointer::ActivePlayer2);
                    state.push_int(1);
                }
                _ => state.push_int(0),
            }
        }
        op::BOTH_HEROPOINTS => {
            // Engine-TS BOTH_HEROPOINTS: credit `damage` from one active player
            // to the other's PvP tally; `.command` flips the from/to direction.
            let damage = state.pop_int();
            let (from, to) = if state.secondary() {
                (state.active_player2, state.active_player)
            } else {
                (state.active_player, state.active_player2)
            };
            let from_pid = from.ok_or("no from player")?;
            let to_pid = to.ok_or("no to player")?;
            if let Some(p) = world.players.get_mut(to_pid).and_then(|o| o.as_mut()) {
                p.add_hero(from_pid, damage);
            }
        }
        op::P_COUNTDIALOG => {
            // Engine-TS P_COUNTDIALOG: pause until the player submits an amount
            // (RESUME_P_COUNTDIALOG resumes it, exposing the value via LAST_INT).
            // The "enter amount" input is opened by the content's IF_OPENCHAT
            // interface, so this op only suspends — no open packet at this rev.
            state.execution = crate::script::state::Execution::CountDialog;
        }
        op::SETTIMER | op::SOFTTIMER => {
            // Engine-TS SETTIMER / SOFTTIMER: register a repeating timer script.
            // Stack (bottom→top): timer_id, interval, <args...>, typespec.
            let kind = if opcode == op::SETTIMER {
                player::TimerKind::Normal
            } else {
                player::TimerKind::Soft
            };
            let args = pop_script_args(state);
            let interval = state.pop_int();
            let timer_id = state.pop_int();
            let script = world.scripts.as_ref()
                .and_then(|s| s.get(timer_id))
                .ok_or_else(|| format!("Unable to find timer script: {timer_id}"))?;
            let now = world.tick;
            let p = active_player(state, world)?;
            p.set_timer(kind, script, args, interval, now);
        }
        op::CLEARTIMER | op::CLEARSOFTTIMER => {
            // Engine-TS CLEARTIMER / CLEARSOFTTIMER: drop the timer by its id
            // (both opcodes call the same clearTimer — the kind is implicit in id).
            let timer_id = state.pop_int();
            active_player(state, world)?.clear_timer(timer_id);
        }
        op::GETTIMER => {
            // Engine-TS GETTIMER: push the timer's clock (last-fire tick), or -1
            // when no timer with that id is registered.
            let timer_id = state.pop_int();
            // Resolve the id is unnecessary for lookup, but the reference errors
            // when the script id doesn't exist at all, so mirror that.
            if world.scripts.as_ref().and_then(|s| s.get(timer_id)).is_none() {
                return Err(format!("Unable to find timer script: {timer_id}"));
            }
            let clock = active_player(state, world)?.timers.get(&timer_id)
                .map_or(-1, |t| t.clock as i32);
            state.push_int(clock);
        }
        op::QUEUE | op::STRONGQUEUE | op::WEAKQUEUE => {
            // Engine-TS QUEUE / STRONGQUEUE / WEAKQUEUE: queue a script with a
            // single int arg. Stack (bottom→top): script_id, delay, arg.
            let kind = match opcode {
                op::STRONGQUEUE => player::QueueKind::Strong,
                op::WEAKQUEUE => player::QueueKind::Weak,
                _ => player::QueueKind::Normal,
            };
            let [script_id, delay, arg] = state.pop_ints::<3>();
            let script = world.scripts.as_ref().and_then(|s| s.get(script_id))
                .ok_or_else(|| format!("Unable to find queue script: {script_id}"))?;
            active_player(state, world)?
                .enqueue_script(kind, script, vec![ScriptArg::Int(arg)], delay);
        }
        op::QUEUEVARARG | op::STRONGQUEUEVARARG | op::WEAKQUEUEVARARG => {
            // The vararg variants pop a typespec-described arg list instead.
            let kind = match opcode {
                op::STRONGQUEUEVARARG => player::QueueKind::Strong,
                op::WEAKQUEUEVARARG => player::QueueKind::Weak,
                _ => player::QueueKind::Normal,
            };
            let args = pop_script_args(state);
            let [script_id, delay] = state.pop_ints::<2>();
            let script = world.scripts.as_ref().and_then(|s| s.get(script_id))
                .ok_or_else(|| format!("Unable to find queue script: {script_id}"))?;
            active_player(state, world)?.enqueue_script(kind, script, args, delay);
        }
        op::LONGQUEUE => {
            // Engine-TS LONGQUEUE: a NORMAL-like queue with a leading logout-
            // action arg. Stack: script_id, delay, arg, logout_action.
            let [script_id, delay, arg, logout_action] = state.pop_ints::<4>();
            let script = world.scripts.as_ref().and_then(|s| s.get(script_id))
                .ok_or_else(|| format!("Unable to find queue script: {script_id}"))?;
            active_player(state, world)?.enqueue_script(
                player::QueueKind::Long, script,
                vec![ScriptArg::Int(logout_action), ScriptArg::Int(arg)], delay);
        }
        op::LONGQUEUEVARARG => {
            let args = pop_script_args(state);
            let [script_id, delay, logout_action] = state.pop_ints::<3>();
            let script = world.scripts.as_ref().and_then(|s| s.get(script_id))
                .ok_or_else(|| format!("Unable to find queue script: {script_id}"))?;
            let mut full = vec![ScriptArg::Int(logout_action)];
            full.extend(args);
            active_player(state, world)?
                .enqueue_script(player::QueueKind::Long, script, full, delay);
        }
        op::GETQUEUE => {
            // Engine-TS GETQUEUE: count queued scripts with this id (both queues).
            let script_id = state.pop_int();
            let n = active_player(state, world)?.count_queued(script_id);
            state.push_int(n);
        }
        op::CLEARQUEUE => {
            // Engine-TS CLEARQUEUE: unlink all queued scripts with this id.
            let script_id = state.pop_int();
            active_player(state, world)?.clear_queued_script(script_id);
        }
        op::WALKTRIGGER => {
            // Engine-TS WALKTRIGGER: arm the script to run when the player next
            // walks (fired and cleared by World::process_walktrigger).
            let script_id = state.pop_int();
            active_player(state, world)?.walk_trigger = script_id;
        }
        op::GETWALKTRIGGER => {
            // Engine-TS GETWALKTRIGGER: read the armed walk-trigger script id.
            let wt = active_player(state, world)?.walk_trigger;
            state.push_int(wt);
        }
        op::P_EXACTMOVE => {
            let [start, end, start_cycle, end_cycle, direction] = state.pop_ints::<5>();
            let p = active_player(state, world)?;
            // Engine-TS P_EXACTMOVE clears the minimap walk flag before taking
            // over movement, so the client drops its now-stale queued path.
            p.unset_map_flag();
            p.entity.set_exact_move(
                (start >> 14) & 0x3fff,
                start & 0x3fff,
                (end >> 14) & 0x3fff,
                end & 0x3fff,
                start_cycle,
                end_cycle,
                direction,
                player::MASK_EXACT_MOVE,
            );
        }
        op::SPOTANIM_MAP => {
            let [spotanim, coord, height, delay] = state.pop_ints::<4>();
            world.map_anim(
                spotanim,
                height,
                delay,
                (coord >> 14) & 0x3fff,
                coord & 0x3fff,
                (coord >> 28) & 0x3,
            );
        }
        op::PROJANIM_MAP => {
            let [src, dst, spotanim, src_h, dst_h, delay, duration, peak, arc] =
                state.pop_ints::<9>();
            world.map_projanim(
                spotanim,
                (src >> 14) & 0x3fff,
                src & 0x3fff,
                (dst >> 14) & 0x3fff,
                dst & 0x3fff,
                0, // tile-to-tile: no homing target
                src_h,
                dst_h,
                delay,
                duration,
                peak,
                arc,
                (src >> 28) & 0x3,
            );
        }
        op::INZONE => {
            // Is `pos` (c3) inside the inclusive box spanning corners c1..c2?
            // (COORDX/Y/Z, DISTANCE, MOVECOORD, PLAYERCOUNT already live in the
            // "Server ops" section above — INZONE is the only new one here.)
            let [c1, c2, c3] = state.pop_ints::<3>();
            let unpack = |c: i32| ((c >> 14) & 0x3fff, (c >> 28) & 0x3, c & 0x3fff);
            let (fx, fl, fz) = unpack(c1);
            let (tx, tl, tz) = unpack(c2);
            let (px, pl, pz) = unpack(c3);
            let inside = (fx..=tx).contains(&px)
                && (fl..=tl).contains(&pl)
                && (fz..=tz).contains(&pz);
            state.push_int(i32::from(inside));
        }
        op::FINDUID => {
            let uid = state.pop_int();
            match world.get_player_by_uid(uid) {
                Some(pid) => {
                    let idx = if state.secondary() { 1 } else { 0 };
                    if idx == 1 {
                        state.active_player2 = Some(pid);
                    } else {
                        state.active_player = Some(pid);
                    }
                    state.pointer_add(crate::script::state::ACTIVE_PLAYER[idx]);
                    state.push_int(1);
                }
                None => state.push_int(0),
            }
        }
        op::PROJANIM_PL => {
            let [src, uid, spotanim, src_h, dst_h, delay, duration, peak, arc] =
                state.pop_ints::<9>();
            if let Some(pid) = world.get_player_by_uid(uid) {
                let (dx, dz) = {
                    let e = &world.players[pid].as_ref().unwrap().entity;
                    (e.x, e.z)
                };
                world.map_projanim(
                    spotanim,
                    (src >> 14) & 0x3fff,
                    src & 0x3fff,
                    dx,
                    dz,
                    -(pid as i32) - 1, // player target encoding
                    src_h,
                    dst_h,
                    delay,
                    duration,
                    peak,
                    arc,
                    (src >> 28) & 0x3,
                );
            }
        }
        op::PROJANIM_NPC => {
            let [src, uid, spotanim, src_h, dst_h, delay, duration, peak, arc] =
                state.pop_ints::<9>();
            if let Some(nid) = world.get_npc_by_uid(uid) {
                let (dx, dz) = {
                    let e = &world.npcs[nid].as_ref().unwrap().entity;
                    (e.x, e.z)
                };
                world.map_projanim(
                    spotanim,
                    (src >> 14) & 0x3fff,
                    src & 0x3fff,
                    dx,
                    dz,
                    nid as i32 + 1, // npc target encoding
                    src_h,
                    dst_h,
                    delay,
                    duration,
                    peak,
                    arc,
                    (src >> 28) & 0x3,
                );
            }
        }
        op::COORD => {
            let p = active_player(state, world)?;
            let coord = ((p.entity.level & 0x3) << 28)
                | ((p.entity.x & 0x3fff) << 14)
                | (p.entity.z & 0x3fff);
            state.push_int(coord);
        }
        op::DISPLAYNAME | op::NAME => {
            let p = active_player(state, world)?;
            let name = p.display_name.clone();
            state.push_string(name);
        }
        op::UID => {
            // Engine-TS UID: the player's uid — the Base37 name hash packed above
            // the slot, NOT the raw slot. P_FINDUID / get_player_by_uid decode this
            // and validate the hash to detect a slot reused by another account, so
            // returning the bare pid would fail that check.
            let uid = active_player(state, world)?.uid();
            state.push_int(uid);
        }
        op::GENDER => {
            let p = active_player(state, world)?;
            let g = p.gender;
            state.push_int(g);
        }
        op::SETGENDER => {
            let g = state.pop_int();
            let p = active_player(state, world)?;
            p.gender = g;
        }
        op::SETIDKIT => {
            let [part, idk] = state.pop_ints::<2>();
            let p = active_player(state, world)?;
            if (0..7).contains(&part) {
                p.body[part as usize] = idk;
            }
        }
        op::SETSKINCOLOUR => {
            let colour = state.pop_int();
            let p = active_player(state, world)?;
            p.colours[4] = colour;
        }
        op::BUILDAPPEARANCE => {
            // Reference pops the inv to source worn equipment from;
            // no inventories yet — rebuild from body/colours.
            let _inv = state.pop_int();
            let p = active_player(state, world)?;
            p.build_appearance();
        }
        op::P_TELEJUMP => {
            state.pointer_check(Pointer::ProtectedActivePlayer)?;
            let coord = state.pop_int();
            let p = active_player(state, world)?;
            p.entity.teleport((coord >> 14) & 0x3fff, coord & 0x3fff, (coord >> 28) & 0x3, true);
        }
        op::P_TELEPORT => {
            state.pointer_check(Pointer::ProtectedActivePlayer)?;
            let coord = state.pop_int();
            let p = active_player(state, world)?;
            p.entity.teleport((coord >> 14) & 0x3fff, coord & 0x3fff, (coord >> 28) & 0x3, false);
        }
        op::P_RUN => {
            // Engine-TS P_RUN: set the *persistent* run toggle and sync the RUN
            // varp (the client's run orb). The move speed is derived from `run`
            // each tick by update_movement, so setting move_speed directly here (as
            // OS did) was overwritten by that recompute before the step ran — the
            // toggle had no effect and never persisted across ticks.
            state.pointer_check(Pointer::ProtectedActivePlayer)?;
            let run = state.pop_int();
            let p = active_player(state, world)?;
            p.run = run != 0;
            p.set_var(player::VARP_RUN, run);
        }
        op::P_WALK => {
            // Engine-TS P_WALK: queue the pathfinder's route to the coord. We
            // run the collision BFS (move-near) and queue its waypoints; with no
            // collision loaded the BFS returns empty and we queue the coord
            // directly (the pre-collision behaviour, and what unit tests rely on).
            state.pointer_check(Pointer::ProtectedActivePlayer)?;
            let coord = state.pop_int();
            let (dx, dz, _) = unpack_coord(coord);
            let (sx, sz, level) = {
                let p = active_player(state, world)?;
                (p.entity.x, p.entity.z, p.entity.level)
            };
            let path = world.collision.find_path(level, sx, sz, dx, dz, true);
            let p = active_player(state, world)?;
            if path.is_empty() {
                p.entity.queue_waypoints(&[(dx, dz)]);
            } else {
                p.entity.queue_waypoints(&path);
            }
        }
        op::P_LOGOUT => {
            // Engine-TS P_LOGOUT only *requests* a logout; process_logouts grants
            // it (respecting p_preventlogout) at the end of the player phase.
            state.pointer_check(Pointer::ProtectedActivePlayer)?;
            active_player(state, world)?.request_logout = true;
        }
        op::P_PREVENTLOGOUT => {
            // Engine-TS P_PREVENTLOGOUT: refuse logouts for `delay` ticks, showing
            // `message` if the player tries (the combat logout delay). Stack:
            // int(delay) then string(message); a short antilog may overwrite a
            // long one, so no ordering checks.
            state.pointer_check(Pointer::ProtectedActivePlayer)?;
            let message = state.pop_string();
            let delay = state.pop_int();
            let until = world.tick as i32 + delay;
            let p = active_player(state, world)?;
            p.prevent_logout_message = Some(message);
            p.prevent_logout_until = until;
        }
        op::RUNENERGY => {
            // Engine-TS RUNENERGY is a *getter* — push the player's run energy
            // (0..=10000). OS previously implemented it as a setter, which both
            // corrupted the stack for a reader and silently overwrote the energy.
            let energy = active_player(state, world)?.run_energy;
            state.push_int(energy);
        }
        op::HEADICONS_GET => {
            let p = active_player(state, world)?;
            // Packed prayer icon (the reference packs both).
            state.push_int(p.headicon_prayer.max(0));
        }
        op::HEADICONS_SET => {
            let icons = state.pop_int();
            let p = active_player(state, world)?;
            p.headicon_prayer = icons;
            p.build_appearance();
        }
        op::MINIMAP_TOGGLE => {
            // Engine-TS MINIMAP_TOGGLE: set the client's minimap visibility.
            let kind = state.pop_int();
            let p = active_player(state, world)?;
            p.write(msg::minimap_toggle(kind));
        }
        op::MIDI_SONG => {
            let id = state.pop_int();
            let p = active_player(state, world)?;
            // Engine-TS skips music for a low-detail client (saves its memory).
            if !p.low_memory {
                p.write(msg::midi_song(id));
            }
        }
        op::MIDI_JINGLE => {
            // Reference pops (delay, jingle) — rev1 wire carries only
            // the id; the trailing bytes are unused by the client.
            let [_delay, id] = state.pop_ints::<2>();
            let p = active_player(state, world)?;
            if !p.low_memory {
                p.write(msg::midi_jingle(id));
            }
        }
        op::SOUND_SYNTH => {
            let [synth, loops, delay] = state.pop_ints::<3>();
            let p = active_player(state, world)?;
            if !p.low_memory {
                p.write(msg::synth_sound(synth, loops, delay));
            }
        }
        op::STAFFMODLEVEL => {
            // Engine-TS STAFFMODLEVEL: the active player's staff/mod level. Reads
            // the same field AFK_EVENT gates on (0 until the rights system lands),
            // rather than a hardcoded constant.
            let level = active_player(state, world)?.staff_mod_level;
            state.push_int(level);
        }
        op::LOWMEM => {
            // Engine-TS LOWMEM: 1 if the active player is on a low-detail client.
            let low = active_player(state, world)?.low_memory;
            state.push_int(low as i32);
        }
        op::PLAYERMEMBER => {
            state.push_int(1);
        }

        // ── NPC ops ───────────────────────────────────────────────
        op::NPC_ADD => {
            let [coord, type_id, duration] = state.pop_ints::<3>();
            let _ = duration; // despawn timers land with npc modes
            let (x, z, level) = ((coord >> 14) & 0x3fff, coord & 0x3fff, (coord >> 28) & 0x3);
            if let Some(nid) = world.add_npc(type_id, x, z, level) {
                state.active_npc = Some(nid);
                state.pointer_add(Pointer::ActiveNpc);
            }
        }
        op::NPC_DEL => {
            state.pointer_check(Pointer::ActiveNpc)?;
            let nid = state.active_npc.ok_or("no active_npc")?;
            world.remove_npc(nid);
        }
        op::NPC_ANIM => {
            let [seq, delay] = state.pop_ints::<2>();
            let n = active_npc(state, world)?;
            n.entity.anim_id = seq;
            n.entity.anim_delay = delay;
            n.entity.masks |= npc::MASK_ANIM;
        }
        op::NPC_SAY => {
            // Engine-TS `Npc.say` guards empty text (unlike `Player.say`, which
            // doesn't): an empty NPC_SAY is a no-op — no overhead message and no
            // SAY mask. The ActiveNpc requirement still applies either way.
            let text = state.pop_string();
            let n = active_npc(state, world)?;
            if !text.is_empty() {
                n.entity.chat = Some(text);
                n.entity.masks |= npc::MASK_SAY;
            }
        }
        op::NPC_FACESQUARE => {
            let coord = state.pop_int();
            let tx = (coord >> 14) & 0x3fff;
            let tz = coord & 0x3fff;
            let n = active_npc(state, world)?;
            n.entity.face_x = tx;
            n.entity.face_z = tz;
            // Persistent orientation too — see FACESQUARE (Engine-TS focus).
            n.entity.face_angle_x = tx;
            n.entity.face_angle_z = tz;
            n.entity.masks |= npc::MASK_FACE_COORD;
        }
        op::NPC_COORD => {
            let n = active_npc(state, world)?;
            let coord = ((n.entity.level & 0x3) << 28)
                | ((n.entity.x & 0x3fff) << 14)
                | (n.entity.z & 0x3fff);
            state.push_int(coord);
        }
        op::NPC_DAMAGE => {
            // Engine-TS NPC_DAMAGE: a sourceless hit on the active npc (push
            // order is type then amount; the deob pops amount first).
            let [dtype, amount] = state.pop_ints::<2>();
            let n = active_npc(state, world)?;
            n.apply_damage(amount, dtype, None);
        }
        op::NPC_STAT => {
            let stat = state.pop_int();
            let n = active_npc(state, world)?;
            let v = *n.levels.get(stat as usize).ok_or("npc_stat: stat out of range")?;
            state.push_int(v);
        }
        op::NPC_BASESTAT => {
            let stat = state.pop_int();
            let n = active_npc(state, world)?;
            let v = *n.base_levels.get(stat as usize)
                .ok_or("npc_basestat: stat out of range")?;
            state.push_int(v);
        }
        op::NPC_STATADD => {
            let [stat, constant, percent] = state.pop_ints::<3>();
            let n = active_npc(state, world)?;
            if (stat as usize) < npc::NPC_STAT_COUNT {
                n.stat_add(stat as usize, constant, percent);
            }
        }
        op::NPC_STATSUB => {
            let [stat, constant, percent] = state.pop_ints::<3>();
            let n = active_npc(state, world)?;
            if (stat as usize) < npc::NPC_STAT_COUNT {
                n.stat_sub(stat as usize, constant, percent);
            }
        }
        op::NPC_STATHEAL => {
            let [stat, constant, percent] = state.pop_ints::<3>();
            let n = active_npc(state, world)?;
            if (stat as usize) < npc::NPC_STAT_COUNT {
                n.stat_heal(stat as usize, constant, percent);
            }
        }
        op::NPC_HEROPOINTS => {
            // Engine-TS NPC_HEROPOINTS: credit the active player's hit on the
            // active npc's damage tally (kill/loot ownership).
            let points = state.pop_int();
            let pid = state.active_player.ok_or("no active_player")?;
            let n = active_npc(state, world)?;
            n.add_hero(pid, points);
        }
        op::NPC_FINDHERO => {
            // Engine-TS NPC_FINDHERO: resolve the top damage dealer, set it as
            // the active player and push 1; push 0 (and leave the pointer) when
            // nobody qualifies or the dealer has since logged out.
            let hero = active_npc(state, world)?.find_hero();
            match hero {
                Some(pid) if world.players.get(pid).is_some_and(|p| p.is_some()) => {
                    state.active_player = Some(pid);
                    state.push_int(1);
                }
                _ => state.push_int(0),
            }
        }
        op::NPC_TYPE => {
            let n = active_npc(state, world)?;
            let t = n.type_id;
            state.push_int(t);
        }
        op::NPC_CHANGETYPE | op::NPC_CHANGETYPE_KEEPALL => {
            let type_id = state.pop_int();
            let n = active_npc(state, world)?;
            n.change_type(type_id);
        }
        op::NPC_TELE | op::NPC_TELEJUMP => {
            // NPC_TELE snaps but only *jumps* on a level change (jump=false);
            // NPC_TELEJUMP always jumps — 1:1 with the player P_TELEPORT /
            // P_TELEJUMP split (Engine-TS `teleport` vs `teleJump`). OS previously
            // forced a jump for both.
            let coord = state.pop_int();
            let jump = opcode == op::NPC_TELEJUMP;
            let n = active_npc(state, world)?;
            n.entity.teleport((coord >> 14) & 0x3fff, coord & 0x3fff, (coord >> 28) & 0x3, jump);
        }
        op::NPC_WALK => {
            // Engine-TS NPC_WALK: pathfind to the coord (move-near), falling back
            // to a direct waypoint when no collision is loaded (see P_WALK).
            let coord = state.pop_int();
            let (dx, dz, _) = unpack_coord(coord);
            let (sx, sz, level) = {
                let n = active_npc(state, world)?;
                (n.entity.x, n.entity.z, n.entity.level)
            };
            let path = world.collision.find_path(level, sx, sz, dx, dz, true);
            let n = active_npc(state, world)?;
            if path.is_empty() {
                n.entity.queue_waypoints(&[(dx, dz)]);
            } else {
                n.entity.queue_waypoints(&path);
            }
        }
        op::SPOTANIM_NPC => {
            let [spot, height, delay] = state.pop_ints::<3>();
            let n = active_npc(state, world)?;
            n.entity.spotanim_id = spot;
            n.entity.spotanim_height = height;
            n.entity.spotanim_delay = delay;
            n.entity.masks |= npc::MASK_SPOTANIM;
        }
        op::NPC_UID => {
            let n = active_npc(state, world)?;
            state.push_int(n.nid as i32);
        }
        op::NPC_GETMODE => {
            // Engine-TS NPC_GETMODE: the active npc's current AI mode.
            state.push_int(active_npc(state, world)?.mode);
        }
        op::NPC_SETHUNT => {
            // Engine-TS NPC_SETHUNT: set the active npc's hunt search radius.
            let range = state.pop_int();
            active_npc(state, world)?.hunt_range = range;
        }
        op::NPC_SETHUNTMODE => {
            // Engine-TS NPC_SETHUNTMODE: set the hunt type id (-1 clears).
            // Engine-TS validates against HuntType config; OS has no HuntType
            // decode, so the id is stored unvalidated (the hunt system that
            // consumes it lands later).
            let hunt = state.pop_int();
            active_npc(state, world)?.hunt_mode = hunt;
        }
        op::NPC_HASOP => {
            // Engine-TS NPC_HASOP: 1 if the active npc's type has op slot `op`
            // (1-based), else 0.
            let op_idx = state.pop_int();
            let type_id = active_npc(state, world)?.type_id;
            let has = world
                .npc_info
                .get(&type_id)
                .and_then(|i| usize::try_from(op_idx - 1).ok().and_then(|k| i.op.get(k)))
                .is_some_and(|o| o.is_some());
            state.push_int(i32::from(has));
        }
        op::NPC_SETMODE => {
            // Engine-TS NPC_SETMODE: set the npc's AI mode. NONE/WANDER/PATROL
            // (0/1/2) and NULL (-1) just clear the target; the higher modes aim
            // it at the active player (3..=16), loc (17..=26), obj (27..=36), or
            // secondary npc (37+). An absent target resets to NONE.
            let mode = state.pop_int();
            let nid = state.active_npc.ok_or("no active_npc")?;
            if mode == -1 || mode <= 2 {
                if let Some(n) = world.npcs[nid].as_mut() {
                    n.mode = mode.max(0);
                    n.target = None;
                    n.entity.clear_interaction();
                    n.entity.waypoints.clear();
                }
            } else {
                let target = if mode >= 37 {
                    state.active_npc2.map(player::InteractTarget::Npc)
                } else if mode >= 27 {
                    state.active_obj.map(|(x, z, level, id)| player::InteractTarget::Obj { x, z, level, id })
                } else if mode >= 17 {
                    state
                        .active_loc
                        .map(|(x, z, level, id, shape, angle)| player::InteractTarget::Loc { x, z, level, id, shape, angle })
                } else {
                    state.active_player.map(player::InteractTarget::Player)
                };
                if let Some(n) = world.npcs[nid].as_mut() {
                    match target {
                        Some(t) => {
                            n.mode = mode;
                            n.target = Some(t);
                            // Engine-TS setInteraction sets the face once; the
                            // client then tracks the entity/coord.
                            match t {
                                player::InteractTarget::Player(p2) => n.entity.set_face_entity(p2 as i32 + 32768),
                                player::InteractTarget::Npc(t2) => n.entity.set_face_entity(t2 as i32),
                                player::InteractTarget::Loc { x, z, .. }
                                | player::InteractTarget::Obj { x, z, .. } => n.entity.set_face_coord_target(x, z),
                            }
                        }
                        None => {
                            n.mode = 0;
                            n.target = None;
                            n.entity.clear_interaction();
                        }
                    }
                }
            }
        }
        op::NPC_INRANGE => {
            // Engine-TS NPC_INRANGE: targetWithinMaxRange for the active npc.
            let nid = state.active_npc.ok_or("no active_npc")?;
            state.push_int(i32::from(world.npc_target_within_maxrange(nid)));
        }
        op::NPC_HUNTALL => {
            // Engine-TS NPC_HUNTALL: gather npcs within `distance` (chebyshev) of
            // the coord on its level, optionally line-of-sight/walk filtered, into
            // the npc iterator (nearest-last, so NPC_FINDNEXT pops nearest first).
            let [coord, distance, check_vis] = state.pop_ints::<3>();
            let (cx, cz, clevel) = unpack_coord(coord);
            let mut found: Vec<(usize, i32)> = world
                .npcs_within(cx, cz, clevel, distance)
                .into_iter()
                .filter_map(|nid| {
                    let n = world.npcs[nid].as_ref()?;
                    if !n.active || n.entity.level != clevel {
                        return None;
                    }
                    let d = (n.entity.x - cx).abs().max((n.entity.z - cz).abs());
                    if d > distance {
                        return None;
                    }
                    hunt_visible(world, clevel, n.entity.x, n.entity.z, cx, cz, check_vis)
                        .then_some((nid, d))
                })
                .collect();
            found.sort_by_key(|&(_, d)| d);
            state.npc_iterator = found.into_iter().rev().map(|(nid, _)| nid).collect();
        }
        op::NPC_HUNT => {
            // Engine-TS NPC_HUNT: set the closest (by euclidean distance) npc in
            // range active, push 1; else push 0.
            let [coord, distance, check_vis] = state.pop_ints::<3>();
            let (cx, cz, clevel) = unpack_coord(coord);
            let closest = world
                .npcs_within(cx, cz, clevel, distance)
                .into_iter()
                .filter_map(|nid| {
                    let n = world.npcs[nid].as_ref()?;
                    if !n.active || n.entity.level != clevel {
                        return None;
                    }
                    let (dx, dz) = (n.entity.x - cx, n.entity.z - cz);
                    if dx.abs().max(dz.abs()) > distance {
                        return None;
                    }
                    hunt_visible(world, clevel, n.entity.x, n.entity.z, cx, cz, check_vis)
                        .then_some((nid, (dx * dx + dz * dz) as i64))
                })
                .min_by_key(|&(_, e)| e)
                .map(|(nid, _)| nid);
            match closest {
                Some(nid) => {
                    let secondary = state.secondary();
                    if secondary {
                        state.active_npc2 = Some(nid);
                    } else {
                        state.active_npc = Some(nid);
                    }
                    state.pointer_add(crate::script::state::ACTIVE_NPC[secondary as usize]);
                    state.push_int(1);
                }
                None => state.push_int(0),
            }
        }

        _ => {
            return Err("unhandled command".to_string());
        }
    }
    Ok(())
}

// Cheap LCG for the random ops — deterministic-but-good distribution
// without an external crate.
/// The success "value" interpolated between the level-1 bound (`low`) and the
/// level-99 bound (`high`) by the player's current `level` — 1:1 with the
/// Engine-TS STAT_RANDOM formula. The op rolls 0..=255 and succeeds when this
/// value exceeds the roll, so `low`/`high` are the effective success weights at
/// the two ends of the skill range.
fn stat_random_value(level: i32, low: i32, high: i32) -> i32 {
    // Engine-TS floors each term with `Math.floor` (toward -inf), which differs
    // from Rust's truncate-toward-zero `/` once a term goes negative — a boosted
    // level (> 99) makes `99 - level` negative, a level drained to 0 makes
    // `level - 1` negative. `div_euclid` floors for the positive divisor 98.
    (low * (99 - level)).div_euclid(98) + (high * (level - 1)).div_euclid(98) + 1
}

use crate::script::trigger;

/// Triggers in which LAST_ITEM is valid (Engine-TS allowedTriggers).
const OPHELD_OR_INVBUTTON: [u16; 12] = [
    trigger::OPHELD1, trigger::OPHELD2, trigger::OPHELD3, trigger::OPHELD4, trigger::OPHELD5,
    trigger::OPHELDU, trigger::OPHELDT,
    trigger::INV_BUTTON1, trigger::INV_BUTTON2, trigger::INV_BUTTON3, trigger::INV_BUTTON4,
    trigger::INV_BUTTON5,
];
/// LAST_SLOT additionally allows INV_BUTTOND.
const OPHELD_OR_INVBUTTON_D: [u16; 13] = [
    trigger::OPHELD1, trigger::OPHELD2, trigger::OPHELD3, trigger::OPHELD4, trigger::OPHELD5,
    trigger::OPHELDU, trigger::OPHELDT,
    trigger::INV_BUTTON1, trigger::INV_BUTTON2, trigger::INV_BUTTON3, trigger::INV_BUTTON4,
    trigger::INV_BUTTON5, trigger::INV_BUTTOND,
];
/// LAST_USEITEM / LAST_USESLOT triggers (the "use X on Y" approach/op set).
const USE_TRIGGERS: [u16; 9] = [
    trigger::OPHELDU, trigger::APOBJU, trigger::APLOCU, trigger::APNPCU, trigger::APPLAYERU,
    trigger::OPOBJU, trigger::OPLOCU, trigger::OPNPCU, trigger::OPPLAYERU,
];

/// Engine-TS allowedTriggers guard: error when an op is used outside the
/// triggers where its context value is meaningful.
fn require_trigger(state: &ScriptState, allowed: &[u16], op: &str) -> Result<(), String> {
    if allowed.contains(&state.trigger) {
        Ok(())
    } else {
        Err(format!("{op} is not safe to use in this trigger"))
    }
}

/// HuntVis filter — 0 = off, 1 = line-of-sight, 2 = line-of-walk — between two
/// tiles on `level`. Used by the hunt ops.
fn hunt_visible(world: &World, level: i32, fx: i32, fz: i32, tx: i32, tz: i32, check_vis: i32) -> bool {
    match check_vis {
        1 => world.collision.line_of_sight(level, fx, fz, tx, tz),
        2 => world.collision.line_of_walk(level, fx, fz, tx, tz),
        _ => true,
    }
}

/// Drop `count` of `obj` at player `pid`'s tile, owned by them for 200 ticks —
/// the Engine-TS inventory-overflow behaviour (singles for a non-stackable obj
/// or a 1-overflow, else one stack).
fn drop_overflow(world: &mut World, pid: usize, obj: i32, count: i32) {
    let Some((x, z, level)) = world.players[pid].as_ref().map(|p| (p.entity.x, p.entity.z, p.entity.level))
    else {
        return;
    };
    if !world.obj_stackable(obj) || count == 1 {
        for _ in 0..count {
            world.add_ground_obj(obj, 1, x, z, level, pid as i32, 200);
        }
    } else {
        world.add_ground_obj(obj, count, x, z, level, pid as i32, 200);
    }
}

/// Engine-TS config-name fallback: the type's `name` if set, else `"null"`.
/// (Engine-TS chains `name ?? debugname ?? 'null'`; OS configs don't decode a
/// debugname, so an empty name falls straight through to `"null"`.)
fn config_name(name: Option<&String>) -> String {
    match name {
        Some(n) if !n.is_empty() => n.clone(),
        _ => "null".to_string(),
    }
}

/// Unpack a RuneScript coord int into `(x, z, level)` — the layout used by
/// COORDX/Y/Z and every map op.
fn unpack_coord(coord: i32) -> (i32, i32, i32) {
    let x = (coord >> 14) & 0x3fff;
    let z = coord & 0x3fff;
    let level = (coord >> 28) & 0x3;
    (x, z, level)
}

/// Does any active dynamic loc's footprint already cover `(x, z, level)`? Backs
/// MAP_LOCADDUNSAFE. Walls (shape 0..=3) and ground decor (22) occupy only
/// their anchor tile; scenery/ground locs (9/10/11/≥12) occupy a rotation-
/// swapped rectangle; wall decor (4..=8) occupies nothing. Only locs anchored
/// in the coord's own zone are tested (Engine-TS has the same limitation).
fn loc_occupies_tile(world: &World, x: i32, z: i32, level: i32) -> bool {
    let zidx = crate::zone::zone_index(x, z, level);
    for loc in world.zones.locs_in(zidx) {
        if loc.level != level {
            continue;
        }
        let Some(cfg) = world.loc_config.get(&loc.id).copied() else { continue };
        if cfg.active != 1 {
            continue;
        }
        let shape = loc.shape;
        if (0..=3).contains(&shape) || shape == 22 {
            if loc.x == x && loc.z == z {
                return true;
            }
        } else if !(4..=8).contains(&shape) {
            let (lw, ll) = if loc.angle == 1 || loc.angle == 3 {
                (cfg.length, cfg.width)
            } else {
                (cfg.width, cfg.length)
            };
            if x >= loc.x && x < loc.x + lw && z >= loc.z && z < loc.z + ll {
                return true;
            }
        }
    }
    false
}

/// Engine-TS `MASK[width]` = `(2^width − 1)` cast to i32: a run of `width` low
/// bits set. `MASK[0] = 0`, `MASK[32] = -1` (every bit). Used by the bit-range
/// ops (SETBIT_RANGE / CLEARBIT_RANGE / SETBIT_RANGE_TOINT).
fn bit_mask(width: i32) -> i32 {
    if width <= 0 {
        0
    } else if width >= 32 {
        -1
    } else {
        ((1u32 << width) - 1) as i32
    }
}

/// Convert a 24-bit RGB colour (`0xRRGGBB`) to the client's 15-bit 5/5/5 form —
/// 1:1 with Engine-TS `ColorConversion.rgb24to15`. Used by IF_SETCOLOUR.
fn rgb24_to_15(rgb: i32) -> i32 {
    let r = (rgb >> 16) & 0xff;
    let g = (rgb >> 8) & 0xff;
    let b = rgb & 0xff;
    ((r >> 3) << 10) + ((g >> 3) << 5) + (b >> 3)
}

fn rand_unit() -> f64 {
    use std::sync::{Mutex, OnceLock};
    // Entropy-seed once, from wall-clock — Engine-TS likewise seeds JavaRandom
    // from `Math.random()`, so the script RNG (RANDOM/RANDOMINC/STAT_RANDOM) isn't
    // an identical sequence every boot (which would make drops/skill rolls
    // predictable). `| 1` keeps the LCG state nonzero in the clock-reads-zero edge.
    static SEED: OnceLock<Mutex<u64>> = OnceLock::new();
    let seed = SEED.get_or_init(|| {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        Mutex::new((nanos ^ 0x9E37_79B9_7F4A_7C15) | 1)
    });
    let mut g = seed.lock().unwrap();
    *g = g.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    ((*g >> 11) as f64 / (1u64 << 53) as f64).clamp(0.0, 1.0 - f64::EPSILON)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script::file::{ScriptFile, ScriptInfo};
    use std::sync::Arc;

    /// One-shot script: push each of `ints`, then run `op_code`, then RETURN.
    fn op_script(ints: &[i32], op_code: u16) -> ScriptFile {
        let mut opcodes = Vec::new();
        let mut int_operands = Vec::new();
        for &v in ints {
            opcodes.push(op::PUSH_CONSTANT_INT);
            int_operands.push(v);
        }
        opcodes.push(op_code);
        int_operands.push(0);
        opcodes.push(op::RETURN);
        int_operands.push(0);
        let string_operands = vec![None; opcodes.len()];
        ScriptFile {
            id: 0,
            info: ScriptInfo {
                script_name: "test".into(),
                source_file_path: "test".into(),
                lookup_key: -1,
                parameter_types: Vec::new(),
                pcs: Vec::new(),
                lines: Vec::new(),
            },
            int_local_count: 0,
            string_local_count: 0,
            int_arg_count: 0,
            string_arg_count: 0,
            switch_tables: Vec::new(),
            opcodes,
            int_operands,
            string_operands,
        }
    }

    /// Run a one-shot `op_code` script with `pid` as the active player.
    fn run_op(world: &mut World, pid: Option<usize>, ints: &[i32], op_code: u16) -> ScriptState {
        let mut state = ScriptState::new(Arc::new(op_script(ints, op_code)), &[]);
        state.active_player = pid;
        assert_eq!(execute(&mut state, world), Execution::Finished, "script finished");
        state
    }

    /// Run a one-shot `op_code` script with `nid` as the active npc.
    fn run_op_npc(world: &mut World, nid: usize, ints: &[i32], op_code: u16) -> ScriptState {
        let mut state = ScriptState::new(Arc::new(op_script(ints, op_code)), &[]);
        state.active_npc = Some(nid);
        assert_eq!(execute(&mut state, world), Execution::Finished, "script finished");
        state
    }

    /// Pack a coord the way the COORD op / RuneScript does.
    fn coord(level: i32, x: i32, z: i32) -> i32 {
        ((level & 0x3) << 28) | ((x & 0x3fff) << 14) | (z & 0x3fff)
    }

    /// The value a getter op left on the stack.
    fn pushed(world: &mut World, ints: &[i32], op_code: u16) -> i32 {
        let st = run_op(world, None, ints, op_code);
        *st.int_stack.last().expect("op pushed a result")
    }

    #[test]
    fn coord_component_and_geometry_ops() {
        let mut w = World::new();
        let c = coord(1, 3210, 3245);

        assert_eq!(pushed(&mut w, &[c], op::COORDX), 3210);
        assert_eq!(pushed(&mut w, &[c], op::COORDY), 1, "level");
        assert_eq!(pushed(&mut w, &[c], op::COORDZ), 3245);

        // Chebyshev distance: max(|dx|, |dz|) = max(4, 3) = 4.
        let a = coord(0, 3200, 3200);
        let b = coord(0, 3204, 3203);
        assert_eq!(pushed(&mut w, &[a, b], op::DISTANCE), 4);

        // MOVECOORD shifts (x, y=level, z).
        let moved = pushed(&mut w, &[coord(0, 3200, 3200), 5, 1, -3], op::MOVECOORD);
        assert_eq!(moved, coord(1, 3205, 3197));

        // INZONE: inside the box, then outside on z.
        let from = coord(0, 3200, 3200);
        let to = coord(0, 3210, 3210);
        assert_eq!(pushed(&mut w, &[from, to, coord(0, 3205, 3205)], op::INZONE), 1);
        assert_eq!(pushed(&mut w, &[from, to, coord(0, 3205, 3211)], op::INZONE), 0);
    }

    #[test]
    fn finduid_resolves_and_rejects_stale() {
        let mut w = World::new();
        let pid = w.add_player("zezima".into(), 3200, 3200, 0).unwrap();
        let uid = w.players[pid].as_ref().unwrap().uid();

        let st = run_op(&mut w, None, &[uid], op::FINDUID);
        assert_eq!(*st.int_stack.last().unwrap(), 1, "uid found");
        assert_eq!(st.active_player, Some(pid), "active player set to the resolved slot");

        // Corrupt the name-hash bits while keeping the same slot → stale.
        let st = run_op(&mut w, None, &[uid ^ (1 << 11)], op::FINDUID);
        assert_eq!(*st.int_stack.last().unwrap(), 0, "stale uid (slot reused) rejected");
    }

    #[test]
    fn projanim_pl_homes_on_player_uid() {
        use io::packet::Packet;
        let mut w = World::new();
        let observer = w.add_player("obs".into(), 3222, 3222, 0).unwrap();
        let target = w.add_player("tgt".into(), 3224, 3223, 0).unwrap();
        let uid = w.players[target].as_ref().unwrap().uid();
        w.players[observer].as_mut().unwrap().out.clear();

        run_op(&mut w, None,
            &[coord(0, 3223, 3222), uid, 220, 40, 36, 5, 25, 16, 0], op::PROJANIM_PL);

        let out = &w.players[observer].as_ref().unwrap().out;
        let pkt = out.iter().find(|m| m.opcode == 32).expect("MAP_PROJANIM");
        let mut r = Packet::from_vec(pkt.body.clone());
        let (_slot, _dx, _dz) = (r.g1(), r.g1b(), r.g1b());
        assert_eq!(r.g2b() as i32, -(target as i32) - 1, "homes on the player (target = -slot-1)");
    }

    #[test]
    fn projanim_npc_homes_on_npc_uid() {
        use io::packet::Packet;
        let mut w = World::new();
        let observer = w.add_player("obs".into(), 3222, 3222, 0).unwrap();
        let nid = w.add_npc(1, 3224, 3223, 0).unwrap();
        w.players[observer].as_mut().unwrap().out.clear();

        // npc uid = (type << 16) | slot.
        let uid = (1 << 16) | nid as i32;
        run_op(&mut w, None,
            &[coord(0, 3223, 3222), uid, 220, 40, 36, 5, 25, 16, 0], op::PROJANIM_NPC);

        let out = &w.players[observer].as_ref().unwrap().out;
        let pkt = out.iter().find(|m| m.opcode == 32).expect("MAP_PROJANIM");
        let mut r = Packet::from_vec(pkt.body.clone());
        let (_slot, _dx, _dz) = (r.g1(), r.g1b(), r.g1b());
        assert_eq!(r.g2b() as i32, nid as i32 + 1, "homes on the npc (target = nid+1)");
    }

    #[test]
    fn npc_stat_ops_read_and_modify_levels() {
        use crate::entity::npc::{NPC_STAT_HITPOINTS, NPC_STAT_STRENGTH};
        let mut w = World::new();
        let nid = w.add_npc(1, 3224, 3223, 0).unwrap();
        {
            let n = w.npcs[nid].as_mut().unwrap();
            n.base_levels[NPC_STAT_HITPOINTS] = 20;
            n.levels[NPC_STAT_HITPOINTS] = 20;
            n.base_levels[NPC_STAT_STRENGTH] = 10;
            n.levels[NPC_STAT_STRENGTH] = 10;
        }
        // NPC_STAT / NPC_BASESTAT getters.
        let st = run_op_npc(&mut w, nid, &[NPC_STAT_HITPOINTS as i32], op::NPC_STAT);
        assert_eq!(*st.int_stack.last().unwrap(), 20);
        let st = run_op_npc(&mut w, nid, &[NPC_STAT_STRENGTH as i32], op::NPC_BASESTAT);
        assert_eq!(*st.int_stack.last().unwrap(), 10);

        // NPC_STATSUB: -5 + 0% off strength 10 -> 5.
        run_op_npc(&mut w, nid, &[NPC_STAT_STRENGTH as i32, 5, 0], op::NPC_STATSUB);
        assert_eq!(w.npcs[nid].as_ref().unwrap().levels[NPC_STAT_STRENGTH], 5);
        // NPC_STATADD: +3 back -> 8.
        run_op_npc(&mut w, nid, &[NPC_STAT_STRENGTH as i32, 3, 0], op::NPC_STATADD);
        assert_eq!(w.npcs[nid].as_ref().unwrap().levels[NPC_STAT_STRENGTH], 8);
        // NPC_STATHEAL: heals toward base (capped at base 10).
        run_op_npc(&mut w, nid, &[NPC_STAT_STRENGTH as i32, 100, 0], op::NPC_STATHEAL);
        assert_eq!(w.npcs[nid].as_ref().unwrap().levels[NPC_STAT_STRENGTH], 10);
    }

    #[test]
    fn npc_damage_op_reduces_hitpoints_and_clears_heroes_on_heal() {
        use crate::entity::npc::{MASK_DAMAGE, NPC_STAT_HITPOINTS};
        let mut w = World::new();
        let nid = w.add_npc(1, 3224, 3223, 0).unwrap();
        {
            let n = w.npcs[nid].as_mut().unwrap();
            n.base_levels[NPC_STAT_HITPOINTS] = 30;
            n.levels[NPC_STAT_HITPOINTS] = 30;
            n.add_hero(0, 12); // a damage dealer on record
        }
        // Push order is type, amount (deob pops amount first).
        run_op_npc(&mut w, nid, &[1, 7], op::NPC_DAMAGE);
        {
            let n = w.npcs[nid].as_ref().unwrap();
            assert_eq!(n.levels[NPC_STAT_HITPOINTS], 23, "hit subtracted from HP");
            assert_ne!(n.entity.masks & MASK_DAMAGE, 0, "DAMAGE mask flagged");
            assert_eq!(n.find_hero(), Some(0), "hero still on record while hurt");
        }
        // Heal HP back to base clears the hero tally (Engine-TS NPC_STATHEAL).
        run_op_npc(&mut w, nid, &[NPC_STAT_HITPOINTS as i32, 100, 0], op::NPC_STATHEAL);
        assert_eq!(w.npcs[nid].as_ref().unwrap().find_hero(), None,
                   "heroes cleared once HP back to base");
    }

    #[test]
    fn npc_heropoints_credits_active_player_and_findhero_resolves_it() {
        let mut w = World::new();
        let p1 = w.add_player("a".into(), 3222, 3222, 0).unwrap();
        let p2 = w.add_player("b".into(), 3223, 3222, 0).unwrap();
        let nid = w.add_npc(1, 3224, 3223, 0).unwrap();

        // p1 deals 5, p2 deals 9 — p2 should win the kill.
        let mut s = ScriptState::new(Arc::new(op_script(&[5], op::NPC_HEROPOINTS)), &[]);
        s.active_player = Some(p1);
        s.active_npc = Some(nid);
        assert_eq!(execute(&mut s, &mut w), Execution::Finished);
        let mut s = ScriptState::new(Arc::new(op_script(&[9], op::NPC_HEROPOINTS)), &[]);
        s.active_player = Some(p2);
        s.active_npc = Some(nid);
        assert_eq!(execute(&mut s, &mut w), Execution::Finished);

        // NPC_FINDHERO sets the active player to the top dealer (p2) and pushes 1.
        let st = run_op_npc(&mut w, nid, &[], op::NPC_FINDHERO);
        assert_eq!(*st.int_stack.last().unwrap(), 1, "a hero was found");
        assert_eq!(st.active_player, Some(p2), "active player set to the top dealer");
    }

    #[test]
    fn npc_findhero_pushes_zero_when_no_dealers() {
        let mut w = World::new();
        let nid = w.add_npc(1, 3224, 3223, 0).unwrap();
        let st = run_op_npc(&mut w, nid, &[], op::NPC_FINDHERO);
        assert_eq!(*st.int_stack.last().unwrap(), 0, "no hero -> 0");
    }

    #[test]
    fn playercount_op_counts_active_players() {
        let mut w = World::new();
        assert_eq!(pushed(&mut w, &[], op::PLAYERCOUNT), 0);
        w.add_player("a".into(), 3200, 3200, 0).unwrap();
        w.add_player("b".into(), 3201, 3200, 0).unwrap();
        assert_eq!(pushed(&mut w, &[], op::PLAYERCOUNT), 2);
    }

    #[test]
    fn last_com_op_returns_clicked_component() {
        let mut w = World::new();
        let pid = w.add_player("p".into(), 3222, 3222, 0).unwrap();
        let com = (548 << 16) | 6;
        // A button click records last_com (the IF_BUTTON trigger is a no-op
        // here — no scripts loaded).
        w.handle_message(pid, protocol::client::ClientMessage::IfButton {
            op: 1, component: com, sub: -1,
        });
        let st = run_op(&mut w, Some(pid), &[], op::LAST_COM);
        assert_eq!(*st.int_stack.last().unwrap(), com, "LAST_COM returns the click");
        // LAST_INT defaults to 0 for a fresh invocation.
        let st = run_op(&mut w, Some(pid), &[], op::LAST_INT);
        assert_eq!(*st.int_stack.last().unwrap(), 0);
    }

    #[test]
    fn if_settext_op_writes_text_packet() {
        let mut world = World::new();
        let pid = world.add_player("p".into(), 3222, 3222, 0).unwrap();
        world.players[pid].as_mut().unwrap().out.clear();
        // Push the component, then the text (op pops text first).
        let com = (23 << 16) | 5;
        let mut s = ScriptState::new(Arc::new(op_script(&[com], op::IF_SETTEXT)), &[]);
        s.string_stack.push("Welcome to RuneScape.".into());
        s.active_player = Some(pid);
        assert_eq!(execute(&mut s, &mut world), Execution::Finished);
        let p = world.players[pid].as_ref().unwrap();
        let pkt = p.out.iter().find(|m| m.opcode == 197).expect("IF_SETTEXT (197)");
        // jstr (len 21 + nul terminator) then the 4-byte component.
        assert_eq!(pkt.body.len(), 22 + 4, "jstr + p4 component");
    }

    #[test]
    fn rgb24_to_15_matches_engine_ts() {
        // White, red, green, blue, black.
        assert_eq!(rgb24_to_15(0xFFFFFF), 0x7FFF);
        assert_eq!(rgb24_to_15(0xFF0000), 0x7C00);
        assert_eq!(rgb24_to_15(0x00FF00), 0x03E0);
        assert_eq!(rgb24_to_15(0x0000FF), 0x001F);
        assert_eq!(rgb24_to_15(0x000000), 0);
    }

    #[test]
    fn bit_range_ops_match_engine_ts() {
        let mut w = World::new();
        // SETBIT_RANGE(num, start, end): OR a [start..=end] run into num.
        // 0 | (MASK[3] << 4) = 0x70.
        assert_eq!(pushed(&mut w, &[0, 4, 6], op::SETBIT_RANGE), 0x70);
        // CLEARBIT_RANGE: 0xFF & ~(MASK[4] << 0) = 0xF0.
        assert_eq!(pushed(&mut w, &[0xFF, 0, 3], op::CLEARBIT_RANGE), 0xF0);
        // GETBIT_RANGE: pull bits [4..=7] of 0xAB (1010_1011) → 0xA.
        assert_eq!(pushed(&mut w, &[0xAB, 4, 7], op::GETBIT_RANGE), 0xA);
        // GETBIT_RANGE is unsigned (`>>>`): top bit of a full word reads back
        // without sign extension. bit [31..=31] of i32::MIN → 1.
        assert_eq!(pushed(&mut w, &[i32::MIN, 31, 31], op::GETBIT_RANGE), 1);
        // SETBIT_RANGE_TOINT(num, value, start, end): write 5 into bits [4..=7]
        // of 0xF0F → clear nibble then set: 0xF0F & ~0xF0 | (5<<4) = 0xF5F.
        assert_eq!(pushed(&mut w, &[0xF0F, 5, 4, 7], op::SETBIT_RANGE_TOINT), 0xF5F);
        // Saturates: value 0xFF into a 4-bit field clamps to 0xF.
        assert_eq!(pushed(&mut w, &[0, 0xFF, 0, 3], op::SETBIT_RANGE_TOINT), 0xF);
    }

    #[test]
    fn map_blocked_and_indoors_ops() {
        let mut w = World::new();
        // Unloaded collision is permissive.
        assert_eq!(pushed(&mut w, &[coord(0, 3200, 3200)], op::MAP_BLOCKED), 0);
        w.collision.block_ground(3200, 3200, 0);
        assert_eq!(pushed(&mut w, &[coord(0, 3200, 3200)], op::MAP_BLOCKED), 1);
        // A neighbouring open tile in the now-loaded region stays open.
        assert_eq!(pushed(&mut w, &[coord(0, 3201, 3200)], op::MAP_BLOCKED), 0);
        assert_eq!(pushed(&mut w, &[coord(0, 3201, 3200)], op::MAP_INDOORS), 0);
        w.collision.set_roof(3201, 3200, 0, true);
        assert_eq!(pushed(&mut w, &[coord(0, 3201, 3200)], op::MAP_INDOORS), 1);
    }

    #[test]
    fn lineofsight_and_lineofwalk_ops() {
        let mut w = World::new();
        w.collision.set_roof(3200, 3200, 0, false); // mark the region loaded
        assert_eq!(
            pushed(&mut w, &[coord(0, 3200, 3200), coord(0, 3205, 3200)], op::LINEOFWALK),
            1
        );
        // A walk-only loc (blockrange = false) blocks walk but not sight.
        w.collision.apply_loc(3203, 3200, 0, 10, 0, 1, 1, 2, false, true);
        assert_eq!(
            pushed(&mut w, &[coord(0, 3200, 3200), coord(0, 3206, 3200)], op::LINEOFWALK),
            0
        );
        assert_eq!(
            pushed(&mut w, &[coord(0, 3200, 3200), coord(0, 3206, 3200)], op::LINEOFSIGHT),
            1
        );
        // Different levels never have line of sight/walk.
        assert_eq!(
            pushed(&mut w, &[coord(0, 3200, 3200), coord(1, 3201, 3200)], op::LINEOFSIGHT),
            0
        );
    }

    #[test]
    fn seqlength_op_reads_config() {
        let mut w = World::new();
        assert_eq!(pushed(&mut w, &[7], op::SEQLENGTH), 0); // unknown seq → 0
        w.seq_lengths.insert(7, 42);
        assert_eq!(pushed(&mut w, &[7], op::SEQLENGTH), 42);
    }

    #[test]
    fn map_multiway_op() {
        let mut w = World::new();
        assert_eq!(pushed(&mut w, &[coord(0, 3200, 3200)], op::MAP_MULTIWAY), 0);
        w.collision.multiway.insert(crate::collision::zone_index(3200, 3200, 0));
        assert_eq!(pushed(&mut w, &[coord(0, 3200, 3200)], op::MAP_MULTIWAY), 1);
    }

    #[test]
    fn map_findsquare_returns_tile_in_range() {
        let mut w = World::new();
        let out = pushed(&mut w, &[coord(0, 3200, 3200), 1, 3, 0], op::MAP_FINDSQUARE);
        let (x, z, level) = ((out >> 14) & 0x3fff, out & 0x3fff, (out >> 28) & 0x3);
        assert_eq!(level, 0);
        assert!((x - 3200).abs() <= 3 && (z - 3200).abs() <= 3, "within max radius: {x},{z}");
    }

    #[test]
    fn map_locaddunsafe_op() {
        let mut w = World::new();
        w.loc_config.insert(
            99,
            crate::world::LocCollision { width: 2, length: 2, blockwalk: 1, blockrange: true, active: 1 },
        );
        // Empty tile is safe.
        assert_eq!(pushed(&mut w, &[coord(0, 3210, 3210)], op::MAP_LOCADDUNSAFE), 0);
        // Spawn a 2×2 ground loc (shape 10); its footprint now occupies tiles.
        w.add_loc(99, 10, 0, 3210, 3210, 0);
        assert_eq!(pushed(&mut w, &[coord(0, 3210, 3210)], op::MAP_LOCADDUNSAFE), 1);
        assert_eq!(pushed(&mut w, &[coord(0, 3211, 3211)], op::MAP_LOCADDUNSAFE), 1);
        assert_eq!(pushed(&mut w, &[coord(0, 3212, 3212)], op::MAP_LOCADDUNSAFE), 0);
    }

    #[test]
    fn loc_add_then_del_updates_collision() {
        let mut w = World::new();
        w.loc_config.insert(
            50,
            crate::world::LocCollision { width: 1, length: 1, blockwalk: 1, blockrange: true, active: 1 },
        );
        w.collision.set_roof(3220, 3220, 0, false); // load the region
        assert!(!w.collision.is_blocked(3220, 3220, 0));
        // A scenery loc (shape 10) blocks its tile when spawned…
        w.add_loc(50, 10, 0, 3220, 3220, 0);
        assert!(w.collision.is_blocked(3220, 3220, 0));
        // …and unblocks it when removed.
        w.del_loc(3220, 3220, 0, 10);
        assert!(!w.collision.is_blocked(3220, 3220, 0));
    }

    #[test]
    fn config_query_ops_read_loaded_config() {
        use crate::world::{LocInfo, NpcInfo, ObjInfo};
        let mut w = World::new();
        w.loc_info.insert(1530, LocInfo { name: "Door".into(), width: 1, length: 2, ..Default::default() });
        w.obj_info.insert(
            995,
            ObjInfo { name: "Coins".into(), cost: 1, stackable: 1, members: false, certlink: -1, certtemplate: -1, ..Default::default() },
        );
        // Shark (385) is the un-noted item; its note (386) has certtemplate>=0.
        w.obj_info.insert(
            385,
            ObjInfo { name: "Shark".into(), cost: 100, stackable: 0, members: true, certlink: 386, certtemplate: -1, ..Default::default() },
        );
        w.obj_info.insert(
            386,
            ObjInfo { name: "Shark".into(), cost: 100, stackable: 0, members: true, certlink: 385, certtemplate: 799, ..Default::default() },
        );
        w.npc_info.insert(
            1,
            NpcInfo { name: "Hans".into(), size: 1, vislevel: -1, op: [Some("Talk-to".into()), None, None, None, None], ..Default::default() },
        );

        // Integer readers.
        assert_eq!(pushed(&mut w, &[1530], op::LC_WIDTH), 1);
        assert_eq!(pushed(&mut w, &[1530], op::LC_LENGTH), 2);
        assert_eq!(pushed(&mut w, &[995], op::OC_COST), 1);
        assert_eq!(pushed(&mut w, &[385], op::OC_MEMBERS), 1);
        assert_eq!(pushed(&mut w, &[995], op::OC_STACKABLE), 1);
        assert_eq!(pushed(&mut w, &[385], op::OC_STACKABLE), 0);
        assert_eq!(pushed(&mut w, &[1], op::NC_SIZE), 1);
        assert_eq!(pushed(&mut w, &[1], op::NC_VISLEVEL), -1);
        // OC_CERT on the un-noted item → its note id; OC_UNCERT on the note → base.
        assert_eq!(pushed(&mut w, &[385], op::OC_CERT), 386);
        assert_eq!(pushed(&mut w, &[386], op::OC_UNCERT), 385);
        // Unknown id falls back to the id itself.
        assert_eq!(pushed(&mut w, &[999], op::OC_CERT), 999);

        // String readers (read the string stack the op pushed).
        let st = run_op(&mut w, None, &[1530], op::LC_NAME);
        assert_eq!(st.string_stack.last().unwrap(), "Door");
        let st = run_op(&mut w, None, &[995], op::OC_NAME);
        assert_eq!(st.string_stack.last().unwrap(), "Coins");
        let st = run_op(&mut w, None, &[1], op::NC_NAME);
        assert_eq!(st.string_stack.last().unwrap(), "Hans");
        let st = run_op(&mut w, None, &[1, 1], op::NC_OP);
        assert_eq!(st.string_stack.last().unwrap(), "Talk-to");
        let st = run_op(&mut w, None, &[1, 2], op::NC_OP);
        assert_eq!(st.string_stack.last().unwrap(), "");
        // Unknown config → "null".
        let st = run_op(&mut w, None, &[42], op::OC_NAME);
        assert_eq!(st.string_stack.last().unwrap(), "null");
    }

    #[test]
    fn entity_name_ops_read_active_type() {
        use crate::world::{LocInfo, NpcInfo, ObjInfo};
        let mut w = World::new();
        w.loc_info.insert(1530, LocInfo { name: "Door".into(), width: 1, length: 1, ..Default::default() });
        w.obj_info.insert(995, ObjInfo { name: "Coins".into(), ..Default::default() });
        w.npc_info.insert(1, NpcInfo { name: "Hans".into(), size: 1, vislevel: -1, op: Default::default(), ..Default::default() });

        // NPC_NAME reads the active npc's type.
        let nid = w.add_npc(1, 3200, 3200, 0).unwrap();
        let st = run_op_npc(&mut w, nid, &[], op::NPC_NAME);
        assert_eq!(st.string_stack.last().unwrap(), "Hans");

        // LOC_NAME / OBJ_NAME read the active loc/obj — set them directly.
        let mut s = ScriptState::new(Arc::new(op_script(&[], op::LOC_NAME)), &[]);
        s.active_loc = Some((3200, 3200, 0, 1530, 0, 0));
        assert_eq!(execute(&mut s, &mut w), Execution::Finished);
        assert_eq!(s.string_stack.last().unwrap(), "Door");

        let mut s = ScriptState::new(Arc::new(op_script(&[], op::OBJ_NAME)), &[]);
        s.active_obj = Some((3200, 3200, 0, 995));
        assert_eq!(execute(&mut s, &mut w), Execution::Finished);
        assert_eq!(s.string_stack.last().unwrap(), "Coins");
    }

    #[test]
    fn last_item_ops_honour_the_trigger_guard() {
        let mut w = World::new();
        let pid = w.add_player("p".into(), 3200, 3200, 0).unwrap();
        {
            let p = w.players[pid].as_mut().unwrap();
            p.last_item = 995;
            p.last_slot = 3;
            p.last_use_item = 386;
            p.last_use_slot = 7;
            p.last_target_slot = 9;
        }
        let run_t = |w: &mut World, op_code: u16, trig: u16| {
            let mut s = ScriptState::new(Arc::new(op_script(&[], op_code)), &[]);
            s.active_player = Some(pid);
            s.trigger = trig;
            let exec = execute(&mut s, w);
            (exec, s.int_stack.last().copied())
        };
        assert_eq!(run_t(&mut w, op::LAST_ITEM, trigger::OPHELD1), (Execution::Finished, Some(995)));
        assert_eq!(run_t(&mut w, op::LAST_SLOT, trigger::INV_BUTTOND), (Execution::Finished, Some(3)));
        assert_eq!(run_t(&mut w, op::LAST_USEITEM, trigger::APNPCU), (Execution::Finished, Some(386)));
        assert_eq!(run_t(&mut w, op::LAST_USESLOT, trigger::OPHELDU), (Execution::Finished, Some(7)));
        assert_eq!(run_t(&mut w, op::LAST_TARGETSLOT, trigger::INV_BUTTOND), (Execution::Finished, Some(9)));
        // Used outside an allowed trigger → the script aborts.
        assert_eq!(run_t(&mut w, op::LAST_ITEM, trigger::PROC).0, Execution::Aborted);
    }

    #[test]
    fn player_flag_ops_set_fields() {
        let mut w = World::new();
        let pid = w.add_player("p".into(), 3200, 3200, 0).unwrap();
        run_op(&mut w, Some(pid), &[1], op::ALLOWDESIGN);
        assert!(w.players[pid].as_ref().unwrap().allow_design);
        run_op(&mut w, Some(pid), &[0], op::ALLOWDESIGN);
        assert!(!w.players[pid].as_ref().unwrap().allow_design);

        run_op(&mut w, Some(pid), &[42], op::P_TRANSMOGRIFY);
        assert_eq!(w.players[pid].as_ref().unwrap().transmog, 42);

        // P_APRANGE requires protected access.
        let mut s = ScriptState::new(Arc::new(op_script(&[5], op::P_APRANGE)), &[]);
        s.active_player = Some(pid);
        s.pointer_add(Pointer::ProtectedActivePlayer);
        assert_eq!(execute(&mut s, &mut w), Execution::Finished);
        let p = w.players[pid].as_ref().unwrap();
        assert_eq!(p.ap_range, 5);
        assert!(p.ap_range_called);

        // SESSION_LOG / WEALTH_EVENT just log and finish.
        let mut s = ScriptState::new(Arc::new(op_script(&[1], op::SESSION_LOG)), &[]);
        s.active_player = Some(pid);
        s.string_stack.push("login".into());
        assert_eq!(execute(&mut s, &mut w), Execution::Finished);
        let mut s = ScriptState::new(Arc::new(op_script(&[2, 5, 100], op::WEALTH_EVENT)), &[]);
        s.active_player = Some(pid);
        s.string_stack.push("coins".into());
        assert_eq!(execute(&mut s, &mut w), Execution::Finished);
    }

    #[test]
    fn huntall_gathers_players_and_huntnext_iterates() {
        let mut w = World::new();
        let near = w.add_player("near".into(), 3201, 3200, 0).unwrap();
        let _far = w.add_player("far".into(), 3204, 3200, 0).unwrap();
        let _upstairs = w.add_player("up".into(), 3201, 3200, 1).unwrap();
        // HUNTALL from (3200,3200,0), distance 5, no vis check.
        let mut s = ScriptState::new(
            Arc::new(op_script(&[coord(0, 3200, 3200), 5, 0], op::HUNTALL)),
            &[],
        );
        assert_eq!(execute(&mut s, &mut w), Execution::Finished);
        // Level-1 player excluded; nearest is on top of the pop stack.
        assert_eq!(s.player_iterator.len(), 2);
        assert_eq!(s.player_iterator.last().copied(), Some(near));

        // HUNTNEXT pops nearest first and sets the active player.
        let mut s2 = ScriptState::new(Arc::new(op_script(&[], op::HUNTNEXT)), &[]);
        s2.player_iterator = s.player_iterator.clone();
        assert_eq!(execute(&mut s2, &mut w), Execution::Finished);
        assert_eq!(s2.int_stack.last().copied(), Some(1));
        assert_eq!(s2.active_player, Some(near));

        // Exhausted iterator → 0.
        let mut s3 = ScriptState::new(Arc::new(op_script(&[], op::HUNTNEXT)), &[]);
        assert_eq!(execute(&mut s3, &mut w), Execution::Finished);
        assert_eq!(s3.int_stack.last().copied(), Some(0));
    }

    #[test]
    fn inventory_ops_add_remove_and_query() {
        use crate::world::ObjInfo;
        let mut w = World::new();
        w.inv_sizes.insert(93, 28);
        w.inv_sizes.insert(94, 1);
        w.obj_info.insert(995, ObjInfo { name: "Coins".into(), stackable: 1, ..Default::default() });
        w.obj_info.insert(1277, ObjInfo { name: "Sword".into(), stackable: 0, ..Default::default() });
        let pid = w.add_player("p".into(), 3200, 3200, 0).unwrap();
        let run_inv = |w: &mut World, ints: &[i32], op_code: u16| -> ScriptState {
            let mut s = ScriptState::new(Arc::new(op_script(ints, op_code)), &[]);
            s.active_player = Some(pid);
            s.pointer_add(Pointer::ActivePlayer);
            assert_eq!(execute(&mut s, w), Execution::Finished, "inv op finished");
            s
        };
        let last = |s: ScriptState| s.int_stack.last().copied();

        assert_eq!(last(run_inv(&mut w, &[93], op::INV_SIZE)), Some(28));
        // Stackable coins merge into one slot.
        run_inv(&mut w, &[93, 995, 100], op::INV_ADD);
        run_inv(&mut w, &[93, 995, 50], op::INV_ADD);
        assert_eq!(last(run_inv(&mut w, &[93, 995], op::INV_TOTAL)), Some(150));
        assert_eq!(last(run_inv(&mut w, &[93], op::INV_FREESPACE)), Some(27));
        assert_eq!(last(run_inv(&mut w, &[93, 0], op::INV_GETOBJ)), Some(995));
        assert_eq!(last(run_inv(&mut w, &[93, 0], op::INV_GETNUM)), Some(150));
        // Non-stackable swords take a slot each.
        run_inv(&mut w, &[93, 1277, 3], op::INV_ADD);
        assert_eq!(last(run_inv(&mut w, &[93, 1277], op::INV_TOTAL)), Some(3));
        assert_eq!(last(run_inv(&mut w, &[93], op::INV_FREESPACE)), Some(24));
        run_inv(&mut w, &[93, 1277, 2], op::INV_DEL);
        assert_eq!(last(run_inv(&mut w, &[93, 1277], op::INV_TOTAL)), Some(1));
        // Direct slot writes.
        run_inv(&mut w, &[93, 5, 995, 99], op::INV_SETSLOT);
        assert_eq!(last(run_inv(&mut w, &[93, 5], op::INV_GETNUM)), Some(99));
        run_inv(&mut w, &[93, 5], op::INV_DELSLOT);
        assert_eq!(last(run_inv(&mut w, &[93, 5], op::INV_GETOBJ)), Some(-1));
        // Item-space: an existing coin stack accepts more.
        assert_eq!(last(run_inv(&mut w, &[93, 995, 100, 28], op::INV_ITEMSPACE)), Some(1));
        run_inv(&mut w, &[93], op::INV_CLEAR);
        assert_eq!(last(run_inv(&mut w, &[93, 995], op::INV_TOTAL)), Some(0));

        // Overflow drops to the ground: a size-1 inv, add 2 → 1 in, 1 dropped.
        run_inv(&mut w, &[94, 1277, 2], op::INV_ADD);
        assert_eq!(last(run_inv(&mut w, &[94], op::INV_FREESPACE)), Some(0));
        assert!(w.find_obj(3200, 3200, 0, 1277, pid).is_some(), "overflow dropped to ground");
    }

    #[test]
    fn inventory_move_drop_and_take() {
        use crate::world::ObjInfo;
        let mut w = World::new();
        w.inv_sizes.insert(93, 28);
        w.inv_sizes.insert(95, 10);
        w.obj_info.insert(995, ObjInfo { name: "Coins".into(), stackable: 1, ..Default::default() });
        w.obj_info.insert(1277, ObjInfo { name: "Sword".into(), stackable: 0, ..Default::default() });
        w.obj_info.insert(385, ObjInfo { name: "Shark".into(), stackable: 0, certlink: 386, certtemplate: -1, ..Default::default() });
        w.obj_info.insert(386, ObjInfo { name: "Shark".into(), stackable: 1, certlink: 385, certtemplate: 799, ..Default::default() });
        let pid = w.add_player("p".into(), 3200, 3200, 0).unwrap();
        let run_inv = |w: &mut World, ints: &[i32], op_code: u16| -> ScriptState {
            let mut s = ScriptState::new(Arc::new(op_script(ints, op_code)), &[]);
            s.active_player = Some(pid);
            s.pointer_add(Pointer::ActivePlayer);
            assert_eq!(execute(&mut s, w), Execution::Finished, "inv op finished");
            s
        };
        let last = |s: ScriptState| s.int_stack.last().copied();

        // MOVETOSLOT swaps slots.
        run_inv(&mut w, &[93, 1277, 1], op::INV_ADD);
        run_inv(&mut w, &[93, 93, 0, 3], op::INV_MOVETOSLOT);
        assert_eq!(last(run_inv(&mut w, &[93, 0], op::INV_GETOBJ)), Some(-1));
        assert_eq!(last(run_inv(&mut w, &[93, 3], op::INV_GETOBJ)), Some(1277));

        // MOVEFROMSLOT moves a whole slot to another inv.
        run_inv(&mut w, &[93, 95, 3], op::INV_MOVEFROMSLOT);
        assert_eq!(last(run_inv(&mut w, &[93, 1277], op::INV_TOTAL)), Some(0));
        assert_eq!(last(run_inv(&mut w, &[95, 1277], op::INV_TOTAL)), Some(1));

        // MOVEITEM moves stackables.
        run_inv(&mut w, &[93, 995, 200], op::INV_ADD);
        run_inv(&mut w, &[93, 95, 995, 200], op::INV_MOVEITEM);
        assert_eq!(last(run_inv(&mut w, &[95, 995], op::INV_TOTAL)), Some(200));

        // CERT / UNCERT remap on move.
        run_inv(&mut w, &[93, 385, 1], op::INV_ADD);
        run_inv(&mut w, &[93, 95, 385, 1], op::INV_MOVEITEM_CERT);
        assert_eq!(last(run_inv(&mut w, &[95, 386], op::INV_TOTAL)), Some(1));
        run_inv(&mut w, &[95, 93, 386, 1], op::INV_MOVEITEM_UNCERT);
        assert_eq!(last(run_inv(&mut w, &[93, 385], op::INV_TOTAL)), Some(1));

        // CHANGESLOT replaces the first matching slot.
        run_inv(&mut w, &[93, 385, 995, 5], op::INV_CHANGESLOT);
        assert_eq!(last(run_inv(&mut w, &[93, 385], op::INV_TOTAL)), Some(0));
        assert_eq!(w.inv_total(pid, 93, 995), 5);

        // DROPITEM puts it on the ground; OBJ_TAKEITEM picks it back up.
        run_inv(&mut w, &[93, coord(0, 3200, 3200), 995, 5, 100], op::INV_DROPITEM);
        assert!(w.find_obj(3200, 3200, 0, 995, pid).is_some());
        let before = w.inv_total(pid, 93, 995);
        let mut s = ScriptState::new(Arc::new(op_script(&[93], op::OBJ_TAKEITEM)), &[]);
        s.active_player = Some(pid);
        s.active_obj = Some((3200, 3200, 0, 995));
        assert_eq!(execute(&mut s, &mut w), Execution::Finished);
        assert!(w.inv_total(pid, 93, 995) > before);
        assert!(w.find_obj(3200, 3200, 0, 995, pid).is_none());

        // DROPALL empties the inv onto the ground.
        run_inv(&mut w, &[93, 1277, 2], op::INV_ADD);
        run_inv(&mut w, &[93, coord(0, 3200, 3200), 100], op::INV_DROPALL);
        assert_eq!(last(run_inv(&mut w, &[93], op::INV_FREESPACE)), Some(28));
    }

    #[test]
    fn both_moveinv_and_dropslot_between_players() {
        use crate::world::ObjInfo;
        let mut w = World::new();
        w.inv_sizes.insert(93, 28);
        w.obj_info.insert(995, ObjInfo { name: "Coins".into(), stackable: 1, ..Default::default() });
        let p1 = w.add_player("a".into(), 3200, 3200, 0).unwrap();
        let p2 = w.add_player("b".into(), 3205, 3200, 0).unwrap();
        w.inv_add(p1, 93, 995, 500);

        // BOTH_MOVEINV (operand 0 → primary p1 → secondary p2).
        let mut s = ScriptState::new(Arc::new(op_script(&[93, 93], op::BOTH_MOVEINV)), &[]);
        s.active_player = Some(p1);
        s.active_player2 = Some(p2);
        s.pointer_add(Pointer::ActivePlayer);
        assert_eq!(execute(&mut s, &mut w), Execution::Finished);
        assert_eq!(w.inv_total(p1, 93, 995), 0);
        assert_eq!(w.inv_total(p2, 93, 995), 500);

        // BOTH_DROPSLOT drops p1's slot 0, owned by p2.
        w.inv_add(p1, 93, 995, 10);
        let mut s = ScriptState::new(
            Arc::new(op_script(&[93, coord(0, 3200, 3200), 0, 100], op::BOTH_DROPSLOT)),
            &[],
        );
        s.active_player = Some(p1);
        s.active_player2 = Some(p2);
        s.pointer_add(Pointer::ActivePlayer);
        assert_eq!(execute(&mut s, &mut w), Execution::Finished);
        assert_eq!(w.inv_total(p1, 93, 995), 0);
        assert!(w.find_obj(3200, 3200, 0, 995, p2).is_some());
    }

    #[test]
    fn inv_dropitem_delayed_spawns_after_delay() {
        use crate::world::ObjInfo;
        let mut w = World::new();
        w.inv_sizes.insert(93, 28);
        w.obj_info.insert(995, ObjInfo { name: "Coins".into(), stackable: 1, ..Default::default() });
        let pid = w.add_player("p".into(), 3200, 3200, 0).unwrap();
        w.inv_add(pid, 93, 995, 50);
        let mut s = ScriptState::new(
            Arc::new(op_script(&[93, coord(0, 3200, 3200), 995, 50, 100, 2], op::INV_DROPITEM_DELAYED)),
            &[],
        );
        s.active_player = Some(pid);
        s.pointer_add(Pointer::ActivePlayer);
        assert_eq!(execute(&mut s, &mut w), Execution::Finished);
        // Removed from the inv immediately, but not on the ground yet.
        assert_eq!(w.inv_total(pid, 93, 995), 0);
        assert!(w.find_obj(3200, 3200, 0, 995, pid).is_none(), "not spawned yet");
        w.cycle(); // delay 2 → 1
        assert!(w.find_obj(3200, 3200, 0, 995, pid).is_none(), "still delayed");
        w.cycle(); // delay 1 → 0 → spawns
        assert!(w.find_obj(3200, 3200, 0, 995, pid).is_some(), "spawned after the delay");
    }

    #[test]
    fn inv_transmit_sends_and_stops_update_inv() {
        use crate::world::ObjInfo;
        let mut w = World::new();
        w.inv_sizes.insert(93, 28);
        w.obj_info.insert(995, ObjInfo { name: "Coins".into(), stackable: 1, ..Default::default() });
        let pid = w.add_player("p".into(), 3200, 3200, 0).unwrap();
        w.inv_add(pid, 93, 995, 100);
        let transmit = |w: &mut World| {
            let mut s = ScriptState::new(Arc::new(op_script(&[93, 12345], op::INV_TRANSMIT)), &[]);
            s.active_player = Some(pid);
            s.pointer_add(Pointer::ActivePlayer);
            assert_eq!(execute(&mut s, w), Execution::Finished);
        };
        transmit(&mut w);
        let sent29 = |w: &World| w.players[pid].as_ref().unwrap().out.iter().any(|m| m.opcode == 29);
        // First flush sends UPDATE_INV_FULL.
        w.players[pid].as_mut().unwrap().out.clear();
        w.cycle();
        assert!(sent29(&w), "UPDATE_INV_FULL sent on first transmit");
        // Unchanged next tick → not resent.
        w.players[pid].as_mut().unwrap().out.clear();
        w.cycle();
        assert!(!sent29(&w), "not resent when the inv is unchanged");
        // A mutation re-sends.
        w.inv_add(pid, 93, 995, 5);
        w.players[pid].as_mut().unwrap().out.clear();
        w.cycle();
        assert!(sent29(&w), "resent after the inv changed");
        // STOPTRANSMIT clears the component and stops further sends.
        let mut s = ScriptState::new(Arc::new(op_script(&[12345], op::INV_STOPTRANSMIT)), &[]);
        s.active_player = Some(pid);
        s.pointer_add(Pointer::ActivePlayer);
        assert_eq!(execute(&mut s, &mut w), Execution::Finished);
        assert!(
            w.players[pid].as_ref().unwrap().out.iter().any(|m| m.opcode == 117),
            "STOPTRANSMIT (117) sent"
        );
        w.inv_add(pid, 93, 995, 5);
        w.players[pid].as_mut().unwrap().out.clear();
        w.cycle();
        assert!(!sent29(&w), "no more sends after stoptransmit");
    }

    #[test]
    fn last_login_info_sends_packet() {
        let mut w = World::new();
        let pid = w.add_player("p".into(), 3200, 3200, 0).unwrap();
        w.players[pid].as_mut().unwrap().out.clear();
        run_op(&mut w, Some(pid), &[], op::LAST_LOGIN_INFO);
        assert!(
            w.players[pid].as_ref().unwrap().out.iter().any(|m| m.opcode == 241),
            "LAST_LOGIN_INFO (241) sent"
        );
    }

    #[test]
    fn npc_hunt_finds_closest_and_huntall_iterates() {
        let mut w = World::new();
        let near = w.add_npc(1, 3201, 3200, 0).unwrap();
        let _far = w.add_npc(1, 3204, 3200, 0).unwrap();
        let _upstairs = w.add_npc(1, 3201, 3200, 1).unwrap();
        // NPC_HUNT picks the closest npc in range and sets it active.
        let mut s = ScriptState::new(
            Arc::new(op_script(&[coord(0, 3200, 3200), 5, 0], op::NPC_HUNT)),
            &[],
        );
        assert_eq!(execute(&mut s, &mut w), Execution::Finished);
        assert_eq!(s.int_stack.last().copied(), Some(1));
        assert_eq!(s.active_npc, Some(near));
        // NPC_HUNTALL fills the npc iterator (level-0 only, nearest on top).
        let mut s = ScriptState::new(
            Arc::new(op_script(&[coord(0, 3200, 3200), 5, 0], op::NPC_HUNTALL)),
            &[],
        );
        assert_eq!(execute(&mut s, &mut w), Execution::Finished);
        assert_eq!(s.npc_iterator.len(), 2);
        assert_eq!(s.npc_iterator.last().copied(), Some(near));
    }

    #[test]
    fn p_opnpc_sets_interaction_only_when_op_present() {
        use crate::entity::player::InteractTarget;
        use crate::world::NpcInfo;
        let mut w = World::new();
        w.npc_info.insert(
            1,
            NpcInfo { name: "x".into(), size: 1, vislevel: -1, op: [Some("Talk-to".into()), None, None, None, None], ..Default::default() },
        );
        let pid = w.add_player("p".into(), 3200, 3200, 0).unwrap();
        let nid = w.add_npc(1, 3205, 3200, 0).unwrap();
        // P_OPNPC op 1 (present) → interaction set with the AP-trigger base.
        let mut s = ScriptState::new(Arc::new(op_script(&[1], op::P_OPNPC)), &[]);
        s.active_player = Some(pid);
        s.active_npc = Some(nid);
        s.pointer_add(Pointer::ProtectedActivePlayer);
        assert_eq!(execute(&mut s, &mut w), Execution::Finished);
        let it = w.players[pid].as_ref().unwrap().interaction.expect("interaction set");
        assert_eq!(it.target, InteractTarget::Npc(nid));
        assert_eq!(it.op, trigger::APNPC1 as i32);
        // P_OPNPC op 2 (absent on this npc) → no interaction.
        w.players[pid].as_mut().unwrap().interaction = None;
        let mut s = ScriptState::new(Arc::new(op_script(&[2], op::P_OPNPC)), &[]);
        s.active_player = Some(pid);
        s.active_npc = Some(nid);
        s.pointer_add(Pointer::ProtectedActivePlayer);
        assert_eq!(execute(&mut s, &mut w), Execution::Finished);
        assert!(w.players[pid].as_ref().unwrap().interaction.is_none(), "no interaction when the op is absent");
    }

    #[test]
    fn enum_ops_look_up_values() {
        use crate::world::EnumData;
        let mut w = World::new();
        w.enums.insert(
            7,
            EnumData {
                keys: vec![1, 2],
                int_values: vec![100, 200],
                string_values: vec![],
                default_int: -1,
                default_string: "null".into(),
                is_string: false,
            },
        );
        w.enums.insert(
            8,
            EnumData {
                keys: vec![1],
                int_values: vec![],
                string_values: vec!["hi".into()],
                default_int: 0,
                default_string: "none".into(),
                is_string: true,
            },
        );
        // ENUM pops [inputType, outputType, enumId, key].
        assert_eq!(pushed(&mut w, &[0, 0, 7, 2], op::ENUM), 200);
        assert_eq!(pushed(&mut w, &[0, 0, 7, 9], op::ENUM), -1); // missing key → default
        assert_eq!(pushed(&mut w, &[7], op::ENUM_GETOUTPUTCOUNT), 2);
        let st = run_op(&mut w, None, &[0, 0, 8, 1], op::ENUM);
        assert_eq!(st.string_stack.last().unwrap(), "hi");
        let st = run_op(&mut w, None, &[0, 0, 8, 5], op::ENUM);
        assert_eq!(st.string_stack.last().unwrap(), "none");
    }

    #[test]
    fn npc_mode_hunt_ops() {
        use crate::world::NpcInfo;
        let mut w = World::new();
        w.npc_info.insert(
            1,
            NpcInfo {
                name: "Hans".into(),
                size: 1,
                vislevel: -1,
                op: [Some("Talk-to".into()), None, Some("Trade".into()), None, None],
                ..Default::default()
            },
        );
        let nid = w.add_npc(1, 3200, 3200, 0).unwrap();
        assert_eq!(run_op_npc(&mut w, nid, &[], op::NPC_GETMODE).int_stack.last().copied(), Some(0));
        run_op_npc(&mut w, nid, &[5], op::NPC_SETHUNT);
        assert_eq!(w.npcs[nid].as_ref().unwrap().hunt_range, 5);
        run_op_npc(&mut w, nid, &[3], op::NPC_SETHUNTMODE);
        assert_eq!(w.npcs[nid].as_ref().unwrap().hunt_mode, 3);
        assert_eq!(run_op_npc(&mut w, nid, &[1], op::NPC_HASOP).int_stack.last().copied(), Some(1));
        assert_eq!(run_op_npc(&mut w, nid, &[2], op::NPC_HASOP).int_stack.last().copied(), Some(0));
        assert_eq!(run_op_npc(&mut w, nid, &[3], op::NPC_HASOP).int_stack.last().copied(), Some(1));
    }

    #[test]
    fn if_set_visual_ops_write_packets() {
        let mut world = World::new();
        let pid = world.add_player("p".into(), 3222, 3222, 0).unwrap();
        let com = (23 << 16) | 4;
        let cases = [
            (vec![com, 2920], op::IF_SETANIM, 176u8, 6usize),
            (vec![com, 0xFF0000], op::IF_SETCOLOUR, 234, 6),
            (vec![com, 9630], op::IF_SETMODEL, 251, 6),
            (vec![com, 995, 1], op::IF_SETOBJECT, 102, 10),
            (vec![com, 10, 20], op::IF_SETPOSITION, 85, 8),
            (vec![com, 1], op::IF_SETNPCHEAD, 66, 6),
            (vec![com], op::IF_SETPLAYERHEAD, 171, 4),
            (vec![com, 100], op::IF_SETSCROLLPOS, 50, 6),
            (vec![com, 150, 0, 560], op::IF_SETANGLE, 26, 10),
        ];
        for (ints, op_code, opcode, size) in cases {
            world.players[pid].as_mut().unwrap().out.clear();
            run_op(&mut world, Some(pid), &ints, op_code);
            let p = world.players[pid].as_ref().unwrap();
            let pkt = p.out.iter().find(|m| m.opcode == opcode)
                .unwrap_or_else(|| panic!("expected opcode {opcode}"));
            assert_eq!(pkt.body.len(), size, "opcode {opcode} body size");
        }
    }

    #[test]
    fn if_sethide_op_writes_hide_packet() {
        let mut world = World::new();
        let pid = world.add_player("p".into(), 3222, 3222, 0).unwrap();
        world.players[pid].as_mut().unwrap().out.clear();
        run_op(&mut world, Some(pid), &[(548 << 16) | 9, 1], op::IF_SETHIDE);
        let p = world.players[pid].as_ref().unwrap();
        let pkt = p.out.iter().find(|m| m.opcode == 84).expect("IF_SETHIDE (84)");
        assert_eq!(pkt.body.len(), 5, "p4 component + p1 hide flag");
    }

    #[test]
    fn minimap_toggle_op_writes_state_packet() {
        let mut world = World::new();
        let pid = world.add_player("p".into(), 3222, 3222, 0).unwrap();
        world.players[pid].as_mut().unwrap().out.clear();
        run_op(&mut world, Some(pid), &[2], op::MINIMAP_TOGGLE);
        let p = world.players[pid].as_ref().unwrap();
        let pkt = p.out.iter().find(|m| m.opcode == 190).expect("MINIMAP_TOGGLE (190)");
        assert_eq!(pkt.body, vec![2], "single state byte");
    }

    #[test]
    fn map_playercount_counts_within_rectangle_and_level() {
        let mut w = World::new();
        w.add_player("in1".into(), 3200, 3200, 0).unwrap();
        w.add_player("in2".into(), 3205, 3208, 0).unwrap();
        w.add_player("edge".into(), 3210, 3210, 0).unwrap(); // on the inclusive corner
        w.add_player("outx".into(), 3211, 3200, 0).unwrap(); // x beyond
        w.add_player("olvl".into(), 3202, 3202, 1).unwrap(); // right spot, wrong level
        // Rectangle (3200,3200)..(3210,3210) on level 0.
        let from = coord(0, 3200, 3200);
        let to = coord(0, 3210, 3210);
        assert_eq!(pushed(&mut w, &[from, to], op::MAP_PLAYERCOUNT), 3,
                   "in1, in2, edge — not outx (x>to) nor olvl (level 1)");
    }

    #[test]
    fn p_exactmove_op_drives_set_exact_move() {
        let mut world = World::new();
        let pid = world.add_player("m".into(), 3200, 3200, 0).unwrap();
        // glide (3200,3200)->(3203,3200), cycles 0..6, facing east(3).
        run_op(&mut world, Some(pid),
            &[coord(0, 3200, 3200), coord(0, 3203, 3200), 0, 6, 3], op::P_EXACTMOVE);

        let e = &world.players[pid].as_ref().unwrap().entity;
        assert_eq!((e.exact_start_x, e.exact_end_x), (3200, 3203));
        assert_eq!((e.exact_move_start, e.exact_move_end, e.exact_move_facing), (0, 6, 3));
        assert_eq!((e.x, e.z), (3203, 3200), "true tile snaps to the destination");
        assert_ne!(e.masks & player::MASK_EXACT_MOVE, 0);
    }

    #[test]
    fn p_exactmove_clears_walk_and_unsets_map_flag() {
        let mut world = World::new();
        let pid = world.add_player("m".into(), 3200, 3200, 0).unwrap();
        // Queue a walk first; the exact-move must discard it and tell the client.
        world.players[pid].as_mut().unwrap().entity.queue_waypoints(&[(3210, 3200)]);
        world.players[pid].as_mut().unwrap().out.clear();
        run_op(&mut world, Some(pid),
            &[coord(0, 3200, 3200), coord(0, 3203, 3200), 0, 6, 3], op::P_EXACTMOVE);
        let p = world.players[pid].as_ref().unwrap();
        assert!(p.entity.waypoints.is_empty(), "queued walk discarded");
        assert!(p.out.iter().any(|m| m.opcode == 161 && m.body.is_empty()),
                "UNSET_MAP_FLAG (161, empty body) sent");
    }

    #[test]
    fn spotanim_map_op_broadcasts_map_anim() {
        let mut world = World::new();
        let pid = world.add_player("p".into(), 3222, 3222, 0).unwrap();
        world.players[pid].as_mut().unwrap().out.clear();
        run_op(&mut world, None, &[120, coord(0, 3223, 3224), 0, 30], op::SPOTANIM_MAP);
        assert!(world.players[pid].as_ref().unwrap().out.iter().any(|m| m.opcode == 20),
            "MAP_ANIM (20) broadcast");
    }

    #[test]
    fn projanim_map_op_broadcasts_projectile() {
        let mut world = World::new();
        let pid = world.add_player("p".into(), 3222, 3222, 0).unwrap();
        world.players[pid].as_mut().unwrap().out.clear();
        run_op(&mut world, None,
            &[coord(0, 3225, 3225), coord(0, 3222, 3227), 220, 40, 36, 5, 25, 16, 0],
            op::PROJANIM_MAP);
        assert!(world.players[pid].as_ref().unwrap().out.iter().any(|m| m.opcode == 32),
            "MAP_PROJANIM (32) broadcast");
    }

    #[test]
    fn set_skill_level_op_sets_appearance_skill_level() {
        let mut world = World::new();
        let pid = world.add_player("p".into(), 3222, 3222, 0).unwrap();
        // Default is 0 -> client shows just the name.
        assert_eq!(world.players[pid].as_ref().unwrap().skill_level, 0);
        run_op(&mut world, Some(pid), &[126], op::SET_SKILL_LEVEL);
        assert_eq!(world.players[pid].as_ref().unwrap().skill_level, 126);
    }

    #[test]
    fn random_op_stays_in_range_and_varies() {
        let mut world = World::new();
        let mut prev = None;
        let mut varied = false;
        for _ in 0..50 {
            let v = pushed(&mut world, &[100], op::RANDOM);
            assert!((0..100).contains(&v), "random(100) must be in [0,100), got {v}");
            if prev.is_some_and(|p| p != v) {
                varied = true;
            }
            prev = Some(v);
        }
        assert!(varied, "the RNG should produce varying values");
        // Guard: random(n<=0) → 0.
        assert_eq!(pushed(&mut world, &[0], op::RANDOM), 0);
    }

    #[test]
    fn divide_and_modulo_by_zero_yield_zero() {
        // Engine-TS divides/mods in JS doubles then ToInt32s the result: a/0 is
        // ±Infinity → 0, n%0 is NaN → 0, and the script keeps running. OS used
        // to abort on a zero divisor — this pins the 1:1 behaviour.
        let mut world = World::new();
        assert_eq!(pushed(&mut world, &[7, 0], op::DIVIDE), 0, "7 / 0 == 0");
        assert_eq!(pushed(&mut world, &[-7, 0], op::DIVIDE), 0, "-7 / 0 == 0");
        assert_eq!(pushed(&mut world, &[7, 0], op::MODULO), 0, "7 % 0 == 0");
        // Non-zero divisors still truncate toward zero as before.
        assert_eq!(pushed(&mut world, &[7, 2], op::DIVIDE), 3, "7 / 2 == 3");
        assert_eq!(pushed(&mut world, &[-7, 2], op::DIVIDE), -3, "-7 / 2 == -3 (toward zero)");
        assert_eq!(pushed(&mut world, &[7, 2], op::MODULO), 1, "7 % 2 == 1");
    }

    #[test]
    fn p_run_sets_the_persistent_run_toggle_and_syncs_the_varp() {
        use crate::entity::MoveSpeed;
        use crate::script::state::Pointer;
        const VARP_SMALL: u8 = 88;
        let mut world = World::new();
        let pid = world.add_player("p".into(), 3222, 3222, 0).unwrap();

        let mut run_p_run = |world: &mut World, value: i32| {
            let mut st = ScriptState::new(Arc::new(op_script(&[value], op::P_RUN)), &[]);
            st.active_player = Some(pid);
            st.pointer_add(Pointer::ProtectedActivePlayer);
            assert_eq!(execute(&mut st, world), Execution::Finished);
        };

        world.players[pid].as_mut().unwrap().out.clear();
        run_p_run(&mut world, 1);
        let p = world.players[pid].as_ref().unwrap();
        assert!(p.run, "the persistent run toggle is set");
        assert!(p.out.iter().any(|m| m.opcode == VARP_SMALL), "the RUN varp is synced to the client");

        // The toggle drives movement: update_movement derives Run (and would still
        // next tick — the old direct move_speed set was lost to the recompute).
        world.players[pid].as_mut().unwrap().entity.queue_waypoints(&[(3226, 3222)]);
        world.players[pid].as_mut().unwrap().update_movement(None);
        assert_eq!(world.players[pid].as_ref().unwrap().entity.move_speed, MoveSpeed::Run);

        // P_RUN 0 clears it → movement walks again.
        run_p_run(&mut world, 0);
        assert!(!world.players[pid].as_ref().unwrap().run, "the toggle is cleared");
        world.players[pid].as_mut().unwrap().entity.queue_waypoints(&[(3230, 3222)]);
        world.players[pid].as_mut().unwrap().update_movement(None);
        assert_eq!(world.players[pid].as_ref().unwrap().entity.move_speed, MoveSpeed::Walk);
    }

    #[test]
    fn uid_op_pushes_the_hashed_uid_not_the_slot() {
        let mut world = World::new();
        let pid = world.add_player("bob".into(), 3222, 3222, 0).unwrap();
        let uid = run_op(&mut world, Some(pid), &[], op::UID).int_stack.last().copied().unwrap();
        assert_ne!(uid, pid as i32, "uid is the hashed value, not the raw slot");
        assert_eq!(world.get_player_by_uid(uid), Some(pid), "uid resolves back to the player");
    }

    #[test]
    fn runenergy_op_reads_the_energy() {
        let mut world = World::new();
        let pid = world.add_player("p".into(), 3222, 3222, 0).unwrap();
        world.players[pid].as_mut().unwrap().run_energy = 7500;
        let st = run_op(&mut world, Some(pid), &[], op::RUNENERGY);
        assert_eq!(st.int_stack.last().copied(), Some(7500), "RUNENERGY pushes the energy");
        assert_eq!(world.players[pid].as_ref().unwrap().run_energy, 7500, "it's a pure read");
    }

    #[test]
    fn staffmodlevel_op_reads_the_player_field() {
        let mut world = World::new();
        let pid = world.add_player("p".into(), 3222, 3222, 0).unwrap();
        // Default 0.
        assert_eq!(run_op(&mut world, Some(pid), &[], op::STAFFMODLEVEL).int_stack.last().copied(), Some(0));
        // Reflects the field (consistent with AFK_EVENT's gate), not a constant.
        world.players[pid].as_mut().unwrap().staff_mod_level = 2;
        assert_eq!(run_op(&mut world, Some(pid), &[], op::STAFFMODLEVEL).int_stack.last().copied(), Some(2));
    }

    #[test]
    fn lowmem_op_and_sound_guard_respect_the_flag() {
        const MIDI_SONG: u8 = 211;
        let mut world = World::new();
        let pid = world.add_player("p".into(), 3222, 3222, 0).unwrap();

        // High-memory (default): LOWMEM pushes 0 and music is sent.
        assert_eq!(run_op(&mut world, Some(pid), &[], op::LOWMEM).int_stack.last().copied(), Some(0));
        world.players[pid].as_mut().unwrap().out.clear();
        run_op(&mut world, Some(pid), &[55], op::MIDI_SONG);
        assert!(world.players[pid].as_ref().unwrap().out.iter().any(|m| m.opcode == MIDI_SONG),
            "high-detail client receives music");

        // Low-memory: LOWMEM pushes 1 and music playback is skipped.
        world.players[pid].as_mut().unwrap().low_memory = true;
        assert_eq!(run_op(&mut world, Some(pid), &[], op::LOWMEM).int_stack.last().copied(), Some(1));
        world.players[pid].as_mut().unwrap().out.clear();
        run_op(&mut world, Some(pid), &[55], op::MIDI_SONG);
        assert!(world.players[pid].as_ref().unwrap().out.iter().all(|m| m.opcode != MIDI_SONG),
            "low-detail client receives no music");
    }

    #[test]
    fn substring_clamps_and_swaps_like_js() {
        let mut world = World::new();
        // SUBSTRING pops the string (top), then [start, end] as pop_ints::<2>.
        let mut sub = |s: &str, start: i32, end: i32| -> String {
            let mut st = ScriptState::new(Arc::new(op_script(&[start, end], op::SUBSTRING)), &[]);
            st.push_string(s.to_string());
            assert_eq!(execute(&mut st, &mut world), Execution::Finished);
            st.string_stack.last().cloned().unwrap()
        };
        assert_eq!(sub("hello", 1, 3), "el", "normal range");
        assert_eq!(sub("hello", 3, 1), "el", "reversed range swaps");
        assert_eq!(sub("hello", -2, 3), "hel", "negative start clamps to 0");
        assert_eq!(sub("hello", 2, 10), "llo", "end past length clamps");
        assert_eq!(sub("hello", 10, 2), "llo", "start past length + swap");
        assert_eq!(sub("hello", 2, 2), "", "empty range");
    }

    #[test]
    fn compare_returns_unclamped_java_compareto() {
        let mut world = World::new();
        let mut cmp = |a: &str, b: &str| -> i32 {
            let mut st = ScriptState::new(Arc::new(op_script(&[], op::COMPARE)), &[]);
            st.push_string(a.to_string()); // parts[0]
            st.push_string(b.to_string()); // parts[1]
            assert_eq!(execute(&mut st, &mut world), Execution::Finished);
            st.int_stack.last().copied().unwrap()
        };
        // The difference is the char-code delta, NOT a clamped -1/0/1.
        assert_eq!(cmp("a", "c"), -2, "'a'(97) - 'c'(99)");
        assert_eq!(cmp("zoo", "ant"), 25, "'z'(122) - 'a'(97)");
        assert_eq!(cmp("hello", "hello"), 0, "equal");
        assert_eq!(cmp("abc", "ab"), 1, "prefix → length difference");
        assert_eq!(cmp("ab", "abc"), -1, "shorter prefix");
    }

    #[test]
    fn string_indexof_string_searches_top_for_the_other() {
        let mut world = World::new();
        // Engine-TS pops `text` first (top) and `find` second (bottom), then does
        // `text.indexOf(find)` — push `find` then `text` so `text` is on top.
        let mut idx = |text: &str, find: &str| -> i32 {
            let mut st = ScriptState::new(Arc::new(op_script(&[], op::STRING_INDEXOF_STRING)), &[]);
            st.push_string(find.to_string()); // bottom
            st.push_string(text.to_string()); // top
            assert_eq!(execute(&mut st, &mut world), Execution::Finished);
            st.int_stack.last().copied().unwrap()
        };
        assert_eq!(idx("hello world", "world"), 6, "substring found");
        assert_eq!(idx("abcabc", "bc"), 1, "first occurrence");
        assert_eq!(idx("hello", "xyz"), -1, "not found → -1");
    }

    #[test]
    fn npc_tele_jumps_only_on_telejump_or_level_change() {
        let mut world = World::new();
        let nid = world.add_npc(11, 3222, 3222, 0).unwrap();
        let jumped = |w: &World| w.npcs[nid].as_ref().unwrap().entity.jump;

        // NPC_TELE within the same level: snap, but no jump.
        run_op_npc(&mut world, nid, &[coord(0, 3225, 3222)], op::NPC_TELE);
        assert!(!jumped(&world), "NPC_TELE on the same level does not jump");

        // NPC_TELEJUMP always jumps.
        run_op_npc(&mut world, nid, &[coord(0, 3226, 3222)], op::NPC_TELEJUMP);
        assert!(jumped(&world), "NPC_TELEJUMP jumps");

        // NPC_TELE across levels is forced to jump (can't glide between planes).
        run_op_npc(&mut world, nid, &[coord(1, 3226, 3222)], op::NPC_TELE);
        assert!(jumped(&world), "NPC_TELE across a level change jumps");
    }

    #[test]
    fn npc_find_picks_the_closest_of_type() {
        let mut world = World::new();
        let near = world.add_npc(11, 3224, 3222, 0).unwrap(); // 2 tiles east
        let _far = world.add_npc(11, 3230, 3222, 0).unwrap(); // 8 tiles east
        let other = world.add_npc(22, 3223, 3222, 0).unwrap(); // closer, but wrong type
        let c = coord(0, 3222, 3222);

        // Closest type-11 npc within distance 10 is `near` (not the wrong-type one).
        let st = run_op(&mut world, None, &[c, 11, 10, 0], op::NPC_FIND);
        assert_eq!(st.int_stack.last().copied(), Some(1), "found a match");
        assert_eq!(st.active_npc, Some(near), "picked the closest type-11 npc");

        // Type 22 resolves to the other npc.
        assert_eq!(run_op(&mut world, None, &[c, 22, 10, 0], op::NPC_FIND).active_npc, Some(other));

        // Within distance 1, no type-11 npc qualifies → push 0, no active npc.
        let st = run_op(&mut world, None, &[c, 11, 1, 0], op::NPC_FIND);
        assert_eq!(st.int_stack.last().copied(), Some(0), "none in range");
        assert_eq!(st.active_npc, None);
    }

    #[test]
    fn obj_addall_drops_a_public_item_everyone_sees() {
        let mut world = World::new();
        let p1 = world.add_player("p1".into(), 3222, 3222, 0).unwrap();
        let p2 = world.add_player("p2".into(), 3223, 3222, 0).unwrap();
        let c = coord(0, 3222, 3222);

        // OBJ_ADDALL is public the instant it drops — both players can see it.
        run_op(&mut world, Some(p1), &[c, 995, 100, 200], op::OBJ_ADDALL);
        assert_eq!(world.find_obj(3222, 3222, 0, 995, p1), Some(100), "dropper sees the public drop");
        assert_eq!(world.find_obj(3222, 3222, 0, 995, p2), Some(100), "bystander sees the public drop");

        // Contrast: plain OBJ_ADD is private to the dropper during its reveal window.
        run_op(&mut world, Some(p1), &[c, 1004, 1, 200], op::OBJ_ADD);
        assert_eq!(world.find_obj(3222, 3222, 0, 1004, p1), Some(1), "dropper sees the private drop");
        assert_eq!(world.find_obj(3222, 3222, 0, 1004, p2), None, "bystander can't see the private drop");
    }

    #[test]
    fn movecoord_masks_components_like_packcoord() {
        let mut world = World::new();
        // Normal offset: (level 0, 3200, 3200) + (dx 5, dy 0, dz -3).
        let base = coord(0, 3200, 3200);
        assert_eq!(pushed(&mut world, &[base, 5, 0, -3], op::MOVECOORD), coord(0, 3205, 3197));
        // x overflow wraps within its 14-bit field instead of bleeding into level:
        // 0x3fff + 1 → 0, level stays 0 (it corrupted level to 1 before the mask).
        let edge = coord(0, 0x3fff, 100);
        assert_eq!(pushed(&mut world, &[edge, 1, 0, 0], op::MOVECOORD), coord(0, 0, 100));
    }

    #[test]
    fn interpolate_and_scale_by_zero_yield_zero() {
        // Same JS-double / ToInt32 path as DIVIDE: a zero denominator makes the
        // expression ±Infinity/NaN, which ToInt32s to 0 — no script abort.
        let mut world = World::new();
        // INTERPOLATE operands: y0, y1, x0, x1, x.
        assert_eq!(pushed(&mut world, &[5, 9, 2, 2, 3], op::INTERPOLATE), 0, "x1 == x0 → 0");
        // Normal lerp: slope floor((9-5)/(4-2)) = 2; at x=3 → 2*(3-2)+5 = 7.
        assert_eq!(pushed(&mut world, &[5, 9, 2, 4, 3], op::INTERPOLATE), 7, "normal interpolation");
        // SCALE operands: a, b, c → (a*c)/b.
        assert_eq!(pushed(&mut world, &[10, 0, 3], op::SCALE), 0, "scale by zero → 0");
        assert_eq!(pushed(&mut world, &[10, 4, 3], op::SCALE), 7, "normal scale (10*3)/4 == 7");
    }

    #[test]
    fn stat_advance_awards_xp() {
        let mut world = World::new();
        let pid = world.add_player("p".into(), 3222, 3222, 0).unwrap();
        world.players[pid].as_mut().unwrap().out.clear();
        // 83 xp into attack = exactly level 2.
        run_op(&mut world, Some(pid), &[player::STAT_ATTACK as i32, 83], op::STAT_ADVANCE);
        let p = world.players[pid].as_ref().unwrap();
        assert_eq!(p.experience[player::STAT_ATTACK], 83);
        assert_eq!(p.base_levels[player::STAT_ATTACK], 2);
    }

    #[test]
    fn stat_boost_fires_changestat_script() {
        use crate::script::provider::ScriptProvider;
        use crate::script::trigger;
        let mut world = World::new();
        // [changestat, attack] plays midi 55 (packet opcode 211).
        let mut provider = ScriptProvider::test_empty();
        provider.test_insert_specific(trigger::CHANGESTAT, player::STAT_ATTACK as i32,
            Arc::new(op_script(&[55], op::MIDI_SONG)));
        world.scripts = Some(provider);
        let pid = world.add_player("p".into(), 3222, 3222, 0).unwrap();

        // Boost attack — the level changes, so CHANGESTAT enqueues.
        run_op(&mut world, Some(pid), &[player::STAT_ATTACK as i32, 5, 0], op::STAT_BOOST);
        world.players[pid].as_mut().unwrap().out.clear();
        world.cycle(); // process the engine queue
        assert!(world.players[pid].as_ref().unwrap().out.iter().any(|m| m.opcode == 211),
                "changestat script fired after the boost");

        // A no-op boost (0/0) leaves the level unchanged → no changestat.
        run_op(&mut world, Some(pid), &[player::STAT_ATTACK as i32, 0, 0], op::STAT_BOOST);
        world.players[pid].as_mut().unwrap().out.clear();
        world.cycle();
        assert!(world.players[pid].as_ref().unwrap().out.iter().all(|m| m.opcode != 211),
                "no changestat when the level doesn't change");
    }

    #[test]
    fn damage_op_hits_player_resolved_by_uid() {
        let mut world = World::new();
        let pid = world.add_player("v".into(), 3222, 3222, 0).unwrap();
        let uid = world.players[pid].as_ref().unwrap().uid();
        let hp0 = world.players[pid].as_ref().unwrap().levels[player::STAT_HITPOINTS];
        // Stack push order is uid, type, amount (Engine-TS pops amount first).
        run_op(&mut world, None, &[uid, 0, 4], op::DAMAGE);
        let p = world.players[pid].as_ref().unwrap();
        // apply_damage subtracts from levels[HITPOINTS] and flags the mask.
        assert_eq!(p.levels[player::STAT_HITPOINTS], hp0 - 4, "hp reduced by the hit");
        assert_ne!(p.entity.masks & player::MASK_DAMAGE, 0, "DAMAGE mask flagged");
    }

    #[test]
    fn damage_op_ignores_unknown_uid() {
        // A stale/unknown uid must be a no-op, not a script error.
        let mut world = World::new();
        let _pid = world.add_player("v".into(), 3222, 3222, 0).unwrap();
        run_op(&mut world, None, &[0x7FFF_FFF0, 0, 4], op::DAMAGE);
    }

    #[test]
    fn stat_random_value_interpolates_between_bounds() {
        // Level 1 collapses to the low bound (+1); level 99 to the high bound
        // (+1); the midpoint blends both halves — matching Engine-TS exactly.
        assert_eq!(stat_random_value(1, 40, 200), 41);
        assert_eq!(stat_random_value(99, 40, 200), 201);
        // level 50: 40*49/98 + 200*49/98 + 1 = 20 + 100 + 1 = 121
        assert_eq!(stat_random_value(50, 40, 200), 121);
        // Boosted above 99: the low half is negative and floors toward -inf
        // (Math.floor), not toward zero. level 100: floor(-40/98) + floor(19800/98)
        // + 1 = -1 + 202 + 1 = 202 (a truncating `/` would give 203).
        assert_eq!(stat_random_value(100, 40, 200), 202);
        // Drained to 0: the high half is negative. floor(3960/98) + floor(-200/98)
        // + 1 = 40 + (-3) + 1 = 38 (truncation would give 39).
        assert_eq!(stat_random_value(0, 40, 200), 38);
    }

    #[test]
    fn stat_random_op_succeeds_when_value_exceeds_max_roll() {
        // value = high+1 = 256 at level 99; the roll maxes at 255, so the op
        // must push 1 (success) on every call regardless of the RNG.
        let mut world = World::new();
        let pid = world.add_player("p".into(), 3222, 3222, 0).unwrap();
        world.players[pid].as_mut().unwrap().set_level(player::STAT_ATTACK, 99);
        for _ in 0..64 {
            let st = run_op(&mut world, Some(pid),
                &[player::STAT_ATTACK as i32, 0, 255], op::STAT_RANDOM);
            assert_eq!(*st.int_stack.last().unwrap(), 1,
                "guaranteed-success roll always pushes 1");
        }
    }

    #[test]
    fn stat_random_op_reads_current_boosted_level() {
        // STAT_RANDOM uses the *current* level, so a boost changes the odds.
        // Boost attack to 99 and use bounds that only clear the 255 ceiling at
        // high level — proves the boosted level (not the base) drives the roll.
        let mut world = World::new();
        let pid = world.add_player("p".into(), 3222, 3222, 0).unwrap();
        {
            let p = world.players[pid].as_mut().unwrap();
            p.set_level(player::STAT_ATTACK, 1);
            p.stat_boost(player::STAT_ATTACK, 98, 0); // current 1 -> 99
        }
        for _ in 0..64 {
            let st = run_op(&mut world, Some(pid),
                &[player::STAT_ATTACK as i32, 0, 255], op::STAT_RANDOM);
            assert_eq!(*st.int_stack.last().unwrap(), 1,
                "boosted level 99 -> value 256 -> always succeeds");
        }
    }
}
