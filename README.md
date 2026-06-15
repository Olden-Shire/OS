# OS — RuneScape rev1 (2007) reimplementation

A from-scratch, byte-faithful reimplementation of the 2007-era ("rev1") RuneScape
**client, server, and cache toolchain** in Rust — built alongside the original,
deobfuscated Java client (`src/`), which serves as the byte-level ground truth.

> Inspired by [**Lost City**](https://github.com/LostCityRS) — its open-source
> engine and RuneScript content are the framework and op-semantics reference this
> project's server side mirrors. Huge thanks to that project and community.

The Rust client runs on the **desktop** and in the **browser** (WebAssembly +
WebGL2). The server speaks the real rev1 protocol and serves the unmodified 2007
cache. A Kotlin RuneScript compiler and an IntelliJ plugin round out the content
tooling.

```
Java client (src/, ground truth)  ──ports──>  Rust crates/  ──> desktop + wasm client, server
                                                   │
        2007 cache (cache/) ──unpack──> Content/ ──repack (CRC-identical)──> served by the server
                                                   │
                            RuneScript (.rs2) ──runescript/ compiler──> server script pack
```

## Layout

```
crates/
  io          Packet / bit IO, ISAAC, RSA, BZip2/GZip, Jagfile
  datastruct  intrusive data structures (linked lists, hash tables)
  wordenc     chat word filter + Huffman compression
  pix         software renderer (port of graphics + dash3d)
  cache       cache read/write, config codecs, CS2 decompiler, map .jm2, Content unpack/pack
  db          persistence — player saves, friends, accounts
  protocol    rev1 server↔client packets
  engine      world tick, entities (Player/Npc), RuneScript runtime
  server      the game server — TCP, login, JS5, world loop
  client      rev1 client — native desktop + wasm/WebGL2 browser
  synth       software synth (Vorbis/MIDI)
  app         cache CLI — unpack / pack / verify / cs2
  jaged       cache / map / interface editor (egui/wgpu)
  panel       server control panel + live world map
src/          deobfuscated rev1 Java client — byte-level ground truth (reference only)
runescript/   Kotlin RuneScript compiler (.rs2 → script pack) + IntelliJ plugin
cache/        vanilla 2007 cache (served)
Content/      editable tree; repacks CRC-identical to the cache
```

See `HOSTING.md` to expose the wasm client (GitHub Pages) at your server via Cloudflare Tunnel.

## Build & run

`build.ps1` builds everything so nothing drifts: the Rust binaries, the wasm
bundle, the Java client, and the IntelliJ plugin.

```powershell
.\build.ps1 -Release         # release Rust + wasm + Java + plugin
.\build.ps1 -SkipWasm        # skip the wasm bundle  (also -SkipJava / -SkipPlugin)
```

Helper launchers:

```powershell
.\run-server.ps1            # headless game server (0.0.0.0:40001)
.\run-server-gui.ps1        # server + control panel (panel)
.\run-client-rust.ps1       # native Rust client
.\run-client-wasm.ps1       # serve the browser client locally
.\run-client-java.ps1       # original Java client (reference)
.\run-jaged.ps1             # cache editor
```

Cache tooling:

```powershell
cargo run --release -p app -- verify          # repack Content, CRC-check vs vanilla
cargo run --release -p app -- unpack --out X  # cache -> editable tree (keeps Content safe)
```

The server packs `Content/` → a cache and **verifies it is CRC-identical to the
vanilla baseline on every boot**, refusing to start on a mismatch — so content
edits can never silently corrupt the served cache.

## Contributor notes (Java client, `src/`)

The Java client is deobfuscated, not decompiled-and-rewritten — keep it faithful:

- Port comments carry the original `@ObfuscatedName`, not deob member names, so the
  code can be diffed against future revision gamepacks.
- Don't move fields or methods — everything is in its original place.
- The deob may over-split locals (`int b = a + 1;`); simplify to `a++` where it
  reads better, but keep deliberate separate declarations.
- Exception messages were stripped (`new RuntimeException("")`); restore from other
  references where known.
- Prefer official naming; any accurate name beats none.
- Document liberally — people after us will want to understand the engine.
