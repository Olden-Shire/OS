rootProject.name = "runescript"

pluginManagement {
    repositories {
        gradlePluginPortal()
        mavenCentral()
    }
}

// Let Gradle auto-provision JDK toolchains it can't find locally (the `:frontend`
// / `:compiler` modules need a JDK 17 toolchain, `:plugin` needs 21). Without
// this, a machine/CI runner that only has one of those JDKs fails with
// "No locally installed toolchains match ... toolchain download repositories
// have not been configured."
plugins {
    id("org.gradle.toolchains.foojay-resolver-convention") version "0.8.0"
}

// The compiler frontend (lexer, parser, symbols, pack-backed metadata) is a
// standalone module so the IntelliJ platform plugin can depend on it directly
// — one source of truth for parsing + symbol resolution shared by the CLI
// compiler and the IDE tooling.
include("frontend")
include("compiler")
include("plugin")
