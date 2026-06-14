# Running OS

Launcher scripts (PowerShell, run from the repo root). Each builds in `--release`
by default, then runs; pass `-NoBuild` to skip the build for a fast restart.
They share `_common.ps1` (tool/JDK/CI checks) - don't run that one directly.

| Script | What it starts |
| --- | --- |
| `.\run-server.ps1`      | **Headless** game server (`server.exe`), serving `0.0.0.0:40001` from `.\Content`. Default for CI / remote. |
| `.\run-server-gui.ps1`  | Game server **+ god-view control panel** (`panel.exe`). Same port. |
| `.\run-client-rust.ps1`| Native **Rust** desktop client, auto-connecting to `127.0.0.1:40001`. |
| `.\run-client-java.ps1`| Reference **Java** client via root Gradle (`gradlew run`), connecting to `127.0.0.1:40001`. |
| `.\run-client-wasm.ps1`         | Builds the wasm bundle, hosts it on `http://localhost:8787`, opens the browser. |
| `.\run-jaged.ps1`       | `jaged` - the Content/cache inspector + pack-name editor (opens `.\Content`). |
| `.\build.ps1`           | Builds all client targets: Rust exe + wasm bundle + IntelliJ plugin. |
| `.\clean.ps1`           | Removes regenerated artifacts (target, wasm bundle, temp cache, logs). Keeps Content/cache/data. |

## Typical flow

```powershell
.\run-server.ps1            # terminal 1 - headless   (or .\run-server-gui.ps1 for the panel)
.\run-client-rust.ps1      # terminal 2   (or .\run-client-java.ps1, or .\run-client-wasm.ps1 for the browser)
```

All clients talk to the running server, so start it first. `run-client-wasm.ps1` connects
via WebSocket to the same port (40001) the desktop clients use over TCP.

## Prerequisites (fresh pull)

- **Rust** (stable) - `rustup`. Everything builds with `cargo`.
- **JDK 21** (server only) - the server compiles RuneScript (`Content\scripts`)
  into `data\pack` on boot. `data\pack` is **generated, not committed** (gitignored),
  so a **fresh clone needs JDK 21** to build the script bundle on its first boot;
  after that, an unchanged local bundle is reused. The scripts detect JDK 21 the
  same way the server does (`JAVA_HOME` ending `jdk-21*`, or
  `C:\Program Files\Java\jdk-21*`) and **warn** if it's missing. A newer default
  JDK on `PATH` can break the Gradle build, so prefer pinning `JAVA_HOME`.
- **Web client only:**
  - `wasm32-unknown-unknown` target (`run-client-wasm.ps1` auto-adds it if missing)
  - `wasm-bindgen-cli`, **version-matched to `Cargo.lock`** (`run-client-wasm.ps1` checks
    and tells you the exact `cargo install -f wasm-bindgen-cli --version X.Y.Z`)
  - **LLVM/clang** (`winget install LLVM.LLVM`) at `C:\Program Files\LLVM`
  - **Python 3** for the static file server

## CI / non-interactive

The scripts are CI-aware (detect `CI`, `GITHUB_ACTIONS`, `TF_BUILD`, etc.):

- Missing tools fail fast with an install hint; native build failures propagate a
  non-zero exit code (so a CI step actually fails).
- `run-client-wasm.ps1` in CI builds the bundle and exits instead of starting a blocking
  HTTP server / opening a browser. Force it anywhere with `-BuildOnly`.

## Notes

- First build is slow (whole workspace, release). Subsequent runs are fast; use
  `-NoBuild` to skip building entirely.
- Override the server bind/content: `.\run-server.ps1 -Addr 0.0.0.0:40001 -Content .\Content`.
- Override the web port: `.\run-client-wasm.ps1 -Port 9000`.
- Open a different cache in jaged: `.\run-jaged.ps1 .\cache`.
- `.\clean.ps1 -KeepTarget` skips the slow cargo rebuild; `-Screens` also deletes
  root `*.png`/`*.jpg` debug captures.
