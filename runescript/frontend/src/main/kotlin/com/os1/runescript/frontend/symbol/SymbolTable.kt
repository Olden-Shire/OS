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
         * @param commandPack the engine command table (`command.pack`).
         * @param packDirs    directories of config `.pack` files (our pack).
         * @param constantPack optional `constant.pack` (`name=value`).
         */
        fun load(commandPack: File, packDirs: List<File>, constantPack: File? = null): SymbolTable {
            val table = SymbolTable()

            for (e in PackFile.read(commandPack)) {
                table.commands[e.name] = CommandSymbol(e.name, e.id)
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

            constantPack?.let { cp ->
                if (cp.exists()) {
                    cp.forEachLine { raw ->
                        val line = raw.substringBefore("//").trim()
                        if (line.isEmpty()) return@forEachLine
                        val eq = line.indexOf('=')
                        if (eq <= 0) return@forEachLine
                        val name = line.substring(0, eq).trim()
                        val value = line.substring(eq + 1).trim()
                        val intVal = value.toIntOrNull()
                        table.constants[name] = ConstantSymbol(name, intVal, if (intVal == null) value else null)
                    }
                }
            }

            return table
        }
    }
}
