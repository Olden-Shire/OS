# Builds EVERYTHING so nothing drifts behind the source:
#   1. Rust binaries (cargo): client (rust desktop), server (headless), panel (server+gui), jaged
#   2. Rust wasm client bundle    -> crates/client/web/pkg            (always release)
#   3. RuneScript server content  -> data/pack/server                 (Kotlin compiler, needs JDK 21)
#   4. Java client (root Gradle)  -> build/libs                       (needs a JDK)
#   5. IntelliJ RuneScript plugin -> runescript/plugin/build/distributions (needs JDK 21)
#
#   .\build.ps1              # debug rust + wasm + scripts + java + plugin
#   .\build.ps1 -Release     # release rust + wasm + scripts + java + plugin
#   .\build.ps1 -SkipWasm    # skip the wasm bundle
#   .\build.ps1 -SkipScripts # skip the RuneScript content compile
#   .\build.ps1 -SkipJava    # skip the Java client
#   .\build.ps1 -SkipPlugin  # skip the IntelliJ plugin (faster iteration)
#
# NOTE: ASCII only in this file. PowerShell 5.1 reads BOM-less UTF-8
# as ANSI, and a UTF-8 em-dash inside a string garbles into a curly
# quote that breaks parsing.
param(
    [switch]$Release,
    [switch]$SkipWasm,
    [switch]$SkipScripts,
    [switch]$SkipJava,
    [switch]$SkipPlugin
)
. "$PSScriptRoot\_common.ps1"
Set-Location $PSScriptRoot

Assert-Tool cargo "Install Rust from https://rustup.rs"

# 1. Rust workspace binaries: client (rust desktop), server (headless), panel (server+gui), jaged.
$cargoArgs = @('build', '-p', 'client', '-p', 'server', '-p', 'panel', '-p', 'jaged')
if ($Release) { $cargoArgs += '--release' }
Write-Host "==> cargo $($cargoArgs -join ' ')" -ForegroundColor Cyan
cargo @cargoArgs
if ($LASTEXITCODE -ne 0) {
    # A running binary locks its exe and fails the relink; don't abort the rest
    # (so wasm / java / plugin still build), just warn which ones were skipped.
    $running = @('client', 'server', 'panel', 'jaged') | Where-Object { Get-Process $_ -ErrorAction SilentlyContinue }
    if ($running) {
        Write-Warning "running binaries lock their exe ($($running -join ', ')): those were NOT relinked; continuing."
    } else {
        exit $LASTEXITCODE
    }
}

# 2. wasm client bundle.
if (-not $SkipWasm) {
    Write-Host "==> wasm bundle (crates\client\web\build.ps1)" -ForegroundColor Cyan
    & "$PSScriptRoot\crates\client\web\build.ps1"
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}

# 3. RuneScript server content -> data/pack/server, via the Kotlin compiler
#    (:compiler:run) — the SAME invocation the server runs at boot. data/pack is
#    gitignored, so compiling it here is what gives `cargo test` (engine's
#    compiled_login end-to-end test) a bundle to load in CI.
if (-not $SkipScripts) {
    $jdk = Get-Jdk21Home
    if ($jdk) { $env:JAVA_HOME = $jdk }
    $root = $PSScriptRoot -replace '\\', '/'
    # One --args string the compiler splits on spaces (abs paths, fwd slashes).
    $compileArgs = "--args=--src $root/Content/scripts --out $root/data/pack " +
        "--commands $root/runescript/data/symbols/command.pack --packs $root/Content/pack"
    Write-Host "==> RuneScript content (:compiler:run -> data/pack)" -ForegroundColor Cyan
    & "$PSScriptRoot\runescript\gradlew.bat" "-p" "$PSScriptRoot\runescript" ":compiler:run" "--console=plain" $compileArgs
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}

# 4. Java client (root Gradle project; assemble = compile + jar, no tests).
if (-not $SkipJava) {
    $jdk = Get-Jdk21Home
    if ($jdk) { $env:JAVA_HOME = $jdk }
    Write-Host "==> Java client (gradle assemble)" -ForegroundColor Cyan
    & "$PSScriptRoot\gradlew.bat" assemble --console=plain
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}

# 5. IntelliJ RuneScript plugin (separate Gradle project under runescript/).
if (-not $SkipPlugin) {
    $jdk = Get-Jdk21Home
    if ($jdk) {
        $env:JAVA_HOME = $jdk
        Write-Host "==> IntelliJ plugin (:plugin:buildPlugin, JAVA_HOME=$jdk)" -ForegroundColor Cyan
    } else {
        Write-Warning "JDK 21 not found; building the plugin with the ambient JDK (may fail if it isn't 21)."
    }
    & "$PSScriptRoot\runescript\gradlew.bat" "-p" "$PSScriptRoot\runescript" ":plugin:buildPlugin" "--console=plain"
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}

Write-Host "all targets built" -ForegroundColor Green
