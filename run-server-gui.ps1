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
# Ask for backtraces; the app appends any crash (panic or server-exit) to
# panel_crash.log itself (don't pipe a native GUI app's stderr through PS — on
# PS 5.1 that wraps each line as a NativeCommandError and can abort the app).
$env:RUST_BACKTRACE = "1"
Write-Host "==> crashes are logged to panel_crash.log" -ForegroundColor DarkGray
& $exe --addr $Addr --content $Content
