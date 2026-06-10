//! Script executor — mirrors the Engine-TS reference ScriptRunner.ts.
//! One dispatch loop with the op handlers inlined per section
//! (CoreOps / NumberOps / StringOps / ServerOps / PlayerOps / NpcOps
//! in the reference). Entity-dependent ops resolve their handles
//! through &mut World.

use crate::entity::{npc, player};
use crate::script::opcode as op;
use crate::script::state::{Execution, Pointer, ScriptState};
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
            if varp < p.varps.len() {
                p.varps[varp] = v;
            }
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
            if b == 0 {
                return Err("division by zero".to_string());
            }
            state.push_int(a.wrapping_div(b));
        }
        op::MODULO => {
            let [a, b] = state.pop_ints::<2>();
            if b == 0 {
                return Err("modulo by zero".to_string());
            }
            state.push_int(a.wrapping_rem(b));
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
            if x1 == x0 {
                return Err("interpolate with x1 == x0".to_string());
            }
            let lerp = ((y1 - y0) as f64 / (x1 - x0) as f64).floor() as i32 * (x - x0) + y0;
            state.push_int(lerp);
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
            if b == 0 {
                return Err("scale by zero".to_string());
            }
            state.push_int(((a as i64 * c as i64) / b as i64) as i32);
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
            state.push_int(match parts[0].cmp(&parts[1]) {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            });
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
            let sub: String = s.chars()
                .skip(start.max(0) as usize)
                .take((end - start).max(0) as usize)
                .collect();
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
            let needle = state.pop_string();
            let hay = state.pop_string();
            state.push_int(hay.find(&needle).map_or(-1, |i| i as i32));
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
            let x = ((coord >> 14) & 0x3fff) + dx;
            let y = ((coord >> 28) & 0x3) + dy;
            let z = (coord & 0x3fff) + dz;
            state.push_int((y << 28) | (x << 14) | z);
        }
        op::PLAYERCOUNT => {
            let count = world.players.iter().filter(|p| p.is_some()).count();
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
            p.entity.anim_id = seq;
            p.entity.anim_delay = delay;
            p.entity.masks |= player::MASK_ANIM;
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
            let p = active_player(state, world)?;
            p.entity.face_x = (coord >> 14) & 0x3fff;
            p.entity.face_z = coord & 0x3fff;
            p.entity.masks |= player::MASK_FACE_COORD;
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
            let p = active_player(state, world)?;
            state.push_int(p.pid as i32);
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
            let run = state.pop_int();
            let p = active_player(state, world)?;
            p.entity.run = run == 1;
        }
        op::P_WALK => {
            state.pointer_check(Pointer::ProtectedActivePlayer)?;
            let coord = state.pop_int();
            let p = active_player(state, world)?;
            let (x, z) = ((coord >> 14) & 0x3fff, coord & 0x3fff);
            p.entity.queue_waypoints(&[(x, z)]);
        }
        op::P_LOGOUT => {
            state.pointer_check(Pointer::ProtectedActivePlayer)?;
            let p = active_player(state, world)?;
            p.logging_out = true;
            p.write(msg::logout());
        }
        op::RUNENERGY => {
            let v = state.pop_int();
            let p = active_player(state, world)?;
            p.run_energy = v.clamp(0, 100);
            let energy = p.run_energy;
            p.write(msg::update_runenergy(energy));
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
        op::MIDI_SONG => {
            let id = state.pop_int();
            let p = active_player(state, world)?;
            p.write(msg::midi_song(id));
        }
        op::MIDI_JINGLE => {
            // Reference pops (delay, jingle) — rev1 wire carries only
            // the id; the trailing bytes are unused by the client.
            let [_delay, id] = state.pop_ints::<2>();
            let p = active_player(state, world)?;
            p.write(msg::midi_jingle(id));
        }
        op::SOUND_SYNTH => {
            let [synth, loops, delay] = state.pop_ints::<3>();
            let p = active_player(state, world)?;
            p.write(msg::synth_sound(synth, loops, delay));
        }
        op::STAFFMODLEVEL => {
            state.push_int(0);
        }
        op::LOWMEM => {
            state.push_int(0);
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
            let text = state.pop_string();
            let n = active_npc(state, world)?;
            n.entity.chat = Some(text);
            n.entity.masks |= npc::MASK_SAY;
        }
        op::NPC_FACESQUARE => {
            let coord = state.pop_int();
            let n = active_npc(state, world)?;
            n.entity.face_x = (coord >> 14) & 0x3fff;
            n.entity.face_z = coord & 0x3fff;
            n.entity.masks |= npc::MASK_FACE_COORD;
        }
        op::NPC_COORD => {
            let n = active_npc(state, world)?;
            let coord = ((n.entity.level & 0x3) << 28)
                | ((n.entity.x & 0x3fff) << 14)
                | (n.entity.z & 0x3fff);
            state.push_int(coord);
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
            let coord = state.pop_int();
            let n = active_npc(state, world)?;
            n.entity.teleport((coord >> 14) & 0x3fff, coord & 0x3fff, (coord >> 28) & 0x3, true);
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
fn rand_unit() -> f64 {
    use std::sync::Mutex;
    static SEED: Mutex<u64> = Mutex::new(0x9E37_79B9_7F4A_7C15);
    let mut g = SEED.lock().unwrap();
    *g = g.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    ((*g >> 11) as f64 / (1u64 << 53) as f64).clamp(0.0, 1.0 - f64::EPSILON)
}
