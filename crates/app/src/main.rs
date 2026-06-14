//! CLI entry point. Subcommands available so far:
//!
//! * `unpack [--cache DIR] [--out DIR]`        — cache → Content-shaped tree (default `cache` → `Content`)
//! * `pack   [--in DIR]    [--out DIR]`        — Content-shaped tree → cache (default `Content` → `cache_repacked`)
//! * `import-names [--content DIR] [--from DIR]` — hash-match Content-old files into our
//!     Content tree, renaming any byte-identical files (default `Content` ← `Content-old`)
//! * `cs2asm FILE.cs2 [--content DIR]`         — compile structured cs2 source, print the
//!     assembly listing to stdout (backs the IntelliJ plugin's side-by-side asm preview)
//! * `cs2src FILE [--cache DIR] [--content DIR] [--id N]` — decompile raw cs2 bytecode to
//!     structured source on stdout (`.cs2asm` listing on stderr-warned fallback)

use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        usage(&args.first().map(String::as_str).unwrap_or("os"));
        return ExitCode::from(2);
    }
    let rest = &args[2..];
    let res = match args[1].as_str() {
        "unpack" => cmd_unpack(rest),
        "pack" => cmd_pack(rest),
        "import-names" => cmd_import_names(rest),
        "gen-baseline" => cmd_gen_baseline(rest),
        "verify" => cmd_verify(rest),
        "cs2asm" => cmd_cs2asm(rest),
        "cs2src" => cmd_cs2src(rest),
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

    // We are CRC-complete against the vanilla cache and verify that on every
    // server boot, so a re-unpack is never needed in normal operation — and it
    // would wipe curated pack names + renamed config files (`hans.npc`, the
    // `tutorial` varp, …) back to bare numeric stubs. Refuse to clobber a
    // populated tree unless the caller explicitly opts in with `--force`; unpack
    // elsewhere with `--out <dir>` for inspection. (The lib `unpack()` and tests
    // are unaffected — they target throwaway dirs.)
    let force = args.iter().any(|a| a == "--force");
    if !force && out_dir.join("pack").exists() {
        eprintln!(
            "refusing to unpack over the existing tree at {} — it carries curated pack \
             names / renamed config files that a re-unpack resets to numeric ids.\n  \
             pass --force to overwrite anyway, or --out <dir> to unpack somewhere else.",
            out_dir.display()
        );
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "unpack target already populated (use --force or --out)",
        ));
    }

    // Optional name source: when present, hash-match group names against this `pack/` dir
    // to recover real Jagex names for songs, etc. Defaults to `Content-old/` if it exists.
    let name_source = arg_path(args, "--names").or_else(|| {
        let default = PathBuf::from("Content-old");
        default.join("pack").exists().then_some(default)
    });

    let mut c = cache::Cache::open(&cache_dir)?;
    let keys_path = cache_dir.join("keys.json");
    let keys = if keys_path.exists() {
        cache::maps::XteaKeys::load(&keys_path)?
    } else {
        cache::maps::XteaKeys::default()
    };
    let name_map = if let Some(p) = name_source.as_ref() {
        cache::content::hash_names::load_from_pack_dir(&p.join("pack"))
    } else {
        cache::content::hash_names::ArchiveNameMap::new()
    };
    let stats = cache::content::unpack_with_names(&mut c, &keys, &out_dir, &name_map)?;
    let recovered: usize = name_map.values().map(std::collections::HashMap::len).sum();
    println!(
        "unpacked {} groups across 16 archives ({} master entries, {} payload bytes) to {} \
         [{} name candidates loaded from {}]",
        stats.total_groups,
        stats.master_entries,
        stats.total_payload_bytes,
        out_dir.display(),
        recovered,
        name_source.as_ref().map_or("<none>".to_string(), |p| p.display().to_string()),
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

fn cmd_import_names(args: &[String]) -> std::io::Result<()> {
    let content_dir = arg_path(args, "--content").unwrap_or_else(|| PathBuf::from("Content"));
    let old_dir = arg_path(args, "--from").unwrap_or_else(|| PathBuf::from("Content-old"));
    let stats = cache::content::import_names::import(&content_dir, &old_dir)?;
    let report = |label: &str, s: &cache::content::import_names::ScopeStats| {
        println!(
            "  {label:<8}  source {:>5}  scanned {:>5}  renamed {:>5}  already {:>4}  conflicts {:>3}",
            s.source_files, s.target_files, s.renamed, s.already_named, s.conflicts,
        );
    };
    println!("imported names from {} into {}:", old_dir.display(), content_dir.display());
    report("models",  &stats.models);
    report("songs",   &stats.songs);
    report("jingles", &stats.jingles);
    Ok(())
}

fn cmd_gen_baseline(args: &[String]) -> std::io::Result<()> {
    let cache_dir = arg_path(args, "--cache").unwrap_or_else(|| PathBuf::from("cache"));
    let out = arg_path(args, "--out").unwrap_or_else(|| cache_dir.join("crc_baseline.json"));
    let baseline = cache::verify::Baseline::generate(&cache_dir)?;
    baseline.save(&out)?;
    let groups: usize = baseline.groups.values().map(std::collections::BTreeMap::len).sum();
    println!(
        "wrote CRC baseline ({groups} groups, {} master entries) from {} to {}",
        baseline.master.len(),
        cache_dir.display(),
        out.display(),
    );
    Ok(())
}

fn cmd_verify(args: &[String]) -> std::io::Result<()> {
    let content_dir = arg_path(args, "--content").unwrap_or_else(|| PathBuf::from("Content"));
    let baseline_path =
        arg_path(args, "--baseline").unwrap_or_else(|| PathBuf::from("cache/crc_baseline.json"));
    let tmp = arg_path(args, "--out")
        .unwrap_or_else(|| std::env::temp_dir().join("os_verify_repack"));

    let baseline = cache::verify::Baseline::load(&baseline_path)?;
    let report = cache::verify::verify_repack(&content_dir, &baseline, &tmp)?;
    println!("{report}");
    if report.is_ok() {
        Ok(())
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "cache verification failed",
        ))
    }
}

