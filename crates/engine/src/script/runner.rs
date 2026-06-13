//! Script executor — mirrors the Engine-TS reference ScriptRunner.ts.
//! One dispatch loop with the op handlers inlined per section
//! (CoreOps / NumberOps / StringOps / ServerOps / PlayerOps / NpcOps
//! in the reference). Entity-dependent ops resolve their handles
//! through &mut World.

use crate::entity::{npc, player};
use crate::script::opcode as op;
use crate::script::state::{Execution, Pointer, ScriptArg, ScriptState};
use crate::world::World;

use protocol::server as msg;

/// Run `state` until it finishes, suspends, or errors. Errors print a
/// stack backtrace (script name + line numbers) like the reference
/// and report to the player when one is attached.
pub fn execute(state: &mut ScriptState, world: &mut World) -> Execution {
    state.execution = Execution::Running;

    let result = run(state, world);
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
        op::SETBIT_RANGE | op::CLEARBIT_RANGE | op::GETBIT_RANGE
        | op::SETBIT_RANGE_TOINT | op::SIN_DEG | op::COS_DEG | op::ATAN2_DEG
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
            // independently, then swap them when start > end. OS1 previously took
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
            // OS1 previously searched the other way round. `indexOf` is a character
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
            // Engine-TS OBJ_DEL: remove the active ground item. OS1's objs are
            // DESPAWN-lifecycle, so there's no config respawn rate to schedule.
            let (x, z, level, id) = state.active_obj.ok_or("no active_obj")?;
            world.remove_obj_broadcast(x, z, level, id);
        }
        op::OBJ_ADD => {
            // Engine-TS OBJ_ADD: drop a ground item owned by the active player
            // with a despawn duration, and make it the active obj. (The
            // stackable split for non-stackable count > 1 and the members /
            // dummyitem gates need obj config; OS1 drops the one-pile model.)
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
            // (Stackable-split / members / dummyitem gates need obj config; OS1
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
            // or -1 = permanent) and make it the active loc. (OS1 keeps a flat
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
                p.entity.clear_interaction();
            }
            world.close_modal(pid);
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
            // OS1 did) was overwritten by that recompute before the step ran — the
            // toggle had no effect and never persisted across ticks.
            state.pointer_check(Pointer::ProtectedActivePlayer)?;
            let run = state.pop_int();
            let p = active_player(state, world)?;
            p.run = run != 0;
            p.set_var(player::VARP_RUN, run);
        }
        op::P_WALK => {
            state.pointer_check(Pointer::ProtectedActivePlayer)?;
            let coord = state.pop_int();
            let p = active_player(state, world)?;
            let (x, z) = ((coord >> 14) & 0x3fff, coord & 0x3fff);
            p.entity.queue_waypoints(&[(x, z)]);
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
            // (0..=10000). OS1 previously implemented it as a setter, which both
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
            // P_TELEJUMP split (Engine-TS `teleport` vs `teleJump`). OS1 previously
            // forced a jump for both.
            let coord = state.pop_int();
            let jump = opcode == op::NPC_TELEJUMP;
            let n = active_npc(state, world)?;
            n.entity.teleport((coord >> 14) & 0x3fff, coord & 0x3fff, (coord >> 28) & 0x3, jump);
        }
        op::NPC_WALK => {
            let coord = state.pop_int();
            let n = active_npc(state, world)?;
            let (x, z) = ((coord >> 14) & 0x3fff, coord & 0x3fff);
            n.entity.queue_waypoints(&[(x, z)]);
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
        // ±Infinity → 0, n%0 is NaN → 0, and the script keeps running. OS1 used
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
        world.players[pid].as_mut().unwrap().update_movement();
        assert_eq!(world.players[pid].as_ref().unwrap().entity.move_speed, MoveSpeed::Run);

        // P_RUN 0 clears it → movement walks again.
        run_p_run(&mut world, 0);
        assert!(!world.players[pid].as_ref().unwrap().run, "the toggle is cleared");
        world.players[pid].as_mut().unwrap().entity.queue_waypoints(&[(3230, 3222)]);
        world.players[pid].as_mut().unwrap().update_movement();
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
