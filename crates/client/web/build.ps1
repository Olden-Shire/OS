# Builds the wasm client bundle into crates/client/web/pkg.
#
# Requirements:
#   rustup target add wasm32-unknown-unknown
#   cargo install wasm-bindgen-cli   (version must match Cargo.lock's wasm-bindgen)
#   LLVM (clang) — winget install LLVM.LLVM — for imgui-sys's C++; it compiles
#   against the freestanding headers in crates/client/wasm-libc/include, with
#   symbols provided by crates/client/src/wasm_libc.rs.
#
# Serve crates/client/web/ over any static HTTP server; run the game server
# normally (it accepts WebSocket upgrades on its TCP port).

$ErrorActionPreference = "Stop"
$root = Resolve-Path "$PSScriptRoot\..\..\.."
$include = "$root\crates\client\wasm-libc\include" -replace '\\', '/'
$llvm = "C:\Program Files\LLVM\bin"

$env:CC_wasm32_unknown_unknown = "$llvm\clang.exe"
$env:CXX_wasm32_unknown_unknown = "$llvm\clang++.exe"
$env:AR_wasm32_unknown_unknown = "$llvm\llvm-ar.exe"
$env:CXXFLAGS_wasm32_unknown_unknown = "-isystem $include -DIMGUI_USE_STB_SPRINTF -DIMGUI_DISABLE_FILE_FUNCTIONS"
# No C++ runtime on wasm32-unknown-unknown (imgui builds -fno-exceptions/-fno-rtti).
$env:CXXSTDLIB_wasm32_unknown_unknown = ""

Set-Location $root
cargo build -p client --release --target wasm32-unknown-unknown
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
wasm-bindgen target/wasm32-unknown-unknown/release/client.wasm `
    --target web --no-typescript --out-dir crates/client/web/pkg
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
Write-Host "bundle ready: crates/client/web/pkg"
