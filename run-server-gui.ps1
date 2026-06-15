# Build (release) and launch the OS server + god-view control panel (panel.exe).
#
# The panel hosts the authoritative game server AND the egui control panel in one
# process. Clients (desktop / web) connect to it on the game port. For a headless
# server (CI / remote), use .\run-server.ps1 instead.
#
# The server compiles RuneScript (Content\scripts) into data\pack on boot, which
# needs JDK 21. data\pack is generated (gitignored), so a fresh clone requires
# JDK 21 for the first boot; afterwards an unchanged local bundle is reused.
#
# Usage:
#   .\run-server-gui.ps1                       # build, then serve 0.0.0.0:40001 from .\Content
#   .\run-server-gui.ps1 -NoBuild              # skip cargo build (fast restart)
#   .\run-server-gui.ps1 -Addr 0.0.0.0:40001 -Content .\Content
param(
    [string]$Addr = "0.0.0.0:40001",
    [string]$Content = "Content",
    [switch]$NoBuild
)
. "$PSScriptRoot\_common.ps1"
Set-Location $PSScriptRoot

Assert-Tool cargo "Install Rust from https://rustup.rs"
Show-Jdk21Status

if (-not $NoBuild) {
    Write-Host "==> building panel (release)..." -ForegroundColor Cyan
    Invoke-Checked cargo @('build', '--release', '-p', 'panel')
}

$exe = Join-Path $PSScriptRoot "target\release\panel.exe"
if (-not (Test-Path $exe)) { throw "panel.exe not found at $exe (build it without -NoBuild)." }

Write-Host "==> starting server+panel: panel.exe --addr $Addr --content $Content" -ForegroundColor Green
# Capture the full run to a log + ask for backtraces, so a crash leaves a trace.
# A panic (incl. the background server thread) is also appended to panel_crash.log.
$env:RUST_BACKTRACE = "1"
$log = Join-Path $PSScriptRoot "server-gui.log"
Write-Host "==> logging to $log (crashes also in panel_crash.log)" -ForegroundColor DarkGray
& $exe --addr $Addr --content $Content 2>&1 | Tee-Object -FilePath $log
