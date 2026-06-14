//! Rename an interface symbol across the Content tree — the on-disk side of
//! an IDE/jaged rename refactor. Edits interface.pack, the `.if` filename
//! (hybrid `{id}_{name}.if`), the `.if` component section header, and the
//! `.rs2`/`.cs2` references — all CRC-neutral (names never reach the cache).
//!
//! Usage:
//!   cargo run -p cache --example rename_interface -- <id> <new_name>
//!   cargo run -p cache --example rename_interface -- <parent>:<child> <new_name>
//!
//! Examples:
//!   rename_interface 549 welcome              # if_549 -> welcome
//!   rename_interface 549:2 welcome_top        # if_549:com_2 -> welcome:welcome_top
//!
//! Operates on ./Content (override with $OS_CONTENT). Reversible: run again
//! with the original name.

use std::path::PathBuf;

use cache::content::rename;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        eprintln!("usage: rename_interface <id|parent:child> <new_name>");
        std::process::exit(2);
    }
    let target = &args[0];
    let new_name = &args[1];
    let content = PathBuf::from(std::env::var("OS_CONTENT").unwrap_or_else(|_| "Content".into()));

    let result = if let Some((p, c)) = target.split_once(':') {
        let parent: u32 = p.parse().expect("parent id");
        let child: u32 = c.parse().expect("child index");
        rename::rename_component(&content, parent, child, new_name)
    } else {
        let id: u32 = target.parse().expect("interface id");
        rename::rename_interface(&content, id, new_name)
    };

    match result {
        Ok(report) => {
            println!("renamed {target} -> {new_name}");
            println!("  interface.pack lines: {}", report.pack_lines_changed);
            if let Some((old, new)) = &report.file_renamed {
                println!("  file: {old} -> {new}");
            }
            if report.if_header_changed {
                println!("  .if component section header updated");
            }
            println!("  source references (.rs2/.cs2): {}", report.refs_changed);
        }
        Err(e) => {
            eprintln!("rename failed: {e}");
            std::process::exit(1);
        }
    }
}
