package com.os1.runescript.frontend.symbol

import java.io.File

/**
 * Global symbol environment, populated from our pack metadata. This is the
 * shared source of truth the CLI compiler and the IntelliJ plugin both read:
 *
 *  * commands      — `command.pack` generated from the engine opcode table
 *  * game vars     — `varp.pack`, `varbit.pack`, …
 *  * configs       — `interface.pack`, `inv.pack`, `seq.pack`, `npc.pack`, …
 *  * constants     — `constant.pack` (`name=value`, optional)
 *  * scripts       — registered from the source set being compiled
 */
class SymbolTable {
    val commands = HashMap<String, CommandSymbol>()
    val constants = HashMap<String, ConstantSymbol>()
    val vars = HashMap<String, VarSymbol>()

    /** configType -> (name -> symbol). */
    val configs = HashMap<String, HashMap<String, ConfigSymbol>>()

    /** Script name -> symbol. */
    val scriptsByName = HashMap<String, ScriptSymbol>()

    fun command(name: String): CommandSymbol? = commands[name]
    fun constant(name: String): ConstantSymbol? = constants[name]
    fun variable(name: String): VarSymbol? = vars[name]
    fun script(name: String): ScriptSymbol? = scriptsByName[name]

    /** Resolve a config name across every loaded config type. */
    fun config(name: String): ConfigSymbol? {
        for (byName in configs.values) byName[name]?.let { return it }
        return null
    }

    fun config(type: String, name: String): ConfigSymbol? = configs[type]?.get(name)

    companion object {
        /** Pack file names whose ids are game-variable ids. */
        private val VAR_KINDS = mapOf(
            "varp" to VarKind.VARP,
            "varbit" to VarKind.VARBIT,
            "varn" to VarKind.VARN,
            "vars" to VarKind.VARS,
        )

        /**
         * Load the global symbol environment.
         *
         * @param commandPack the engine command table (`command.pack`) — name→opcode.
         * @param packDirs    directories of config `.pack` files (our pack).
         * @param constantPack optional `constant.pack` (`name=value`).
         * @param engineRs2   optional `engine.rs2` — the canonical command
         *                    signatures (`[command,name](params)(returns)`).
         *                    Attaches param/return types to the opcode-keyed
         *                    commands so calls can be arity/type checked.
         */
        fun load(
            commandPack: File,
            packDirs: List<File>,
            constantPack: File? = null,
            engineRs2: File? = null,
            constantFiles: List<File> = emptyList(),
        ): SymbolTable {
            val table = SymbolTable()

            for (e in PackFile.read(commandPack)) {
                table.commands[e.name] = CommandSymbol(e.name, e.id)
            }

            // Overlay signatures from engine.rs2 onto the opcode table.
            engineRs2?.takeIf { it.isFile }?.let { f ->
                var noOpcode = 0
                for ((name, sig) in parseEngineCommands(f)) {
                    val existing = table.commands[name]
                    if (existing == null) {
                        // Declared in engine.rs2 but the engine has no opcode
                        // for it — a real mismatch (engine doesn't implement).
                        noOpcode++
                        continue
                    }
                    table.commands[name] = CommandSymbol(
                        name, existing.opcode, sig.first, sig.second, hasSignature = true,
                    )
                }
                if (noOpcode > 0) {
                    System.err.println(
                        "warning: $noOpcode command(s) in ${f.name} have no opcode in command.pack",
                    )
                }
            }

            for (dir in packDirs) {
                if (!dir.isDirectory) continue
                dir.listFiles { f -> f.extension == "pack" }?.sortedBy { it.name }?.forEach { f ->
                    val type = f.nameWithoutExtension
                    if (type == "command") return@forEach
                    val varKind = VAR_KINDS[type]
                    val byName = table.configs.getOrPut(type) { HashMap() }
                    for (e in PackFile.read(f)) {
                        if (e.name.isEmpty()) continue
                        if (varKind != null) {
                            table.vars[e.name] = VarSymbol(e.name, e.id, varKind)
                        }
                        byName[e.name] = ConfigSymbol(e.name, e.id, type)
                    }
                }
            }

            // Constants come from an optional `constant.pack` and any `.constant`
            // files scanned from the source tree. Both use `name = value` lines;
            // a leading `^` on the name (RuneScript `.constant` style) is stripped
            // so it matches the `^name` reference form used in scripts.
            fun loadConstants(f: File) {
                if (!f.exists()) return
                f.forEachLine { raw ->
                    val line = raw.substringBefore("//").trim()
                    if (line.isEmpty()) return@forEachLine
                    val eq = line.indexOf('=')
                    if (eq <= 0) return@forEachLine
                    val name = line.substring(0, eq).trim().removePrefix("^")
                    val value = line.substring(eq + 1).trim()
                    val intVal = value.toIntOrNull()
                    table.constants[name] = ConstantSymbol(name, intVal, if (intVal == null) value else null)
                }
            }
            constantPack?.let { loadConstants(it) }
            constantFiles.forEach { loadConstants(it) }

            return table
        }

        private val COMMAND_DECL = Regex("""^\s*\[command,(\.?[A-Za-z0-9_]+)]\s*(.*)$""")
        private val PAREN_GROUP = Regex("""\(([^)]*)\)""")

        /**
         * Parse `[command,name](params)(returns)` declarations from an
         * engine.rs2. Returns name → (paramTypes, returnTypes). Declarations
         * with no clean `(...)` signature (e.g. `// todo: signature`) are
         * skipped — those commands stay untyped (no arity check).
         */
        private fun parseEngineCommands(file: File): Map<String, Pair<List<String>, List<String>>> {
            val out = HashMap<String, Pair<List<String>, List<String>>>()
            file.forEachLine { raw ->
                val m = COMMAND_DECL.find(raw) ?: return@forEachLine
                val name = m.groupValues[1]
                val rest = m.groupValues[2].substringBefore("//").trim()
                if (!rest.startsWith("(")) return@forEachLine // untyped declaration
                val groups = PAREN_GROUP.findAll(rest).map { it.groupValues[1] }.toList()
                val params = splitTypes(groups.getOrNull(0), stripVar = true)
                val returns = splitTypes(groups.getOrNull(1), stripVar = false)
                out[name] = params to returns
            }
            return out
        }

        /** Split a `coord $a, int $b` / `int, coord` list into bare type names. */
        private fun splitTypes(group: String?, stripVar: Boolean): List<String> {
            if (group.isNullOrBlank()) return emptyList()
            return group.split(',').mapNotNull { tok ->
                val t = tok.trim()
                if (t.isEmpty()) return@mapNotNull null
                if (stripVar) t.substringBefore('$').trim() else t
            }.filter { it.isNotEmpty() }
        }
    }
}
