//! Pretty-print sample CS2 scripts to stdout — same printer the editor uses, so the
//! output mirrors what you'll see in jaged's CS2 view.
//!
//! Usage: `cargo run --example dump_cs2 -p cache -- [script_id [script_id ...]]`.
//! Default sample: 1, 35, 100, 130 — a mix of small and medium scripts.

use std::path::Path;

use cache::cs2::ClientScript;
use cache::cs2_pretty::{compute_labels, pretty, Line};
use cache::Cache;

const CLIENTSCRIPTS_ARCHIVE: u8 = 12;

fn main() {
    let ids: Vec<u32> = std::env::args()
        .skip(1)
        .filter_map(|s| s.parse().ok())
        .collect();
    let ids = if ids.is_empty() { vec![1, 35, 100, 130] } else { ids };

    let mut cache = Cache::open(Path::new("cache")).expect("open cache/");
    for id in ids {
        println!("=== script {id} ===");
        match cache.read_group(CLIENTSCRIPTS_ARCHIVE, id) {
            Ok(Some(bytes)) => match ClientScript::decode(&bytes) {
                Some(s) => print_listing(id, &s),
                None => println!("(decode failed: buffer too short, {} bytes)", bytes.len()),
            },
            Ok(None) => println!("(missing)"),
            Err(e) => println!("(read error: {e})"),
        }
        println!();
    }
}

fn print_listing(id: u32, s: &ClientScript) {
    println!(
        "// id={id} name={:?} ops={} int_locals={} str_locals={} int_args={} str_args={}",
        s.name,
        s.instructions.len(),
        s.int_local_count,
        s.string_local_count,
        s.int_arg_count,
        s.string_arg_count,
    );
    let labels = compute_labels(s);
    let lines = pretty(s, &labels);
    for line in &lines {
        let primary = line.addrs[0];
        if let Some(n) = labels.get(&primary) {
            println!("label_{n:02}:");
        }
        let addrs = if line.addrs.len() == 1 {
            format!("{:04}     ", line.addrs[0])
        } else {
            format!("{:04}-{:04}", line.addrs[0], line.addrs[line.addrs.len() - 1])
        };
        let stack = format_stack(line);
        println!(
            "  {addrs}  {:<14} {:<32}  {stack}",
            line.mnemonic, line.operand
        );
    }
}

fn format_stack(line: &Line) -> String {
    let i = line.int_depth.map_or_else(|| "?".to_string(), |d| d.to_string());
    let s = line.str_depth.map_or_else(|| "?".to_string(), |d| d.to_string());
    format!("[i:{i} s:{s}]")
}
