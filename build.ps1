# Builds all client targets so nothing drifts behind the source:
#   1. desktop exe         -> target/debug/client.exe   (or -Release)
#   2. wasm web bundle     -> crates/client/web/pkg     (always release)
#   3. IntelliJ plugin zip -> runescript/plugin/build/distributions (needs JDK 21)
#
#   .\build.ps1             # debug exe + wasm bundle + plugin
#   .\build.ps1 -Release    # release exe + wasm bundle + plugin
#   .\build.ps1 -SkipWasm   # skip the wasm bundle
#   .\build.ps1 -SkipPlugin # skip the IntelliJ plugin (faster iteration)
#
# NOTE: ASCII only in this file. PowerShell 5.1 reads BOM-less UTF-8
# as ANSI, and a UTF-8 em-dash inside a string garbles into a curly
# quote that breaks parsing.
param(
    [switch]$Release,
    [switch]$SkipWasm,
    [switch]$SkipPlugin
)
. "$PSScriptRoot\_common.ps1"

if ($Release) {
    cargo build -p client --release
} else {
    cargo build -p client
}
if ($LASTEXITCODE -ne 0) {
    # A running client locks target/debug/client.exe and fails the copy;
    # still build the wasm bundle so it never drifts behind the source.
    if (Get-Process client -ErrorAction SilentlyContinue) {
        Write-Warning "client.exe is running (binary locked): exe NOT rebuilt; continuing with wasm"
    } else {
        exit $LASTEXITCODE
    }
}

if (-not $SkipWasm) {
    & "$PSScriptRoot\crates\client\web\build.ps1"
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}

if (-not $SkipPlugin) {
    # IntelliJ RuneScript plugin (Kotlin compiler/frontend + plugin). Needs JDK 21;
    # the IntelliJ Platform Gradle plugin downloads its SDK on the first run.
    $jdk = Get-Jdk21Home
    if ($jdk) {
        $env:JAVA_HOME = $jdk
        Write-Host "building IntelliJ plugin (JAVA_HOME=$jdk)..."
    } else {
        Write-Warning "JDK 21 not found; building the plugin with the ambient JDK (may fail if it isn't 21)."
    }
    & "$PSScriptRoot\runescript\gradlew.bat" "-p" "$PSScriptRoot\runescript" ":plugin:buildPlugin" "--console=plain"
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}

Write-Host "all targets built"
