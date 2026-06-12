package com.os1.runescript.compiler.clientscript

import com.os1.runescript.compiler.codegen.CompiledScript
import com.os1.runescript.compiler.codegen.Instr
import java.io.ByteArrayOutputStream
import java.io.DataInputStream
import java.io.DataOutputStream

/**
 * The ClientScript (cs2) bytecode container — the format the rev1 client reads
 * in `ClientScript.get` (ported in `crates/cache/src/cs2.rs`):
 *
 *   [optional name: fastgstr]  (we emit a single 0 byte = no name)
 *   [instructions]
 *   [12-byte trailer]: u32 instructionCount, u16 intLocal, u16 strLocal,
 *                      u16 intArg, u16 strArg
 *
 * Per-instruction operand width: op==3 -> inline string; op>=100 or
 * op in {21,38,39} -> 1 byte; else -> 4 bytes (big-endian). No lookup key,
 * source name, line table, or switch block (rev1 cs2 predates those).
 */
object Cs2Writer {

    fun write(script: CompiledScript): ByteArray {
        val instr = ByteArrayOutputStream()
        val iout = DataOutputStream(instr)
        for ((index, ins) in script.instructions.withIndex()) {
            writeInstruction(iout, ins, index)
        }

        val out = ByteArrayOutputStream()
        val d = DataOutputStream(out)
        d.writeByte(0) // no name (fastgstr sees 0 -> None)
        instr.writeTo(d)
        d.writeInt(script.instructions.size)
        d.writeShort(script.intLocalCount)
        d.writeShort(script.stringLocalCount)
        d.writeShort(script.intArgCount)
        d.writeShort(script.stringArgCount)
        return out.toByteArray()
    }

    private fun writeInstruction(out: DataOutputStream, ins: Instr, index: Int) {
        when (ins) {
            is Instr.StrOp -> { out.writeShort(3); writeString(out, ins.text) }
            is Instr.Command -> { out.writeShort(ins.opcode); out.writeByte(if (ins.secondary) 1 else 0) }
            is Instr.BranchOp -> { out.writeShort(ins.opcode); out.writeInt(ins.label.target - index - 1) }
            is Instr.IntOp -> { out.writeShort(ins.opcode); writeOperand(out, ins.opcode, ins.operand) }
            is Instr.SwitchOp -> error("rev1 clientscripts have no switch tables")
        }
    }

    /** cs2 operand width rule (inverse of `cs2.rs` decode). */
    private fun writeOperand(out: DataOutputStream, opcode: Int, operand: Int) {
        if (opcode >= 100 || opcode == 21 || opcode == 38 || opcode == 39) {
            out.writeByte(operand and 0xFF)
        } else {
            out.writeInt(operand)
        }
    }

    private fun writeString(out: DataOutputStream, text: String) {
        for (c in text) out.writeByte(c.code and 0xFF)
        out.writeByte(0)
    }
}

/** A decoded cs2 script — parallel-array form, identical to `cs2.rs`. */
class Cs2Script(
    val name: String?,
    val opcodes: IntArray,
    val intOperands: IntArray,
    val stringOperands: Array<String?>,
    val intLocalCount: Int,
    val stringLocalCount: Int,
    val intArgCount: Int,
    val stringArgCount: Int,
)

/** Decoder — a 1:1 port of `crates/cache/src/cs2.rs` (`ClientScript::decode`). */
object Cs2Decoder {
    fun decode(bytes: ByteArray): Cs2Script? {
        if (bytes.size < 12) return null

        val trailer = DataInputStream(bytes.inputStream().also { it.skip((bytes.size - 12).toLong()) })
        val instructionCount = trailer.readInt()
        val intLocal = trailer.readUnsignedShort()
        val strLocal = trailer.readUnsignedShort()
        val intArg = trailer.readUnsignedShort()
        val strArg = trailer.readUnsignedShort()

        val r = ByteReader(bytes)
        val name = r.fastGStr()

        val ops = IntArray(instructionCount)
        val ints = IntArray(instructionCount)
        val strs = arrayOfNulls<String>(instructionCount)
        val trailerPos = bytes.size - 12
        var i = 0
        while (r.pos < trailerPos && i < instructionCount) {
            val op = r.u16()
            when {
                op == 3 -> strs[i] = r.gStr()
                op >= 100 || op == 21 || op == 38 || op == 39 -> ints[i] = r.u8()
                else -> ints[i] = r.i32()
            }
            ops[i] = op
            i++
        }

        return Cs2Script(
            name,
            ops.copyOf(i), ints.copyOf(i), strs.copyOf(i),
            intLocal, strLocal, intArg, strArg,
        )
    }

    private class ByteReader(val b: ByteArray) {
        var pos = 0
        fun u8(): Int = b[pos++].toInt() and 0xFF
        fun u16(): Int = (u8() shl 8) or u8()
        fun i32(): Int = (u8() shl 24) or (u8() shl 16) or (u8() shl 8) or u8()
        fun gStr(): String {
            val sb = StringBuilder()
            while (b[pos].toInt() != 0) sb.append((b[pos++].toInt() and 0xFF).toChar())
            pos++ // null
            return sb.toString()
        }
        fun fastGStr(): String? {
            if (b[pos].toInt() == 0) { pos++; return null }
            return gStr()
        }
    }
}
