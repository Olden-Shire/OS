# RuneScript toolchain (OS1)

Kotlin/JVM compiler, decompiler, and IntelliJ plugin for OS1 RuneScript — both
**server scripts** (`.rs2`, compiled to the engine's `script.{dat,idx}` pack)
and **clientscripts** (`.cs2`, compiled to the cs2 bytecode the client reads
from cache archive 12). Faithful to the reference `RuneScriptTS-main/` (the
Lost City / Neptune compiler) and the cs2 format in `crates/cache/src/cs2*.rs`.

## Modules

- **`frontend/`** — shared lexer, parser, AST, and pack-backed symbol tables.
  No third-party deps so the IntelliJ plugin embeds it directly. One source of
  truth for parsing + symbol resolution across the CLI and the IDE.
- **`compiler/`** — codegen + writers. `CodeGenerator` is parameterized by an
  `OpcodeProfile` (server vs cs2). Server output → `BinaryWriter`/`PackWriter`;
  clientscript output → `clientscript/Cs2Writer`. `clientscript/Decompiler`
  turns cs2 bytecode back into round-trippable RuneScript.
- **`plugin/`** — IntelliJ platform plugin: highlighting, pack-backed
  completion/annotations, and Compile/Decompile actions for both script kinds.

## Metadata ("our pack")

The compiler and plugin read the same `.pack` (`id=name`) metadata:

- `data/symbols/command.pack` — server command table (generated from the
  engine's `crates/engine/src/script/opcode.rs`).
- `data/symbols/clientscript_command.pack` — cs2 command table (generated from
  `crates/cache/src/cs2_opcodes.rs`).
- `compiler/src/main/resources/cs2_opcodes.tsv` — cs2 opcode metadata (operand
  kind + stack arity) used by the decompiler.
- config name maps from `../Content/pack/*.pack` (interface, inv, seq, npc, …).

## Build / run

Gradle runs on JDK 21 (Java 25 on PATH breaks Gradle 8.10):

```sh
export JAVA_HOME="/c/Program Files/Java/jdk-21.0.10"   # Git Bash path form

# tests
./gradlew test

# compile the server source set -> ../data/pack
./gradlew :compiler:run --args="--src ../content/scripts --out ../data/pack \
    --commands data/symbols/command.pack --packs ../Content/pack"

# build the IntelliJ plugin (.zip in plugin/build/distributions) — first build
# downloads the ~1GB platform SDK
./gradlew :plugin:buildPlugin
```
