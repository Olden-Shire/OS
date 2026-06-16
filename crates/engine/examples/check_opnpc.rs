//! Decisive end-to-end check of the newbie-basics-instructor dialogue, fully
//! server-side: load the real compiled bundle, place npc 945 next to a player,
//! fire the OPNPC1 interaction, cycle, and report whether the script ran and
//! what packets it emitted. Isolates "script runs + emits dialogue" (=> client
//! render bug) from "script never runs / aborts".
//! Usage: cargo run --release --example check_opnpc -p engine

use engine::script::provider::ScriptProvider;
use engine::script::trigger;
use engine::World;

fn main() {
    let provider = match ScriptProvider::load("data/pack") {
        Ok(p) => p,
        Err(e) => { eprintln!("load failed: {e}"); std::process::exit(1); }
    };
    println!("opnpc registered: {}", provider.get_by_trigger(trigger::OPNPC1, 945, -1).is_some());

    let mut world = World::new();
    world.scripts = Some(provider);
    world.load_mesanim(std::path::Path::new("Content"));
    println!("mesanim neutral -> {:?}", world.mesanim.get("neutral"));
    println!("mesanim happy   -> {:?}", world.mesanim.get("happy"));

    // Player adjacent to a freshly-spawned RuneScape Guide (npc 945).
    let pid = world.add_player("tester".into(), 3200, 3200, 0).expect("add player");
    let nid = world.add_npc(945, 3201, 3200, 0).expect("add npc 945");
    // Drive the LIVE path: the decoded OPNPC1 ("Talk-to") client packet.
    world.handle_message(pid, protocol::client::ClientMessage::OpNpc { nid: nid as i32, op: 1 });
    world.players[pid].as_mut().unwrap().out.clear();

    world.cycle();

    let p = world.players[pid].as_ref().unwrap();
    let opcodes: Vec<u8> = p.out.iter().map(|m| m.opcode).collect();
    println!("interaction still set: {}", p.interaction.is_some());
    println!("emitted packet opcodes after click+cycle: {opcodes:?}");
    // if_opensub == 184 (the dialogue-open packet our IF_OPENCHAT emits).
    println!("contains IF_OPENSUB(184) dialogue-open: {}", opcodes.contains(&184));
}
