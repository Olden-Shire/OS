package com.os1.runescript.compiler

import com.os1.runescript.frontend.diagnostics.Diagnostics
import com.os1.runescript.frontend.symbol.SymbolTable
import java.io.File
import kotlin.system.exitProcess

/**
 * CLI: compile a `.rs2` source set into `server/script.{dat,idx}`.
 *
 * Usage:
 *   runescript-compiler --src DIR --out DIR --commands FILE [--packs DIR[,DIR]] [--constants FILE]
 *
 * `--packs` points at our pack metadata (the `.pack` name<->id maps); the same
 * metadata the IntelliJ plugin will consume.
 */
fun main(args: Array<String>) {
    val opts = parseArgs(args)
    val srcDir = File(opts["src"] ?: fail("missing --src"))
    val outDir = File(opts["out"] ?: fail("missing --out"))
    val commandPack = File(opts["commands"] ?: fail("missing --commands"))
    val packDirs = (opts["packs"] ?: "").split(",").filter { it.isNotBlank() }.map { File(it) }
    val constantPack = opts["constants"]?.let { File(it) }

    if (!srcDir.isDirectory) fail("source dir not found: $srcDir")
    if (!commandPack.isFile) fail("command pack not found: $commandPack")

    val sources = srcDir.walkTopDown().filter { it.isFile && it.extension == "rs2" }.sortedBy { it.path }.toList()
    if (sources.isEmpty()) fail("no .rs2 sources under $srcDir")

    val symbols = SymbolTable.load(commandPack, packDirs, constantPack)
    val diagnostics = Diagnostics()

    val ok = Compiler(symbols, diagnostics).compile(sources, outDir)
    diagnostics.printAll()

    if (!ok || diagnostics.hasErrors()) {
        System.err.println("compilation failed (${diagnostics.errorCount} error(s))")
        exitProcess(1)
    }

    val scriptCount = sources.sumOf { countScripts(it) }
    println("compiled $scriptCount script(s) from ${sources.size} file(s) -> ${File(outDir, "server")}")
}

private fun countScripts(file: File): Int =
    file.readLines().count { it.trimStart().startsWith("[") }

private fun parseArgs(args: Array<String>): Map<String, String> {
    val map = HashMap<String, String>()
    var i = 0
    while (i < args.size) {
        val a = args[i]
        if (a.startsWith("--") && i + 1 < args.size) {
            map[a.removePrefix("--")] = args[i + 1]
            i += 2
        } else i++
    }
    return map
}

private fun fail(message: String): Nothing {
    System.err.println("error: $message")
    exitProcess(2)
}
