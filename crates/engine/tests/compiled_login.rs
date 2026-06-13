//! End-to-end check that the RuneScript compiler's output runs on the engine.
//! Loads the pack produced by `runescript/` (compiled from
//! `Content/scripts/login_logout/login.rs2`), fires the `[login,_]` trigger
//! via add_player, and asserts the welcome-screen flow: `[login,_]` calls
//! `~welcome_screen`, which IF_OPENTOPs the overlay holder (549) and docks
//! the welcome-screen-top (378) and secondary panel (23) into it as overlay
//! subcomponents at 549:com_2 / 549:com_3. Exercises the .rs2 if_opentop /
//! if_opensub commands + interface.pack symbol resolution end to end.

use engine::World;
use protocol::server as msg;

#[test]
fn compiled_login_script_runs() {
    let pack = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/pack");

    let mut world = World::new();
    world.load_scripts(pack);

    let pid = world.add_player("tester".into(), 3222, 3222, 0).unwrap();
    let player = world.players[pid].as_ref().expect("player");
    let sent = |m: &msg::ServerPacket| {
        player.out.iter().any(|o| o.opcode == m.opcode && o.body == m.body)
    };

    // [login,_] → ~welcome_screen: open the overlay holder toplevel.
    assert!(sent(&msg::if_opentop(549)), "should IF_OPENTOP the overlay holder 549");
    // Dock 378 (welcome top) and 23 (secondary) as overlay subcomponents.
    // if_opensub(parent:child, sub, kind) → packed component (parent<<16)|child.
    assert!(
        sent(&msg::if_opensub((549 << 16) | 2, 378, 1)),
        "should IF_OPENSUB 378 into 549:com_2 as overlay",
    );
    assert!(
        sent(&msg::if_opensub((549 << 16) | 3, 23, 1)),
        "should IF_OPENSUB 23 into 549:com_3 as overlay",
    );
}
