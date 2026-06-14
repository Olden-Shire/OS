# Build the wasm client bundle, host it over HTTP, and open the browser.
#
# The web client connects to the game server via WebSocket on the SAME TCP port
# it uses natively (40001) - so start the server first in another terminal:
#   .\run-server.ps1   (or .\start-server.ps1)
#
# Prereqs (checked up front): wasm32 target, wasm-bindgen-cli (version-matched to
# Cargo.lock), LLVM at C:\Program Files\LLVM, and Python 3 for the static host.
#
# Usage:
#   .\run-client-wasm.ps1                  # build bundle, host on :8787, open browser
#   .\run-client-wasm.ps1 -Port 9000       # different port
#   .\run-client-wasm.ps1 -NoBuild         # host the existing bundle (skip the wasm rebuild)
#   .\run-client-wasm.ps1 -BuildOnly       # build the bundle and exit (no host/browser)
param(
    [int]$Port = 8787,
    [switch]$NoBuild,
    [switch]$BuildOnly
)
. "$PSScriptRoot\_common.ps1"
Set-Location $PSScriptRoot

# In CI / non-interactive shells, hosting would block the job forever and there's
# no browser - degrade to a build-only run.
if ((Test-OSCi) -and -not $BuildOnly) {
    Write-Host "==> CI/non-interactive detected: building bundle only (no host/browser)." -ForegroundColor DarkGray
    $BuildOnly = $true
}

if (-not $NoBuild) {
    Assert-Tool cargo "Install Rust from https://rustup.rs"
    Assert-WasmTarget
    Assert-WasmBindgen
    Assert-Llvm
    Write-Host "==> building wasm bundle (crates\client\web\build.ps1)..." -ForegroundColor Cyan
    & "$PSScriptRoot\crates\client\web\build.ps1"
    if ($LASTEXITCODE -ne 0) { throw "wasm build failed (exit $LASTEXITCODE)" }
}

if ($BuildOnly) {
    Write-Host "==> bundle ready: crates\client\web\pkg" -ForegroundColor Green
    return
}

$webDir = Join-Path $PSScriptRoot "crates\client\web"
$url = "http://localhost:$Port/"

$py = Get-PythonLauncher
if (-not $py) {
    throw "Python not found (needed for the static file server). Install Python 3, or serve '$webDir' with any static HTTP server on port $Port. (Or use -BuildOnly to just build.)"
}

# Open the browser shortly after the server binds (background job, so the http
# server can hold the foreground - Ctrl+C stops it).
Start-Job -ArgumentList $url { param($u) Start-Sleep -Seconds 1; Start-Process $u } | Out-Null

Write-Host "==> hosting $webDir at $url  (Ctrl+C to stop)" -ForegroundColor Green
Set-Location $webDir
if ($py -eq "py") {
    & py -3 -m http.server $Port
} else {
    & $py -m http.server $Port
}
