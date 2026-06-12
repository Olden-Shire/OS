//! Regenerate the Kotlin compiler/plugin's cs2 opcode data from the canonical table in
//! `cs2_opcodes.rs`:
//!
//! * `runescript/compiler/src/main/resources/cs2_opcodes.tsv` — full metadata
//!   (name, mnemonic, operand kind, net stack deltas; deltas empty for dynamic-arity ops)
//! * `runescript/data/symbols/clientscript_command.pack` — `<opcode>=<name>` for every
//!   command (op >= 100), consumed by the plugin's annotator/completion
//!
//! Run from the workspace root after any opcode-table change:
//! `cargo run --example gen_kotlin_cs2_data -p cache`

use cache::cs2_opcodes::{all_opcodes, mnemonic, opcode_name, operand_kind, stack_delta};

fn main() {
    let mut tsv = String::from("op\tname\tmnemonic\tkind\tintDelta\tstrDelta\n");
    let mut pack = String::from(
        "// ClientScript (cs2) command table - generated from crates/cache/src/cs2_opcodes.rs\n\
         // via `cargo run --example gen_kotlin_cs2_data -p cache`. Do not edit by hand.\n\
         // Format: <cs2 opcode>=<command name>. Sourced from ScriptRunner.java dispatch.\n",
    );

    for op in all_opcodes() {
        let name = opcode_name(op).expect("table entry has a name");
        let (int_delta, str_delta) = match stack_delta(op) {
            Some((i, s)) => (i.to_string(), s.to_string()),
            None => (String::new(), String::new()),
        };
        tsv.push_str(&format!(
            "{op}\t{name}\t{}\t{:?}\t{int_delta}\t{str_delta}\n",
            mnemonic(op),
            operand_kind(op),
        ));
        if op >= 100 {
            pack.push_str(&format!("{op}={name}\n"));
        }
    }

    let tsv_path = "runescript/compiler/src/main/resources/cs2_opcodes.tsv";
    let pack_path = "runescript/data/symbols/clientscript_command.pack";
    std::fs::write(tsv_path, &tsv).expect("write tsv");
    std::fs::write(pack_path, &pack).expect("write pack");
    println!(
        "wrote {} opcodes to {tsv_path} and the command table to {pack_path}",
        all_opcodes().count()
    );
}
