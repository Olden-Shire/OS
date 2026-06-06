//! CLI entry point. Subcommands available so far:
//!
//! * `unpack [--cache DIR] [--out DIR]`   — cache → Content-shaped tree (default `cache` → `Content`)
//! * `pack   [--in DIR]    [--out DIR]`   — Content-shaped tree → cache (default `Content` → `cache_repacked`)

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        usage(&args.first().map(String::as_str).unwrap_or("os1"));
        return ExitCode::from(2);
    }
    let rest = &args[2..];
    let res = match args[1].as_str() {
        "unpack" => cmd_unpack(rest),
        "pack" => cmd_pack(rest),
        "help" | "-h" | "--help" => {
            usage(&args[0]);
            return ExitCode::SUCCESS;
        }
        cmd => {
            eprintln!("unknown command: {cmd}");
            usage(&args[0]);
            return ExitCode::from(2);
        }
    };
    match res {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}

fn cmd_unpack(args: &[String]) -> std::io::Result<()> {
    let cache_dir = arg_path(args, "--cache").unwrap_or_else(|| PathBuf::from("cache"));
    let out_dir = arg_path(args, "--out").unwrap_or_else(|| PathBuf::from("Content"));

    let mut c = cache::Cache::open(&cache_dir)?;
    let keys_path = cache_dir.join("keys.json");
    let keys = if keys_path.exists() {
        cache::maps::XteaKeys::load(&keys_path)?
    } else {
        cache::maps::XteaKeys::default()
    };
    let stats = cache::content::unpack(&mut c, &keys, &out_dir)?;
    println!(
        "unpacked {} groups across 16 archives ({} master entries, {} payload bytes) to {}",
        stats.total_groups,
        stats.master_entries,
        stats.total_payload_bytes,
        out_dir.display(),
    );
    Ok(())
}

fn cmd_pack(args: &[String]) -> std::io::Result<()> {
    let in_dir = arg_path(args, "--in").unwrap_or_else(|| PathBuf::from("Content"));
    let out_dir = arg_path(args, "--out").unwrap_or_else(|| PathBuf::from("cache_repacked"));
    let stats = cache::content::pack(&in_dir, &out_dir)?;
    println!(
        "packed {} groups ({} master entries, {} group bytes) to {}",
        stats.total_groups,
        stats.master_entries,
        stats.total_bytes,
        out_dir.display(),
    );
    Ok(())
}

fn arg_path(args: &[String], name: &str) -> Option<PathBuf> {
    args.windows(2).find(|w| w[0] == name).map(|w| PathBuf::from(&w[1]))
}

fn usage(prog: &str) {
    eprintln!(
        "usage:\n  \
         {prog} unpack [--cache DIR] [--out DIR]   (defaults: cache → Content)\n  \
         {prog} pack   [--in DIR]    [--out DIR]   (defaults: Content → cache_repacked)"
    );
}
