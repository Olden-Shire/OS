//! `cargo run -p server` — boot the rev1 game server.
//!
//! Flags: `--addr HOST:PORT` (default 0.0.0.0:43594),
//! `--cache DIR` (default ./cache), `--scripts DIR` (the directory
//! holding server/script.{dat,idx}; optional).

fn main() -> std::io::Result<()> {
    server::install_crash_logger("server_crash.log");
    let args: Vec<String> = std::env::args().collect();
    let mut config = server::ServerConfig::default();

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--addr" if i + 1 < args.len() => {
                config.addr = args[i + 1].clone();
                i += 2;
            }
            "--cache" if i + 1 < args.len() => {
                config.cache_dir = args[i + 1].clone();
                i += 2;
            }
            "--scripts" if i + 1 < args.len() => {
                config.script_dir = Some(args[i + 1].clone());
                i += 2;
            }
            "--content" if i + 1 < args.len() => {
                config.content_dir = Some(args[i + 1].clone());
                i += 2;
            }
            "--baseline" if i + 1 < args.len() => {
                config.baseline_path = Some(args[i + 1].clone());
                i += 2;
            }
            "--worldid" if i + 1 < args.len() => {
                config.worldid = args[i + 1].parse().unwrap_or_else(|_| {
                    eprintln!("--worldid expects a number, got {}", args[i + 1]);
                    std::process::exit(2);
                });
                i += 2;
            }
            "--adv-host" if i + 1 < args.len() => {
                config.adv_host = args[i + 1].clone();
                i += 2;
            }
            other => {
                eprintln!("unknown flag: {other}");
                eprintln!(
                    "usage: server [--addr HOST:PORT] [--cache DIR] [--scripts DIR] \
                     [--content DIR] [--baseline FILE] [--worldid N] [--adv-host HOST]"
                );
                std::process::exit(2);
            }
        }
    }

    if let Err(e) = server::run(config) {
        server::append_crash_log("server_crash.log", &format!("\n==== SERVER EXITED ====\n{e}\n"));
        return Err(e);
    }
    Ok(())
}
