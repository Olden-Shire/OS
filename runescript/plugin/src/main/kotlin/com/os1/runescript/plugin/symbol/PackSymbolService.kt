package com.os1.runescript.plugin.symbol

import com.intellij.openapi.Disposable
import com.intellij.openapi.components.Service
import com.intellij.openapi.components.service
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFileManager
import com.intellij.openapi.vfs.newvfs.BulkFileListener
import com.intellij.openapi.vfs.newvfs.events.VFileEvent
import com.os1.runescript.frontend.symbol.CommandSymbol
import com.os1.runescript.frontend.symbol.SymbolTable
import java.io.File

/**
 * Project-level access to our pack metadata (the same `.pack` files the CLI
 * compiler reads). Loads server + client command tables and config name maps;
 * degrades gracefully to empty when the packs aren't found so the language
 * features still load.
 */
@Service(Service.Level.PROJECT)
class PackSymbolService(private val project: Project) : Disposable {

    @Volatile private var cached: Loaded? = null

    init {
        // Drop the cache when a pack file or engine.rs2 changes on disk (save
        // or external edit + VFS refresh), so edits show without an IDE restart.
        project.messageBus.connect(this).subscribe(
            VirtualFileManager.VFS_CHANGES,
            object : BulkFileListener {
                override fun after(events: List<VFileEvent>) {
                    if (events.any { affectsSymbols(it.path) }) invalidate()
                }
            },
        )
    }

    private fun affectsSymbols(path: String): Boolean =
        path.endsWith("/engine.rs2") ||
            (path.endsWith(".pack") && (path.contains("/Content/pack/") || path.contains("/data/symbols/")))

    override fun dispose() {}

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
        // engine.rs2 overlays command param/return signatures onto the opcode
        // table (server engine commands) — drives signature hover + completion.
        val engineRs2 = base?.resolve("Content/scripts/engine.rs2")?.takeIf { it.isFile }

        val server = serverCmd?.let { runCatching { SymbolTable.load(it, packDirs, engineRs2 = engineRs2) }.getOrNull() }
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

    /**
     * Resolved interface id for a symbol name — a root (`if_549`, or a renamed
     * name) or a component (`if_549:com_2`). Roots resolve to the real
     * interface id; components to the packed `(id << 16) | child`. `null` when
     * the name isn't a known interface symbol.
     */
    fun interfaceId(name: String): Int? {
        val l = load()
        return (l.server ?: l.client)?.config("interface", name)?.id
    }

    /**
     * One-line hover label for a symbol — command, var, constant, or config
     * (interface, inv, seq, …). Interfaces read `Interface {id} {name}`;
     * interface components decompose the packed id to `Component
     * {parent}:{child} {name}`. `null` when the name isn't in the pack metadata
     * (locals, script names, labels).
     */
    fun describe(name: String): String? {
        val l = load()
        val tables = listOfNotNull(l.server, l.client)
        for (t in tables) t.command(name)?.let {
            return if (it.hasSignature) "Command $name${signatureText(it)}" else "Command ${it.opcode} $name"
        }
        for (t in tables) t.constant(name)?.let { c ->
            return "Constant $name = ${c.intValue ?: c.stringValue ?: ""}"
        }
        for (t in tables) t.config(name)?.let { return formatConfig(it.configType, it.id, name) }
        return null
    }

    /** True if `name` is any known pack symbol (command/config/var/constant). */
    fun isKnownSymbol(name: String): Boolean = describe(name) != null

    /**
     * Rich hover HTML. Commands lay their parameters out VERTICALLY (one type
     * per line) so long signatures fit a narrow tooltip instead of overflowing
     * the width. Non-commands fall back to the compact [describe] line.
     */
    fun docHtml(name: String): String? {
        val l = load()
        val tables = listOfNotNull(l.server, l.client)
        for (t in tables) t.command(name)?.let { c ->
            if (!c.hasSignature) return "command <b>$name</b> &mdash; opcode ${c.opcode}"
            val params = if (c.paramTypes.isEmpty()) {
                "()"
            } else {
                c.paramTypes.joinToString(
                    separator = "<br>&nbsp;&nbsp;",
                    prefix = "(<br>&nbsp;&nbsp;",
                    postfix = "<br>)",
                )
            }
            val ret = if (c.returnTypes.isEmpty()) "void" else c.returnTypes.joinToString(", ")
            return "command <b>$name</b> $params<br>&rarr; $ret"
        }
        return describe(name)
    }

    /** A command's `(params)(returns)` signature, or null if unknown/untyped —
     *  for completion tail text. */
    fun commandSignature(name: String): String? {
        val l = load()
        val c = (l.server ?: l.client)?.command(name) ?: return null
        return if (c.hasSignature) signatureText(c) else null
    }

    private fun signatureText(c: CommandSymbol): String =
        "(${c.paramTypes.joinToString(", ")})(${c.returnTypes.joinToString(", ")})"

    /** Expected argument count for a typed command, or null if it has no
     *  signature (untyped commands skip arity checking, like the compiler). */
    fun commandParamCount(name: String): Int? {
        val l = load()
        val c = (l.server ?: l.client)?.command(name) ?: return null
        return if (c.hasSignature) c.paramTypes.size else null
    }

    /** A config/var reference's `(configType, id)` — for navigating to its
     *  definition file. `null` if `name` isn't a config. */
    fun configRef(name: String): Pair<String, Int>? {
        val l = load()
        val t = l.server ?: l.client ?: return null
        val c = t.config(name) ?: return null
        return c.configType to c.id
    }

    /** Is `name` a known config of `type`? `component` maps to the interface
     *  table (component symbols `iface:child` live there). For trigger-subject
     *  validation. */
    fun isKnownConfig(type: String, name: String): Boolean {
        val l = load()
        val t = l.server ?: l.client ?: return false
        val lookup = if (type == "component") "interface" else type
        return t.config(lookup, name) != null
    }

    /** Names of every config of `type` (`component` → interface table). For
     *  trigger-subject completion. */
    fun configNamesOfType(type: String): Collection<String> {
        val l = load()
        val t = l.server ?: l.client ?: return emptyList()
        val lookup = if (type == "component") "interface" else type
        return t.configs[lookup]?.keys ?: emptyList()
    }

    private fun formatConfig(type: String, id: Int, name: String): String = when {
        type == "interface" && name.contains(':') ->
            "Component ${id ushr 16}:${id and 0xFFFF} $name"
        type == "interface" -> "Interface $id $name"
        else -> "${type.replaceFirstChar(Char::uppercase)} $id $name"
    }

    /** Config name → id pairs, optionally for one config type, for completion. */
    fun configEntries(): List<Triple<String, String, Int>> {
        val l = load()
        val t = l.server ?: l.client ?: return emptyList()
        return t.configs.entries.flatMap { (type, byName) ->
            byName.values.map { Triple(it.name, type, it.id) }
        }
    }

    /** Drop the cache (e.g. after recompiling/repacking). */
    fun invalidate() { cached = null }

    companion object {
        fun get(project: Project): PackSymbolService = project.service()
    }
}
