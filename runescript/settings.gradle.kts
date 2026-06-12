rootProject.name = "runescript"

pluginManagement {
    repositories {
        gradlePluginPortal()
        mavenCentral()
    }
}

// The compiler frontend (lexer, parser, symbols, pack-backed metadata) is a
// standalone module so the IntelliJ platform plugin can depend on it directly
// — one source of truth for parsing + symbol resolution shared by the CLI
// compiler and the IDE tooling.
include("frontend")
include("compiler")
include("plugin")
