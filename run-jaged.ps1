# Build (release) and launch jaged - the Content/cache inspector + pack-name editor (GUI).
#
# Reads from .\Content by default (the source of truth); pass a path to open any
# cache or Content directory.
#
# Usage:
#   .\run-jaged.ps1                 # build, then open .\Content
#   .\run-jaged.ps1 .\cache         # open a different cache/Content dir
#   .\run-jaged.ps1 -NoBuild        # skip cargo build (fast relaunch)
param(
    [string]$Path = "Content",
    [switch]$NoBuild
)
. "$PSScriptRoot\_common.ps1"
Set-Location $PSScriptRoot

Assert-Tool cargo "Install Rust from https://rustup.rs"

if (-not $NoBuild) {
    Write-Host "==> building jaged (release)..." -ForegroundColor Cyan
    Invoke-Checked cargo @('build', '--release', '-p', 'jaged')
}

$exe = Join-Path $PSScriptRoot "target\release\jaged.exe"
if (-not (Test-Path $exe)) { throw "jaged.exe not found at $exe (build it without -NoBuild)." }

Write-Host "==> launching jaged ($Path)..." -ForegroundColor Green
& $exe $Path
