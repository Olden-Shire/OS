package com.os1.runescript.frontend.symbol

/**
 * Resolved symbols. The frontend exposes these to both the compiler codegen
 * (which needs their runtime ids) and the IntelliJ plugin (which needs their
 * kind + source for tooltips and navigation).
 */
sealed interface Symbol {
    val name: String
}

/**
 * A built-in engine command. `name` maps to a bytecode opcode (from
 * `command.pack`); `paramTypes` / `returnTypes` are its signature (from
 * `engine.rs2`). `hasSignature` is false for commands declared without a
 * signature (or absent from engine.rs2) — those skip arity/type checks.
 */
class CommandSymbol(
    override val name: String,
    val opcode: Int,
    val paramTypes: List<String> = emptyList(),
    val returnTypes: List<String> = emptyList(),
    val hasSignature: Boolean = false,
) : Symbol

/** A RuneScript proc/trigger/label. `id` is assigned during registration. */
class ScriptSymbol(
    override val name: String,
    var id: Int,
    val trigger: String,
    /** Subject type for typed triggers (e.g. a config type), else null. */
    val subjectType: String? = null,
) : Symbol

/** A `^constant`. Either an integer or a string value. */
class ConstantSymbol(
    override val name: String,
    val intValue: Int? = null,
    val stringValue: String? = null,
) : Symbol

/** A game variable: `%varp`, `%varbit`, `%varn`, `%vars`. */
class VarSymbol(override val name: String, val id: Int, val kind: VarKind) : Symbol

enum class VarKind {
    // Server vars.
    VARP, VARBIT, VARN, VARS,
    // Client vars (clientscript only): varc_int / varc_str.
    VARC_INT, VARC_STR,
}

/** A config reference (interface, inv, seq, npc, obj, loc, …). */
class ConfigSymbol(override val name: String, val id: Int, val configType: String) : Symbol

/** A `$local` variable or parameter declared within a script. */
class LocalVariableSymbol(
    override val name: String,
    /** Base type as written, e.g. `int`, `string`, `coord`, `intarray`. */
    val type: String,
    val isArray: Boolean,
    val isParameter: Boolean,
) : Symbol {
    val isString: Boolean get() = type == "string"
}
