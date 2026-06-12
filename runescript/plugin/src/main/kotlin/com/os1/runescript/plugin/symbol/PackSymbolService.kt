package com.os1.runescript.plugin.symbol

import com.intellij.openapi.components.Service
import com.intellij.openapi.components.service
import com.intellij.openapi.project.Project
import com.os1.runescript.frontend.symbol.SymbolTable
import java.io.File

/**
 * Project-level access to our pack metadata (the same `.pack` files the CLI
 * compiler reads). Loads server + client command tables and config name maps;
 * degrades gracefully to empty when the packs aren't found so the language
 * features still load.
 */
@Service(Service.Level.PROJECT)
class PackSymbolService(private val project: Project) {

    @Volatile private var cached: Loaded? = null

    private class Loaded(
        val server: SymbolTable?,
        val client: SymbolTable?,
        val commandNames: Set<String>,
        val configNames: Set<String>,
    )

    private fun load(): Loaded {
        cached?.let { return it }
        val base = project.basePath?.let { File(it) }
        val symbolsDir = base?.resolve("runescript/data/symbols")
        val packDirs = listOfNotNull(base?.resolve("Content/pack")).filter { it.isDirectory }

        val serverCmd = symbolsDir?.resolve("command.pack")?.takeIf { it.isFile }
        val clientCmd = symbolsDir?.resolve("clientscript_command.pack")?.takeIf { it.isFile }

        val server = serverCmd?.let { runCatching { SymbolTable.load(it, packDirs) }.getOrNull() }
        val client = clientCmd?.let { runCatching { SymbolTable.load(it, packDirs) }.getOrNull() }

        val commands = buildSet {
            server?.commands?.keys?.let { addAll(it) }
            client?.commands?.keys?.let { addAll(it) }
        }
        val configs = buildSet {
            (server ?: client)?.configs?.values?.forEach { byName -> addAll(byName.keys) }
        }
        return Loaded(server, client, commands, configs).also { cached = it }
    }

    fun isKnownCommand(name: String): Boolean = load().commandNames.contains(name)
    fun commandNames(): Set<String> = load().commandNames
    fun configNames(): Set<String> = load().configNames
    fun hasMetadata(): Boolean = load().commandNames.isNotEmpty()

    /** Drop the cache (e.g. after recompiling/repacking). */
    fun invalidate() { cached = null }

    companion object {
        fun get(project: Project): PackSymbolService = project.service()
    }
}
