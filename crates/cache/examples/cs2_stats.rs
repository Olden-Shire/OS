//! One-off survey for decompiler planning: scan every archive-12 script and report
//! opcode usage, arity-table coverage (`stack_delta`), and basic structural facts.
//!
//! Usage: `cargo run --example cs2_stats -p cache`

use std::collections::BTreeMap;
use std::path::Path;

use cache::cs2::ClientScript;
use cache::cs2_opcodes::{opcode_name, stack_delta};
use cache::Cache;

const CLIENTSCRIPTS_ARCHIVE: u8 = 12;

fn main() {
    let mut cache = Cache::open(Path::new("cache")).expect("open cache/");
    let ids: Vec<u32> = cache
        .index(CLIENTSCRIPTS_ARCHIVE)
        .group_ids
        .iter()
        .map(|&g| g as u32)
        .collect();

    let mut script_count = 0usize;
    let mut total_ops = 0usize;
    let mut histogram: BTreeMap<u16, usize> = BTreeMap::new();
    let mut max_len = (0usize, 0u32);
    let mut named = 0usize;

    for id in ids {
        let Ok(Some(bytes)) = cache.read_group(CLIENTSCRIPTS_ARCHIVE, id) else {
            continue;
        };
        let Some(s) = ClientScript::decode(&bytes) else {
            println!("script {id}: decode FAILED ({} bytes)", bytes.len());
            continue;
        };
        script_count += 1;
        if s.name.is_some() {
            named += 1;
        }
        total_ops += s.instructions.len();
        if s.instructions.len() > max_len.0 {
            max_len = (s.instructions.len(), id);
        }
        for &op in &s.instructions {
            *histogram.entry(op).or_default() += 1;
        }
    }

    println!("scripts: {script_count} (named: {named})");
    println!("total instructions: {total_ops}");
    println!("largest script: id {} with {} ops", max_len.1, max_len.0);
    println!("distinct opcodes used: {}", histogram.len());
    println!();

    let mut unknown_delta: Vec<(u16, usize)> = Vec::new();
    let mut unnamed_ops: Vec<(u16, usize)> = Vec::new();
    for (&op, &count) in &histogram {
        if stack_delta(op).is_none() {
            unknown_delta.push((op, count));
        }
        if opcode_name(op).is_none() {
            unnamed_ops.push((op, count));
        }
    }

    println!(
        "opcodes used but with stack_delta = None: {} (of {})",
        unknown_delta.len(),
        histogram.len()
    );
    for (op, count) in &unknown_delta {
        println!(
            "  op {op:>5}  x{count:<6} {}",
            opcode_name(*op).unwrap_or("<unnamed>")
        );
    }
    println!();
    println!("opcodes used but unnamed in our table: {}", unnamed_ops.len());
    for (op, count) in &unnamed_ops {
        println!("  op {op:>5}  x{count}");
    }
}
