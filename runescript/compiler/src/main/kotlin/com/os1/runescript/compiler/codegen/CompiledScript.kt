package com.os1.runescript.compiler.codegen

/** A jump target, resolved to an instruction index after codegen. */
class Label {
    var target: Int = -1
}

/** One emitted bytecode instruction. */
sealed class Instr {
    /** Source line for the line-number table (-1 = none). */
    var line: Int = -1

    /** A language opcode with an integer operand. */
    class IntOp(val opcode: Int, val operand: Int, val large: Boolean) : Instr()

    /** PUSH_CONSTANT_STRING with its inline string. */
    class StrOp(val text: String) : Instr()

    /** A command call: raw `[opcode u16][secondary u8]`. */
    class Command(val opcode: Int, val secondary: Boolean) : Instr()

    /** A branch whose relative operand is computed from [label] at write time. */
    class BranchOp(val opcode: Int, val label: Label, val large: Boolean = true) : Instr()

    /** SWITCH — operand is the switch-table index. */
    class SwitchOp(val tableId: Int) : Instr()
}

/** A resolved switch table: each case key maps to a target label. */
class SwitchTable(val id: Int) {
    val cases = mutableListOf<Pair<Int, Label>>() // (key, target)
}

/** A fully compiled script, ready to serialize to the pack blob. */
class CompiledScript(
    val id: Int,
    val fullName: String,
    val sourceName: String,
    val lookupKey: Long,
    /** Parameter type codes (debugproc only); empty otherwise. */
    val parameterTypeCodes: List<Int>,
    val instructions: List<Instr>,
    val switchTables: List<SwitchTable>,
    val intLocalCount: Int,
    val stringLocalCount: Int,
    val intArgCount: Int,
    val stringArgCount: Int,
)
