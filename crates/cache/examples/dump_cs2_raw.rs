//! Dump one clientscript's raw decompressed bytecode to a file (for testing
//! `app cs2src` and other bytecode-consuming tools).
//!
//! Usage: `cargo run --example dump_cs2_raw -p cache -- <script_id> <out_file>`

use std::path::Path;

use cache::Cache;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let id: u32 = args.first().and_then(|s| s.parse().ok()).expect("script id");
    let out = args.get(1).expect("output path");

    let mut cache = Cache::open(Path::new("cache")).expect("open cache/");
    let bytes = cache.read_group(12, id).expect("io").expect("group present");
    std::fs::write(out, &bytes).expect("write");
    println!("wrote {} bytes to {out}", bytes.len());
}
