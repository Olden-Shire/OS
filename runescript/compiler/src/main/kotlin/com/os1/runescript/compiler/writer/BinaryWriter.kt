package com.os1.runescript.compiler.writer

import com.os1.runescript.compiler.codegen.CompiledScript
import com.os1.runescript.compiler.codegen.Instr
import com.os1.runescript.compiler.codegen.ServerScriptOpcode
import java.io.ByteArrayOutputStream
import java.io.DataOutputStream

/**
 * Serializes a [CompiledScript] to the on-disk blob the engine reads in
 * `crates/engine/src/script/file.rs` (identical layout to RuneScriptTS's
 * `BinaryScriptWriterContext.finish`). All multi-byte values big-endian.
 */
object BinaryWriter {

    fun write(script: CompiledScript): ByteArray {
        // Encode the instruction stream first; branch/switch offsets are
        // relative to instruction *indices*, resolved here.
        val instrBuf = ByteArrayOutputStream()
        val instrOut = DataOutputStream(instrBuf)
        for ((index, instr) in script.instructions.withIndex()) {
            writeInstruction(instrOut, instr, index)
        }

        // Switch tables: uint16 keyCount, then [int32 key, int32 jump]*.
        val switchBuf = ByteArrayOutputStream()
        val switchOut = DataOutputStream(switchBuf)
        for (table in script.switchTables) {
            switchOut.writeShort(table.cases.size)
            for ((key, label) in table.cases) {
                switchOut.writeInt(key)
                // jump is relative to the SWITCH instruction; resolved by the
                // index recorded when the SwitchOp was emitted.
                val switchIndex = switchInstructionIndex(script, table.id)
                switchOut.writeInt(label.target - switchIndex - 1)
            }
        }

        val lineTable = buildLineTable(script)

        val out = ByteArrayOutputStream()
        val d = DataOutputStream(out)

        writeString(d, script.fullName)
        writeString(d, script.sourceName)
        // i64 since pack v27 — component subjects overflow 32 bits.
        d.writeLong(script.lookupKey)

        d.writeByte(script.parameterTypeCodes.size)
        for (code in script.parameterTypeCodes) d.writeByte(code)

        d.writeShort(lineTable.size)
        for ((pc, line) in lineTable) {
            d.writeInt(pc)
            d.writeInt(line)
        }

        instrBuf.writeTo(d)

        d.writeInt(script.instructions.size)
        d.writeShort(script.intLocalCount)
        d.writeShort(script.stringLocalCount)
        d.writeShort(script.intArgCount)
        d.writeShort(script.stringArgCount)

        d.writeByte(script.switchTables.size)
        switchBuf.writeTo(d)

        // Trailer length = switch byte count + 1 (the switch-count byte).
        d.writeShort(switchBuf.size() + 1)

        return out.toByteArray()
    }

    private fun writeInstruction(out: DataOutputStream, instr: Instr, index: Int) {
        when (instr) {
            is Instr.IntOp -> {
                out.writeShort(instr.opcode)
                if (instr.large) out.writeInt(instr.operand) else out.writeByte(instr.operand and 0xFF)
            }
            is Instr.StrOp -> {
                out.writeShort(ServerScriptOpcode.PUSH_CONSTANT_STRING.id)
                writeString(out, instr.text)
            }
            is Instr.Command -> {
                out.writeShort(instr.opcode)
                out.writeByte(if (instr.secondary) 1 else 0)
            }
            is Instr.BranchOp -> {
                out.writeShort(instr.opcode)
                // Relative to the next instruction: target - thisIndex - 1.
                out.writeInt(instr.label.target - index - 1)
            }
            is Instr.SwitchOp -> {
                out.writeShort(ServerScriptOpcode.SWITCH.id)
                out.writeInt(instr.tableId)
            }
        }
    }

    /** Instruction index of the SWITCH op feeding switch table [tableId]. */
    private fun switchInstructionIndex(script: CompiledScript, tableId: Int): Int {
        script.instructions.forEachIndexed { i, instr ->
            if (instr is Instr.SwitchOp && instr.tableId == tableId) return i
        }
        return 0
    }

    /** instruction index -> line, emitted only when the line changes. */
    private fun buildLineTable(script: CompiledScript): List<Pair<Int, Int>> {
        val table = mutableListOf<Pair<Int, Int>>()
        var prev = -1
        script.instructions.forEachIndexed { index, instr ->
            val line = instr.line
            if (line != -1 && line != prev) {
                table += index to line
                prev = line
            }
        }
        return table
    }

    /** cp1252-ish: low byte of each char, null-terminated. */
    private fun writeString(out: DataOutputStream, text: String) {
        for (c in text) out.writeByte(c.code and 0xFF)
        out.writeByte(0)
    }
}
