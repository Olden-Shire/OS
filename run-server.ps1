# Build (release) and launch the HEADLESS OS game server (server.exe) - no
# god-view panel/GUI. Same wire protocol as run-server-gui.ps1 (the panel
# variant); use this for CI, remote hosts, or when you just want the server.
#
# The server compiles RuneScript (Content\scripts) into data\pack on boot, which
# needs JDK 21. data\pack is generated (gitignored), so a fresh clone requires
# JDK 21 for the first boot; afterwards an unchanged local bundle is reused.
#
# Usage:
#   .\run-server.ps1                       # build, serve 0.0.0.0:40001 from .\Content
#   .\run-server.ps1 -NoBuild              # skip cargo build (fast restart)
#   .\run-server.ps1 -Addr 0.0.0.0:40001 -Content .\Content
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
    Write-Host "==> building server (release)..." -ForegroundColor Cyan
    Invoke-Checked cargo @('build', '--release', '-p', 'server')
}

$exe = Join-Path $PSScriptRoot "target\release\server.exe"
if (-not (Test-Path $exe)) { throw "server.exe not found at $exe (build it without -NoBuild)." }

Write-Host "==> starting headless server: server.exe --addr $Addr --content $Content" -ForegroundColor Green
# Ask for backtraces; the server appends any crash to server_crash.log itself.
$env:RUST_BACKTRACE = "1"
Write-Host "==> crashes are logged to server_crash.log" -ForegroundColor DarkGray
& $exe --addr $Addr --content $Content
