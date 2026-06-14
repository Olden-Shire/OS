package com.os.runescript.compiler.codegen

import com.os.runescript.frontend.ast.BinaryOp
import com.os.runescript.frontend.symbol.VarKind

/**
 * The parts of the opcode set that differ between server scripts and
 * clientscripts. Everything else (push/pop, branches, gosub, locals, join,
 * arrays) shares the same ids in both, so the codegen is otherwise identical.
 *
 *  * Server: arithmetic at 4600+, game vars varp/varbit/varn/vars.
 *  * Client (cs2): arithmetic at 4000+, game vars varp/varbit/varc_int/varc_str.
 */
interface OpcodeProfile {
    /** Opcode for a `calc` arithmetic/bitwise operator. */
    fun arithmetic(op: BinaryOp): Int

    /** Opcode for a game-variable push/pop of the given kind. */
    fun gameVar(kind: VarKind, pop: Boolean): Int

    companion object {
        val SERVER: OpcodeProfile = object : OpcodeProfile {
            override fun arithmetic(op: BinaryOp): Int = when (op) {
                BinaryOp.ADD -> ServerScriptOpcode.ADD
                BinaryOp.SUB -> ServerScriptOpcode.SUB
                BinaryOp.MUL -> ServerScriptOpcode.MULTIPLY
                BinaryOp.DIV -> ServerScriptOpcode.DIVIDE
                BinaryOp.MOD -> ServerScriptOpcode.MODULO
                BinaryOp.AND -> ServerScriptOpcode.AND
                BinaryOp.OR -> ServerScriptOpcode.OR
                else -> ServerScriptOpcode.ADD
            }.id

            override fun gameVar(kind: VarKind, pop: Boolean): Int = when (kind) {
                VarKind.VARP -> if (pop) ServerScriptOpcode.POP_VARP else ServerScriptOpcode.PUSH_VARP
                VarKind.VARBIT -> if (pop) ServerScriptOpcode.POP_VARBIT else ServerScriptOpcode.PUSH_VARBIT
                VarKind.VARN -> if (pop) ServerScriptOpcode.POP_VARN else ServerScriptOpcode.PUSH_VARN
                VarKind.VARS -> if (pop) ServerScriptOpcode.POP_VARS else ServerScriptOpcode.PUSH_VARS
                else -> error("server scripts have no $kind")
            }.id
        }

        /**
         * cs2 opcode profile. Core op ids match [ServerScriptOpcode] (shared
         * clientscript2 lineage); only arithmetic and the var kinds differ.
         */
        val CLIENT: OpcodeProfile = object : OpcodeProfile {
            override fun arithmetic(op: BinaryOp): Int = when (op) {
                BinaryOp.ADD -> 4000
                BinaryOp.SUB -> 4001
                BinaryOp.MUL -> 4002
                BinaryOp.DIV -> 4003
                BinaryOp.MOD -> 4011
                BinaryOp.AND -> 4014
                BinaryOp.OR -> 4015
                else -> 4000
            }

            override fun gameVar(kind: VarKind, pop: Boolean): Int = when (kind) {
                VarKind.VARP -> if (pop) 2 else 1
                VarKind.VARBIT -> if (pop) 27 else 25
                VarKind.VARC_INT -> if (pop) 43 else 42
                VarKind.VARC_STR -> if (pop) 48 else 47
                else -> error("clientscripts have no $kind")
            }
        }
    }
}
