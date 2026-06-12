//! Decompiler coverage survey: lift + structure every archive-12 script and report
//! which ones fail (and why). With script ids as args, debug-prints their IR instead.
//!
//! Usage:
//!   cargo run --example cs2_lift_survey -p cache            # survey all
//!   cargo run --example cs2_lift_survey -p cache -- 3 14    # dump IR for 3 and 14

use std::collections::BTreeMap;
use std::path::Path;

use cache::cs2::ClientScript;
use cache::cs2_decompile::lift;
use cache::cs2_sig::analyze_all;
use cache::Cache;

const CLIENTSCRIPTS_ARCHIVE: u8 = 12;

fn main() {
    let ids: Vec<u32> = std::env::args().skip(1).filter_map(|s| s.parse().ok()).collect();

    let mut cache = Cache::open(Path::new("cache")).expect("open cache/");
    let group_ids: Vec<i32> = cache.index(CLIENTSCRIPTS_ARCHIVE).group_ids.clone();
    let mut scripts: BTreeMap<u32, ClientScript> = BTreeMap::new();
    for gid in group_ids {
        let gid = gid as u32;
        if let Ok(Some(bytes)) = cache.read_group(CLIENTSCRIPTS_ARCHIVE, gid) {
            if let Some(s) = ClientScript::decode(&bytes) {
                scripts.insert(gid, s);
            }
        }
    }

    let analysis = analyze_all(&scripts);
    if !analysis.diags.is_empty() {
        for d in &analysis.diags {
            eprintln!("sig FAIL {d}");
        }
    }

    if !ids.is_empty() {
        let names = cache::cs2_asm::NameMaps::new();
        for id in ids {
            println!("=== script {id} ===");
            match lift(id, &scripts[&id], &analysis.sigs) {
                Ok(ir) => println!("{}", cache::cs2_source::print(&ir, &names)),
                Err(e) => println!("LIFT FAILED: {e}"),
            }
        }
        return;
    }

    let mut lifted = 0usize;
    let mut byte_exact = 0usize;
    let mut failures: Vec<(u32, String)> = Vec::new();
    for (&id, s) in &scripts {
        let ir = match lift(id, s, &analysis.sigs) {
            Ok(ir) => ir,
            Err(e) => {
                failures.push((id, format!("lift: {e}")));
                continue;
            }
        };
        lifted += 1;
        let back = match cache::cs2_compile::compile(&ir) {
            Ok(b) => b,
            Err(e) => {
                failures.push((id, format!("compile: {e}")));
                continue;
            }
        };
        let names = cache::cs2_asm::NameMaps::new();
        let text = cache::cs2_source::print(&ir, &names);
        match cache::cs2_source::parse(&text, &names, &analysis.sigs) {
            Ok(reparsed) if reparsed == ir => {}
            Ok(_) => {
                failures.push((id, "text round-trip: reparsed IR differs".to_owned()));
                continue;
            }
            Err(e) => {
                failures.push((id, format!("text round-trip: parse failed: {e}")));
                continue;
            }
        }
        if back.encode() == s.encode() {
            byte_exact += 1;
        } else {
            let n = s.instructions.len().min(back.instructions.len());
            let first_diff = (0..n)
                .find(|&i| {
                    s.instructions[i] != back.instructions[i]
                        || s.int_operands[i] != back.int_operands[i]
                        || s.string_operands[i] != back.string_operands[i]
                })
                .map_or_else(
                    || format!("length {} vs {}", s.instructions.len(), back.instructions.len()),
                    |i| {
                        format!(
                            "first diff at pc {i}: orig op {} ({}) vs ours op {} ({})",
                            s.instructions[i], s.int_operands[i],
                            back.instructions[i], back.int_operands[i],
                        )
                    },
                );
            failures.push((id, format!("bytes differ: {first_diff}")));
        }
    }

    println!("lifted cleanly: {lifted}/{}", scripts.len());
    println!("byte-exact recompile: {byte_exact}/{}", scripts.len());
    if !failures.is_empty() {
        println!("\nfailures:");
        for (id, msg) in &failures {
            println!("  script {id:>4} ({} ops): {msg}", scripts[id].instructions.len());
        }
    }
}
