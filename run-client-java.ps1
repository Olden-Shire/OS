# Build and launch the reference JAVA desktop client via the root Gradle project
# (application plugin, mainClass = jagex3.client.Client). This is the byte-truth
# client the Rust port is checked against. For the Rust client use .\run-client-rust.ps1.
#
# Connects to the local server at 127.0.0.1:40001 (dev default). Start the server
# first in another terminal:  .\run-server.ps1  (headless) or .\run-server-gui.ps1 (panel)
#
# Needs a JDK (prefers JDK 21, same as the rest of the toolchain). Gradle downloads
# its own deps on first run.
#
# Usage:
#   .\run-client-java.ps1
. "$PSScriptRoot\_common.ps1"
Set-Location $PSScriptRoot

# Prefer JDK 21 for Gradle (a newer default JDK on PATH can break the build);
# fall back to an existing JAVA_HOME / java on PATH.
$jdk = Get-Jdk21Home
if ($jdk) {
    $env:JAVA_HOME = $jdk
    Write-Host "    JAVA_HOME: $jdk" -ForegroundColor DarkGray
} elseif (-not $env:JAVA_HOME -and -not (Get-Command java -ErrorAction SilentlyContinue)) {
    throw "No JDK found. Install JDK 21 (or set JAVA_HOME) to build/run the Java client."
}

Write-Host "==> building + launching Java client (gradle :run, connecting to 127.0.0.1:40001)..." -ForegroundColor Green
& "$PSScriptRoot\gradlew.bat" run --console=plain
