package com.os.runescript.plugin.cli

import com.intellij.openapi.project.Project
import java.io.File
import java.nio.charset.StandardCharsets
import java.util.concurrent.TimeUnit

/**
 * Bridge to the OS Rust CLI (`crates/app`). The plugin never re-implements the cs2
 * codec — the verified pipeline in the Rust workspace is the single source of truth,
 * so the asm preview and the decompile action shell out to `app cs2asm` / `app cs2src`.
 */
object OsCli {
    data class Result(val exitCode: Int, val stdout: String, val stderr: String) {
        val ok: Boolean get() = exitCode == 0
    }

    /** Locate the workspace's `app` binary, preferring the release build. */
    fun findBinary(project: Project): File? {
        val base = project.basePath ?: return null
        val candidates = listOf(
            "target/release/app.exe",
            "target/release/app",
            "target/debug/app.exe",
            "target/debug/app",
        )
        return candidates.map { File(base, it) }.firstOrNull { it.canExecute() || it.isFile }
    }

    /** Human-readable hint when the binary is missing. */
    const val BUILD_HINT: String =
        "OS CLI not found — build it with `cargo build -p app --release` in the workspace root."

    /**
     * Run the CLI with the project root as working directory (so the default `cache/` and
     * `Content/` paths resolve). Blocking — call from a pooled thread, not the EDT.
     */
    fun run(project: Project, vararg args: String, timeoutSeconds: Long = 30): Result? {
        val binary = findBinary(project) ?: return null
        val process = ProcessBuilder(listOf(binary.absolutePath) + args)
            .directory(project.basePath?.let(::File))
            .start()
        val stdout = process.inputStream.readBytes().toString(StandardCharsets.UTF_8)
        val stderr = process.errorStream.readBytes().toString(StandardCharsets.UTF_8)
        if (!process.waitFor(timeoutSeconds, TimeUnit.SECONDS)) {
            process.destroyForcibly()
            return Result(-1, stdout, "timed out after ${timeoutSeconds}s")
        }
        return Result(process.exitValue(), stdout, stderr)
    }
}
