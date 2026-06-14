# Shared helpers for the OS launcher scripts. Dot-source it (`. "$PSScriptRoot\_common.ps1"`),
# don't run it directly. Keep it dependency-free and idempotent.

# Fail fast + quiet, deterministic output (CI logs hate progress spam).
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

# Non-interactive / CI detection - common providers + a fallback for any
# headless host. Used to skip browser launches and never block on a foreground
# server (which would hang a CI job forever).
$script:OS_CI = [bool](
    $env:CI -or $env:GITHUB_ACTIONS -or $env:TF_BUILD -or $env:GITLAB_CI -or
    $env:JENKINS_URL -or $env:BUILDKITE -or (-not [Environment]::UserInteractive)
)
function Test-OSCi { return $script:OS_CI }

# Assert a command exists on PATH, with an install hint, else throw (so CI fails).
function Assert-Tool {
    param([Parameter(Mandatory)][string]$Name, [string]$Hint)
    if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) {
        $m = "Required tool '$Name' was not found on PATH."
        if ($Hint) { $m += "  $Hint" }
        throw $m
    }
}

# Run a native exe and throw on a non-zero exit (PowerShell does NOT do this by
# default - $ErrorActionPreference only governs cmdlets, so native failures slip
# through and break CI silently).
function Invoke-Checked {
    param([Parameter(Mandatory)][string]$File, [string[]]$Arguments = @())
    & $File @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "'$File $($Arguments -join ' ')' exited with code $LASTEXITCODE"
    }
}

# Mirror the server's find_jdk21(): JAVA_HOME whose leaf contains 'jdk-21', else
# the newest C:\Program Files\Java\jdk-21*. Returns the path or $null. (This is
# exactly what the running server probes, so it predicts whether scripts compile.)
function Get-Jdk21Home {
    # CI (actions/setup-java) exposes JAVA_HOME_<ver>_<arch> for each installed JDK.
    foreach ($v in @($env:JAVA_HOME_21_X64, $env:JAVA_HOME_21_x64, $env:JAVA_HOME_21_ARM64)) {
        if ($v -and (Test-Path "$v\bin\java.exe")) { return $v }
    }
    # JAVA_HOME if it really is a 21 JDK. The path leaf varies by distro (e.g.
    # `...\x64`), so confirm via the `release` file, not just the folder name.
    if ($env:JAVA_HOME -and (Test-Path "$env:JAVA_HOME\bin\java.exe")) {
        $rel = Join-Path $env:JAVA_HOME 'release'
        if (((Split-Path $env:JAVA_HOME -Leaf) -like '*jdk-21*') -or
            ((Test-Path $rel) -and (Select-String -Path $rel -SimpleMatch 'JAVA_VERSION="21' -Quiet))) {
            return $env:JAVA_HOME
        }
    }
    $javaDir = 'C:\Program Files\Java'
    if (Test-Path $javaDir) {
        $cand = Get-ChildItem -Path $javaDir -Directory -ErrorAction SilentlyContinue |
            Where-Object { $_.Name -like 'jdk-21*' } | Sort-Object Name | Select-Object -Last 1
        if ($cand) { return $cand.FullName }
    }
    return $null
}

# Report JDK 21 status. The server compiles Content\scripts into data\pack on
# boot and needs JDK 21 to do it. data\pack is NOT committed (it's generated), so
# on a fresh clone the server can't build its script bundle without JDK 21. It's
# only non-fatal when an up-to-date local data\pack already exists from a prior
# build, so this stays a warning rather than a hard stop.
function Show-Jdk21Status {
    $jdk = Get-Jdk21Home
    if ($jdk) {
        Write-Host "    JDK 21: $jdk" -ForegroundColor DarkGray
    } else {
        Write-Warning ("JDK 21 not found (JAVA_HOME ending 'jdk-21*' or C:\Program Files\Java\jdk-21*). " +
            "The server compiles Content\scripts on boot and needs JDK 21; data\pack is generated, not " +
            "committed, so a fresh clone has no bundle to fall back to. Install JDK 21 (a newer default " +
            "JDK on PATH can also break the Gradle build).")
    }
}

# Look up a crate's locked version from Cargo.lock (for the wasm-bindgen check).
function Get-LockedCrateVersion {
    param([Parameter(Mandatory)][string]$Name)
    $lock = Join-Path (Get-Location) 'Cargo.lock'
    if (-not (Test-Path $lock)) { return $null }
    $lines = Get-Content $lock
    for ($i = 0; $i -lt $lines.Count; $i++) {
        if ($lines[$i].Trim() -eq "name = `"$Name`"") {
            for ($j = $i + 1; $j -lt [Math]::Min($i + 4, $lines.Count); $j++) {
                if ($lines[$j] -match '^\s*version = "(.+)"') { return $Matches[1] }
            }
        }
    }
    return $null
}

# --- wasm client prerequisites -------------------------------------------------

function Assert-WasmTarget {
    Assert-Tool rustup "Install Rust from https://rustup.rs"
    $installed = & rustup target list --installed
    if ($installed -notcontains 'wasm32-unknown-unknown') {
        Write-Host "    adding rustup target wasm32-unknown-unknown..." -ForegroundColor DarkYellow
        Invoke-Checked rustup @('target', 'add', 'wasm32-unknown-unknown')
    }
}

function Assert-WasmBindgen {
    Assert-Tool wasm-bindgen "Install: cargo install wasm-bindgen-cli --version <Cargo.lock version>"
    $out = (& wasm-bindgen --version | Out-String)
    $have = if ($out -match '(\d+\.\d+\.\d+)') { $Matches[1] } else { $null }
    $want = Get-LockedCrateVersion 'wasm-bindgen'
    if ($want -and $have -and ($want -ne $have)) {
        throw ("wasm-bindgen CLI is $have but Cargo.lock pins $want - the bundle will be subtly broken. " +
            "Fix: cargo install -f wasm-bindgen-cli --version $want")
    }
    Write-Host "    wasm-bindgen: $have$(if ($want) { " (lock $want)" })" -ForegroundColor DarkGray
}

function Assert-Llvm {
    if (-not (Test-Path 'C:\Program Files\LLVM\bin\clang.exe')) {
        throw "LLVM/clang not found at C:\Program Files\LLVM (imgui-sys compiles C++ for wasm). Install: winget install LLVM.LLVM"
    }
}

# Find a Python launcher for the static file server; $null if none.
function Get-PythonLauncher {
    foreach ($c in 'py', 'python', 'python3') {
        if (Get-Command $c -ErrorAction SilentlyContinue) { return $c }
    }
    return $null
}
