//! End-to-end check that the RuneScript compiler's output runs on the engine.
//! Loads the pack produced by `runescript/` (compiled from
//! `Content/scripts/login.rs2`), fires the `[login,_]` trigger via add_player,
//! and asserts the welcome-screen flow: the mes() line, the interface opens,
//! and the `[if_button,if_378:6]` "Click here to play" → IF_OPENTOP(548).

use engine::World;
use protocol::client::ClientMessage;
use protocol::server as msg;

const MESSAGE_GAME: u8 = 100;

fn message_text(body: &[u8]) -> String {
    // pjstr: bytes up to the null terminator.
    let end = body.iter().position(|&b| b == 0).unwrap_or(body.len());
    String::from_utf8_lossy(&body[..end]).to_string()
}

#[test]
fn compiled_login_script_runs() {
    let pack = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/pack");

    let mut world = World::new();
    world.load_scripts(pack);

    let pid = world.add_player("tester".into(), 3222, 3222, 0).unwrap();

    {
        let player = world.players[pid].as_ref().expect("player");
        let messages: Vec<String> = player
            .out
            .iter()
            .filter(|m| m.opcode == MESSAGE_GAME)
            .map(|m| message_text(&m.body))
            .collect();
        assert_eq!(
            messages,
            vec!["Welcome to RuneScape.".to_string()],
            "compiled [login,_] should emit its mes() line",
        );

        // [login,_] opens the welcome holder (549), not the game frame.
        let opentop = msg::if_opentop(549);
        assert!(
            player.out.iter().any(|m| m.opcode == opentop.opcode && m.body == opentop.body),
            "login script should IF_OPENTOP the welcome holder 549",
        );
    }

    // "Click here to play" — the rev1 client sends IF_BUTTON with the packed
    // component (378 << 16) | 6; the [if_button,if_378:6] script answers with
    // the game frame. This exercises the v27 i64 lookup keys end to end.
    world.handle_message(pid, ClientMessage::IfButton {
        op: 1,
        component: (378 << 16) | 6,
        sub: -1,
    });

    let player = world.players[pid].as_ref().expect("player");
    let opentop = msg::if_opentop(548);
    assert!(
        player.out.iter().any(|m| m.opcode == opentop.opcode && m.body == opentop.body),
        "[if_button,if_378:6] should IF_OPENTOP the game frame 548",
    );
}