/// Compile one structured `.cs2` source file and print its assembly listing — the
/// IntelliJ plugin runs this (debounced) to fill the side-by-side asm preview, so the
/// listing always comes from the same verified pipeline the packer uses. Parse and
/// compile errors go to stderr with a non-zero exit so the plugin can surface them.
fn cmd_cs2asm(args: &[String]) -> std::io::Result<()> {
    let file = arg_free(args)
        .ok_or_else(|| bad_input("usage: cs2asm FILE.cs2 [--content DIR]"))?;
    let content_dir = arg_path(args, "--content")
        .or_else(|| find_content_root(&file))
        .unwrap_or_else(|| PathBuf::from("Content"));

    let names = load_cs2_names(&content_dir)?;
    let sigs = cache::content::pack::scan_cs2_signatures(&content_dir, &names)?;

    let text = std::fs::read_to_string(&file)?;
    let ir = cache::cs2_source::parse(&text, &names, &sigs)
        .map_err(|e| bad_input(format!("{}: {e}", file.display())))?;
    let script = cache::cs2_compile::compile(&ir)
        .map_err(|e| bad_input(format!("{}: {e}", file.display())))?;
    print!("{}", cache::cs2_asm::disassemble(&script, &names));
    Ok(())
}

/// Decompile a raw cs2 bytecode file (e.g. a dumped archive-12 group) to structured
/// source on stdout. Callee signatures come from the cache's clientscript archive; if
/// the script can't be structured it falls back to the assembly listing (warned on
/// stderr) — same policy as unpack.
fn cmd_cs2src(args: &[String]) -> std::io::Result<()> {
    let file = arg_free(args)
        .ok_or_else(|| bad_input("usage: cs2src FILE [--cache DIR] [--content DIR] [--id N]"))?;
    let cache_dir = arg_path(args, "--cache").unwrap_or_else(|| PathBuf::from("cache"));
    let content_dir = arg_path(args, "--content").unwrap_or_else(|| PathBuf::from("Content"));

    let bytes = std::fs::read(&file)?;
    let script = cache::cs2::ClientScript::decode(&bytes)
        .ok_or_else(|| bad_input("not a valid cs2 script (too short / bad trailer)"))?;

    // Script id: --id flag, else a `script_<N>` / `<N>` file stem, else 0.
    let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let id: u32 = arg_path(args, "--id")
        .and_then(|p| p.to_str().and_then(|s| s.parse().ok()))
        .or_else(|| stem.strip_prefix("script_").and_then(|s| s.parse().ok()))
        .or_else(|| stem.parse().ok())
        .unwrap_or(0);

    // Signature table: every script in the cache's clientscript archive, plus this one
    // (it may be a custom script not present in the cache).
    let mut scripts = std::collections::BTreeMap::new();
    if let Ok(mut c) = cache::Cache::open(&cache_dir) {
        let gids: Vec<i32> = c.index(12).group_ids.clone();
        for gid in gids {
            let gid = gid as u32;
            if let Ok(Some(b)) = c.read_group(12, gid)
                && let Some(s) = cache::cs2::ClientScript::decode(&b)
            {
                scripts.insert(gid, s);
            }
        }
    }
    scripts.insert(id, script.clone());
    let sigs = cache::cs2_sig::analyze_all(&scripts).sigs;

    let names = load_cs2_names(&content_dir).unwrap_or_default();
    match cache::cs2_decompile::lift(id, &script, &sigs) {
        Ok(ir) => print!("{}", cache::cs2_source::print(&ir, &names)),
        Err(e) => {
            eprintln!("warning: cannot structure script {id} ({e}); emitting assembly");
            print!("{}", cache::cs2_asm::disassemble(&script, &names));
        }
    }
    Ok(())
}

