# Build (release) and launch the native RUST desktop game client (client.exe).
# (For the reference Java client, use .\run-client-java.ps1.)
#
# Connects to the local server at 127.0.0.1:40001 (dev default). Start the server
# first in another terminal:  .\run-server.ps1  (headless) or .\run-server-gui.ps1 (panel)
#
# Usage:
#   .\run-client-rust.ps1            # build, then launch
#   .\run-client-rust.ps1 -NoBuild   # skip cargo build (fast relaunch)
param(
    [switch]$NoBuild
)
. "$PSScriptRoot\_common.ps1"
Set-Location $PSScriptRoot

Assert-Tool cargo "Install Rust from https://rustup.rs"

if (-not $NoBuild) {
    Write-Host "==> building client (release)..." -ForegroundColor Cyan
    Invoke-Checked cargo @('build', '--release', '-p', 'client')
}

$exe = Join-Path $PSScriptRoot "target\release\client.exe"
if (-not (Test-Path $exe)) { throw "client.exe not found at $exe (build it without -NoBuild)." }

Write-Host "==> launching desktop client (connecting to 127.0.0.1:40001)..." -ForegroundColor Green
& $exe
