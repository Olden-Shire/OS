package com.os1.runescript.compiler.clientscript

import com.os1.runescript.frontend.symbol.SymbolTable

/**
 * Structured cs2 decompiler: bytecode -> RuneScript source. Recovers control
 * flow (if/else/while) by inverting the compiler's own code shapes — a forward
 * conditional branch is an `if` (skip-on-false), a trailing unconditional
 * branch makes it `if/else`, and a backward branch is a `while`. Expressions
 * are rebuilt by symbolic execution of the stack machine; void-command call
 * arity comes from the opcode stack deltas.
 *
 * Produces source the compiler re-accepts (round-trip) for the constructs it
 * emits. Value-returning / variable-arity commands without a known signature
 * are surfaced explicitly rather than guessed.
 */
class Decompiler(
    private val script: Cs2Script,
    private val scriptName: String = "script",
    private val symbols: SymbolTable? = null,
    /** id -> clientscript name, for gosub/proc references. */
    private val scriptNames: (Int) -> String? = { null },
) {
    private val n = script.opcodes.size
    private val out = StringBuilder()

    private class Expr(val text: String, val arith: Boolean = false) {
        fun calc(): String = if (arith) "calc($text)" else text
    }

    fun decompile(): String {
        emitHeader()
        emitLocalDecls()
        // The compiler auto-appends a trailing RETURN; trim it so a re-compile
        // doesn't double it (mid-script early returns are kept).
        val end = if (n > 0 && script.opcodes[n - 1] == 21) n - 1 else n
        emit(0, end, 1)
        return out.toString()
    }

    private fun emitHeader() {
        // Clientscripts are id-referenced; emit a [clientscript,name] header
        // with int-then-string parameters in slot order.
        val params = buildList {
            for (i in 0 until script.intArgCount) add("int \$int$i")
            for (i in 0 until script.stringArgCount) add("string \$str$i")
        }
        out.append("[clientscript,").append(scriptName).append("]")
        if (params.isNotEmpty()) out.append("(").append(params.joinToString(", ")).append(")")
        out.append('\n')
    }

    private fun emitLocalDecls() {
        // Non-parameter locals get an explicit declaration so re-compilation
        // assigns the same slots.
        for (i in script.intArgCount until script.intLocalCount) line(1, "def_int \$int$i;")
        for (i in script.stringArgCount until script.stringLocalCount) line(1, "def_string \$str$i;")
    }

    /** Emit structured statements for the instruction range [start, end). */
    private fun emit(start: Int, end: Int, indent: Int) {
        val stack = ArrayDeque<Expr>()
        var pc = start
        while (pc < end) {
            val op = script.opcodes[pc]
            when {
                Cs2Opcodes.isConditionalBranch(op) -> { pc = emitIf(pc, end, indent, stack); }
                Cs2Opcodes.isUnconditionalBranch(op) -> {
                    val target = pc + 1 + script.intOperands[pc]
                    if (target <= pc) { /* back-edge handled by while */ }
                    pc++
                }
                op == 21 -> { line(indent, "return;"); pc++ }
                else -> { interpret(pc, indent, stack); pc++ }
            }
        }
    }

    /**
     * `if` / `if-else` / `while` reconstruction at a conditional branch.
     * Returns the pc just past the whole construct.
     */
    private fun emitIf(pc: Int, end: Int, indent: Int, stack: ArrayDeque<Expr>): Int {
        val b = stack.removeLastOrNull() ?: Expr("?")
        val a = stack.removeLastOrNull() ?: Expr("?")
        val op = script.opcodes[pc]
        val cond = "${a.text} ${inverseCmp(op)} ${b.text}"
        val skipTarget = pc + 1 + script.intOperands[pc] // jumped to when condition false

        // while: the body ends in a backward branch to this test.
        val beforeSkip = skipTarget - 1
        if (skipTarget in (pc + 1)..end && beforeSkip >= pc &&
            Cs2Opcodes.isUnconditionalBranch(script.opcodes.getOrElse(beforeSkip) { -1 }) &&
            (beforeSkip + 1 + script.intOperands[beforeSkip]) <= pc
        ) {
            line(indent, "while ($cond) {")
            emit(pc + 1, beforeSkip, indent + 1)
            line(indent, "}")
            return skipTarget
        }

        // if / if-else.
        val thenEnd: Int
        val elseRange: Pair<Int, Int>?
        val maybeBranch = skipTarget - 1
        if (maybeBranch >= pc + 1 && Cs2Opcodes.isUnconditionalBranch(script.opcodes.getOrElse(maybeBranch) { -1 })) {
            val joinTarget = maybeBranch + 1 + script.intOperands[maybeBranch]
            if (joinTarget > skipTarget) {
                thenEnd = maybeBranch
                elseRange = skipTarget to joinTarget
            } else { thenEnd = skipTarget; elseRange = null }
        } else { thenEnd = skipTarget; elseRange = null }

        line(indent, "if ($cond) {")
        emit(pc + 1, thenEnd, indent + 1)
        if (elseRange != null) {
            line(indent, "} else {")
            emit(elseRange.first, elseRange.second, indent + 1)
            line(indent, "}")
            return elseRange.second
        }
        line(indent, "}")
        return thenEnd
    }

    /** Symbolic execution of one non-branch instruction. */
    private fun interpret(pc: Int, indent: Int, stack: ArrayDeque<Expr>) {
        val op = script.opcodes[pc]
        val operand = script.intOperands[pc]
        when {
            op == 0 -> stack.addLast(Expr(operand.toString()))
            op == 3 -> stack.addLast(Expr("\"${script.stringOperands[pc]}\""))
            op == 33 -> stack.addLast(Expr("\$int$operand"))
            op == 35 -> stack.addLast(Expr("\$str$operand"))
            op == 34 -> line(indent, "\$int$operand = ${pop(stack)};")
            op == 36 -> line(indent, "\$str$operand = ${pop(stack)};")
            op == 1 -> stack.addLast(Expr(varName("varp", operand)))
            op == 2 -> line(indent, "${varName("varp", operand)} = ${pop(stack)};")
            op == 25 -> stack.addLast(Expr(varName("varbit", operand)))
            op == 27 -> line(indent, "${varName("varbit", operand)} = ${pop(stack)};")
            op == 42 -> stack.addLast(Expr("%varc_int$operand"))
            op == 43 -> line(indent, "%varc_int$operand = ${pop(stack)};")
            op == 47 -> stack.addLast(Expr("%varc_str$operand"))
            op == 48 -> line(indent, "%varc_str$operand = ${pop(stack)};")
            op == 37 -> joinString(operand, stack)
            op == 38 || op == 39 -> { /* discard: the produced value goes unused */ stack.removeLastOrNull() }
            op == 40 -> gosub(operand, indent, stack)
            op in ARITH -> arith(op, stack)
            Cs2Opcodes.isCommand(op) -> command(op, indent, stack)
            else -> line(indent, "// unhandled op $op (${Cs2Opcodes.name(op)})")
        }
    }

    private fun arith(op: Int, stack: ArrayDeque<Expr>) {
        val b = pop(stack); val a = pop(stack)
        stack.addLast(Expr("$a ${ARITH[op]} $b", arith = true))
    }

    private fun joinString(count: Int, stack: ArrayDeque<Expr>) {
        val parts = (0 until count).map { stack.removeLastOrNull()?.text ?: "?" }.reversed()
        // Render as a single interpolated string literal.
        val sb = StringBuilder("\"")
        for (p in parts) {
            if (p.startsWith("\"") && p.endsWith("\"")) sb.append(p.substring(1, p.length - 1))
            else sb.append("<").append(p).append(">")
        }
        sb.append("\"")
        stack.addLast(Expr(sb.toString()))
    }

    private fun gosub(id: Int, indent: Int, stack: ArrayDeque<Expr>) {
        val name = scriptNames(id) ?: "script$id"
        // Without the callee signature we cannot know its arity; emit as a
        // statement consuming nothing (the common void-proc case).
        line(indent, "~$name;")
    }

    private fun command(op: Int, indent: Int, stack: ArrayDeque<Expr>) {
        val meta = Cs2Opcodes.meta(op)
        val name = meta?.name ?: "op$op"
        val intD = meta?.intDelta
        val strD = meta?.strDelta
        if (intD == null || strD == null) {
            line(indent, "$name(...); // variable arity — args not recovered")
            return
        }
        // Void command: net delta == popped count (nothing pushed).
        val pops = (-intD) + (-strD)
        if (pops < 0) {
            // Net push (a getter) — surface as an expression with unknown args.
            stack.addLast(Expr("$name()"))
            return
        }
        val args = (0 until pops).map { pop(stack) }.reversed()
        line(indent, "$name(${args.joinToString(", ")});")
    }

    private fun pop(stack: ArrayDeque<Expr>): String = (stack.removeLastOrNull() ?: Expr("?")).calc()

    private fun varName(type: String, id: Int): String {
        val name = symbols?.config(type, idLookup = id)
        return if (name != null) "%$name" else "%$type$id"
    }

    private fun inverseCmp(op: Int): String = when (op) {
        7 -> "="    // branch_not jumps on !=, so condition is ==
        8 -> "!"    // branch_equals jumps on ==, condition is !=
        9 -> ">="   // branch_lt jumps on <, condition is >=
        10 -> "<="  // branch_gt jumps on >, condition is <=
        31 -> ">"   // branch_le jumps on <=, condition is >
        32 -> "<"   // branch_ge jumps on >=, condition is <
        else -> "?"
    }

    private fun line(indent: Int, text: String) {
        repeat(indent) { out.append("    ") }
        out.append(text).append('\n')
    }

    private companion object {
        val ARITH = mapOf(4000 to "+", 4001 to "-", 4002 to "*", 4003 to "/", 4011 to "%", 4014 to "&", 4015 to "|")
    }
}

/** Reverse config lookup by id (added for the decompiler's var naming). */
private fun SymbolTable.config(type: String, idLookup: Int): String? =
    configs[type]?.values?.firstOrNull { it.id == idLookup }?.name