/// Name tables for symbolic operands, from a Content tree's `pack/` dir. Missing pack
/// files just leave that scope numeric.
fn load_cs2_names(content_dir: &Path) -> std::io::Result<cache::cs2_asm::NameMaps> {
    let pack_dir = content_dir.join("pack");
    let mut names = cache::cs2_asm::NameMaps::new();
    for (scope, set) in [
        ("script", cache::cs2_asm::NameMaps::set_scripts as fn(&mut _, &_)),
        ("varp", cache::cs2_asm::NameMaps::set_varps),
        ("varbit", cache::cs2_asm::NameMaps::set_varbits),
    ] {
        let path = pack_dir.join(format!("{scope}.pack"));
        if path.exists() {
            set(&mut names, &cache::content::pack_file::read(&path)?);
        }
    }
    Ok(names)
}

/// Walk up from `file` to the Content root (the dir that owns a `pack/` subdir).
fn find_content_root(file: &Path) -> Option<PathBuf> {
    let mut dir = file.parent()?;
    loop {
        if dir.join("pack").is_dir() {
            return Some(dir.to_path_buf());
        }
        dir = dir.parent()?;
    }
}

fn bad_input(msg: impl Into<String>) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, msg.into())
}

/// First argument that isn't a `--flag` or a flag's value.
fn arg_free(args: &[String]) -> Option<PathBuf> {
    let mut i = 0;
    while i < args.len() {
        if args[i].starts_with("--") {
            i += 2;
        } else {
            return Some(PathBuf::from(&args[i]));
        }
    }
    None
}

fn arg_path(args: &[String], name: &str) -> Option<PathBuf> {
    args.windows(2).find(|w| w[0] == name).map(|w| PathBuf::from(&w[1]))
}

fn usage(prog: &str) {
    eprintln!(
        "usage:\n  \
         {prog} unpack       [--cache DIR] [--out  DIR] [--force]   (defaults: cache → Content; --force overwrites a populated tree)\n  \
         {prog} pack         [--in    DIR] [--out  DIR]   (defaults: Content → cache_repacked)\n  \
         {prog} import-names [--content DIR] [--from DIR]   (defaults: Content ← Content-old)\n  \
         {prog} gen-baseline [--cache DIR] [--out  FILE]  (default: cache → cache/crc_baseline.json)\n  \
         {prog} verify       [--content DIR] [--baseline FILE] [--out DIR]  (repack + CRC compare)\n  \
         {prog} cs2asm       FILE.cs2 [--content DIR]   (compile source, print asm listing)\n  \
         {prog} cs2src       FILE [--cache DIR] [--content DIR] [--id N]  (bytecode → source)"
    );
}
