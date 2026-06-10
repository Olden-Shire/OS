//! `cargo run -p server` — boot the rev1 game server.
//!
//! Flags: `--addr HOST:PORT` (default 0.0.0.0:43594),
//! `--cache DIR` (default ./cache), `--scripts DIR` (the directory
//! holding server/script.{dat,idx}; optional).

fn main() -> std::io::Result<()> {
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
            other => {
                eprintln!("unknown flag: {other}");
                eprintln!("usage: server [--addr HOST:PORT] [--cache DIR] [--scripts DIR]");
                std::process::exit(2);
            }
        }
    }

    server::run(config)
}
