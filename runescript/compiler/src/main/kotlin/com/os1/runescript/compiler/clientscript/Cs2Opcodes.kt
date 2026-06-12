package com.os1.runescript.compiler.clientscript

/**
 * cs2 opcode metadata (name, operand kind, stack arity), loaded from the
 * `cs2_opcodes.tsv` resource generated off `crates/cache/src/cs2_opcodes.rs`.
 * The decompiler uses the stack deltas to recover command call arity.
 */
object Cs2Opcodes {
    data class Meta(
        val op: Int,
        val name: String,
        val mnemonic: String,
        val kind: String,
        /** Net int-stack delta; null when unknown (variable arity). */
        val intDelta: Int?,
        val strDelta: Int?,
    )

    private val byOp: Map<Int, Meta> = load()

    fun meta(op: Int): Meta? = byOp[op]
    fun name(op: Int): String = byOp[op]?.name ?: "op$op"

    fun isConditionalBranch(op: Int): Boolean = op in 7..10 || op == 31 || op == 32
    fun isUnconditionalBranch(op: Int): Boolean = op == 6
    fun isBranch(op: Int): Boolean = isUnconditionalBranch(op) || isConditionalBranch(op)
    fun isCommand(op: Int): Boolean = op >= 100

    private fun load(): Map<Int, Meta> {
        val res = Cs2Opcodes::class.java.getResourceAsStream("/cs2_opcodes.tsv")
            ?: error("cs2_opcodes.tsv resource missing")
        val map = HashMap<Int, Meta>()
        res.bufferedReader().useLines { lines ->
            for ((i, line) in lines.withIndex()) {
                if (i == 0 || line.isBlank()) continue
                val c = line.split('\t')
                val intD = c[4].toIntOrNull()
                val strD = c[5].toIntOrNull()
                map[c[0].toInt()] = Meta(c[0].toInt(), c[1], c[2], c[3], intD, strD)
            }
        }
        return map
    }
}
