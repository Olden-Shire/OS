package com.os1.runescript.compiler.codegen

import com.os1.runescript.frontend.ast.*
import com.os1.runescript.frontend.diagnostics.Diagnostics
import com.os1.runescript.frontend.lexer.SourceSpan
import com.os1.runescript.frontend.symbol.*

/**
 * Lowers a `ScriptNode` to bytecode. Faithful to the RuneScriptTS code
 * generator for the language subset needed by hand-authored server content:
 * command/proc/jump calls, locals, assignments, `if`/`while` with
 * short-circuit `&`/`|`, `calc` arithmetic, switches, and literals.
 */
class CodeGenerator(
    private val symbols: SymbolTable,
    private val diagnostics: Diagnostics,
    private val profile: OpcodeProfile = OpcodeProfile.SERVER,
) {
    fun generate(node: ScriptNode, symbol: ScriptSymbol, lookupKey: Long, sourceName: String): CompiledScript {
        return ScriptGen(node, symbol, lookupKey, sourceName).run()
    }

    private inner class ScriptGen(
        val node: ScriptNode,
        val symbol: ScriptSymbol,
        val lookupKey: Long,
        val sourceName: String,
    ) {
        val instructions = mutableListOf<Instr>()
        val switchTables = mutableListOf<SwitchTable>()
        val locals = HashMap<String, LocalVariableSymbol>()
        var intLocals = 0
        var stringLocals = 0
        var intArgs = 0
        var stringArgs = 0
        var curLine = -1

        fun run(): CompiledScript {
            for (p in node.parameters) {
                val isString = p.type == "string"
                declareLocal(p.name, p.type, isArray = p.type.endsWith("array"), isParam = true)
                if (isString) stringArgs++ else intArgs++
            }
            for (s in node.statements) genStatement(s)
            // Implicit return at the end of every script.
            emit(Instr.IntOp(ServerScriptOpcode.RETURN.id, 0, false))

            return CompiledScript(
                id = symbol.id,
                fullName = "[${node.trigger},${node.subject}]",
                sourceName = sourceName,
                lookupKey = lookupKey,
                parameterTypeCodes = emptyList(),
                instructions = instructions,
                switchTables = switchTables,
                intLocalCount = intLocals,
                stringLocalCount = stringLocals,
                intArgCount = intArgs,
                stringArgCount = stringArgs,
            )
        }

        // ── statements ────────────────────────────────────────────────

        fun genStatement(s: StatementNode) {
            curLine = s.span.line
            when (s) {
                is BlockStatement -> s.statements.forEach { genStatement(it) }
                is EmptyStatement -> {}
                is ExpressionStatement -> { genExpression(s.expression); /* void result assumed */ }
                is DeclarationStatement -> genDeclaration(s)
                is AssignmentStatement -> genAssignment(s)
                is IfStatement -> genIf(s)
                is WhileStatement -> genWhile(s)
                is ReturnStatement -> genReturn(s)
                is SwitchStatement -> genSwitch(s)
            }
        }

        fun genDeclaration(s: DeclarationStatement) {
            val isArray = s.type.endsWith("array")
            val local = declareLocal(s.name, s.type, isArray, isParam = false)
            if (isArray) {
                // def_Xarray $a(size); — size, then DEFINE_ARRAY (id<<16 | code).
                s.initializer?.let { genExpression(it) } ?: emit(pushInt(0))
                val code = typeChar(local.type.removeSuffix("array"))
                emit(Instr.IntOp(ServerScriptOpcode.DEFINE_ARRAY.id, (varId(local) shl 16) or code, true))
                return
            }
            s.initializer?.let {
                genExpression(it)
                emit(popLocal(local))
            }
        }

        fun genAssignment(s: AssignmentStatement) {
            s.values.forEach { genExpression(it) }
            // Pop into targets in reverse (stack is LIFO).
            for (t in s.targets.asReversed()) genStore(t)
        }

        fun genStore(target: ExpressionNode) {
            when (target) {
                is LocalVariableExpression -> emit(popLocal(resolveLocal(target.name, target.span)))
                is GameVariableExpression -> emit(popGameVar(target))
                is LocalArrayVariableExpression -> {
                    val local = resolveLocal(target.name, target.span)
                    genExpression(target.index)
                    emit(Instr.IntOp(ServerScriptOpcode.POP_ARRAY_INT.id, varId(local), true))
                }
                else -> err(target.span, "invalid assignment target")
            }
        }

        fun genIf(s: IfStatement) {
            val elseLabel = Label()
            genCondJump(s.condition, elseLabel, jumpWhenTrue = false)
            genStatement(s.then)
            val otherwise = s.otherwise
            if (otherwise != null) {
                val endLabel = Label()
                emit(Instr.BranchOp(ServerScriptOpcode.BRANCH.id, endLabel))
                place(elseLabel)
                genStatement(otherwise)
                place(endLabel)
            } else {
                place(elseLabel)
            }
        }

        fun genWhile(s: WhileStatement) {
            val start = Label()
            val end = Label()
            place(start)
            genCondJump(s.condition, end, jumpWhenTrue = false)
            genStatement(s.body)
            emit(Instr.BranchOp(ServerScriptOpcode.BRANCH.id, start))
            place(end)
        }

        fun genReturn(s: ReturnStatement) {
            s.values.forEach { genExpression(it) }
            emit(Instr.IntOp(ServerScriptOpcode.RETURN.id, 0, false))
        }

        fun genSwitch(s: SwitchStatement) {
            genExpression(s.subject)
            val table = SwitchTable(switchTables.size)
            switchTables += table
            emit(Instr.SwitchOp(table.id))
            val end = Label()
            var defaultLabel: Label? = null
            val caseLabels = s.cases.map { Label() }
            // After SWITCH, fall through to default (or end).
            // Emit a jump to default/end, then each case body.
            val afterSwitch = Label()
            emit(Instr.BranchOp(ServerScriptOpcode.BRANCH.id, afterSwitch))
            for ((i, c) in s.cases.withIndex()) {
                place(caseLabels[i])
                if (c.keys == null) defaultLabel = caseLabels[i]
                else for (k in c.keys!!) table.cases += constInt(k) to caseLabels[i]
                c.statements.forEach { genStatement(it) }
                emit(Instr.BranchOp(ServerScriptOpcode.BRANCH.id, end))
            }
            place(afterSwitch)
            defaultLabel?.let { emit(Instr.BranchOp(ServerScriptOpcode.BRANCH.id, it)) }
            place(end)
        }

        // ── conditions (short-circuit) ────────────────────────────────

        fun genCondJump(cond: ExpressionNode, label: Label, jumpWhenTrue: Boolean) {
            if (cond is BinaryExpression && (cond.operator == BinaryOp.AND || cond.operator == BinaryOp.OR)) {
                genLogical(cond, label, jumpWhenTrue)
                return
            }
            if (cond is BinaryExpression && cond.operator.isComparison()) {
                genExpression(cond.left)
                genExpression(cond.right)
                emit(Instr.BranchOp(branchOpcode(cond.operator, jumpWhenTrue), label))
                return
            }
            // plain expression: treat nonzero as true.
            genExpression(cond)
            emit(pushInt(0))
            val op = if (jumpWhenTrue) ServerScriptOpcode.BRANCH_NOT else ServerScriptOpcode.BRANCH_EQUALS
            emit(Instr.BranchOp(op.id, label))
        }

        fun genLogical(cond: BinaryExpression, label: Label, jumpWhenTrue: Boolean) {
            val and = cond.operator == BinaryOp.AND
            // De Morgan: short-circuit depends on (operator, jumpWhenTrue).
            if (and == jumpWhenTrue) {
                // AND+true or OR+false: need a "skip" intermediate.
                val skip = Label()
                genCondJump(cond.left, skip, !jumpWhenTrue)
                genCondJump(cond.right, label, jumpWhenTrue)
                place(skip)
            } else {
                // AND+false or OR+true: both branches target the same label.
                genCondJump(cond.left, label, jumpWhenTrue)
                genCondJump(cond.right, label, jumpWhenTrue)
            }
        }

        // ── expressions (push a value) ────────────────────────────────

        fun genExpression(e: ExpressionNode) {
            when (e) {
                is IntegerLiteral -> emit(pushInt(e.value))
                is BooleanLiteral -> emit(pushInt(if (e.value) 1 else 0))
                is NullLiteral -> emit(pushInt(-1))
                is CharLiteral -> emit(pushInt(e.value.code))
                is CoordLiteral -> emit(pushInt(packCoord(e.raw)))
                is StringLiteral -> emit(Instr.StrOp(e.value))
                is JoinedStringExpression -> genJoinedString(e)
                is ConstantVariableExpression -> genConstant(e)
                is GameVariableExpression -> emit(pushGameVar(e))
                is LocalVariableExpression -> emit(pushLocal(resolveLocal(e.name, e.span)))
                is LocalArrayVariableExpression -> {
                    val local = resolveLocal(e.name, e.span)
                    genExpression(e.index)
                    emit(Instr.IntOp(ServerScriptOpcode.PUSH_ARRAY_INT.id, varId(local), true))
                }
                is Identifier -> genIdentifier(e)
                is CommandCallExpression -> genCommandCall(e)
                is ProcCallExpression -> genProcCall(e)
                is JumpCallExpression -> genJumpCall(e)
                is CalcExpression -> genExpression(e.expression)
                is BinaryExpression -> genArithmetic(e)
            }
        }

        fun genArithmetic(e: BinaryExpression) {
            genExpression(e.left)
            genExpression(e.right)
            if (e.operator !in CALC_OPS) err(e.span, "operator ${e.operator.symbol} not valid in calc")
            emit(Instr.IntOp(profile.arithmetic(e.operator), 0, false))
        }

        fun genJoinedString(e: JoinedStringExpression) {
            for (part in e.parts) genExpression(part)
            emit(Instr.IntOp(ServerScriptOpcode.JOIN_STRING.id, e.parts.size, true))
        }

        fun genConstant(e: ConstantVariableExpression) {
            val c = symbols.constant(e.name)
            if (c == null) { err(e.span, "unknown constant ^${e.name}"); emit(pushInt(0)); return }
            val sv = c.stringValue
            if (sv != null) emit(Instr.StrOp(sv)) else emit(pushInt(c.intValue ?: 0))
        }

        fun genIdentifier(e: Identifier) {
            // A bare identifier resolves to a config id, a constant, or a
            // script reference used as a value. Supports `interface:component`.
            symbols.constant(e.name)?.let {
                val sv = it.stringValue
                if (sv != null) emit(Instr.StrOp(sv)) else emit(pushInt(it.intValue ?: 0))
                return
            }
            if (e.name.contains(':')) {
                val (iface, comp) = e.name.split(':', limit = 2)
                val ifaceSym = symbols.config("interface", iface)
                if (ifaceSym != null) {
                    val sub = comp.toIntOrNull() ?: symbols.config("interface", comp)?.id ?: 0
                    emit(pushInt((ifaceSym.id shl 16) or sub))
                    return
                }
            }
            symbols.config(e.name)?.let { emit(pushInt(it.id)); return }
            symbols.script(e.name)?.let { emit(pushInt(it.id)); return }
            err(e.span, "unknown identifier '${e.name}'")
            emit(pushInt(0))
        }

        fun genCommandCall(e: CommandCallExpression) {
            val cmd = symbols.command(e.name)
            if (cmd == null) { err(e.span, "unknown command '${e.name}'"); return }
            for (arg in e.arguments) genExpression(arg)
            emit(Instr.Command(cmd.opcode, secondary = false))
        }

        fun genProcCall(e: ProcCallExpression) {
            val target = symbols.script(e.name)
            if (target == null) { err(e.span, "unknown proc ~${e.name}"); return }
            for (arg in e.arguments) genExpression(arg)
            emit(Instr.IntOp(ServerScriptOpcode.GOSUB_WITH_PARAMS.id, target.id, true))
        }

        fun genJumpCall(e: JumpCallExpression) {
            val target = symbols.script(e.name)
            if (target == null) { err(e.span, "unknown jump @${e.name}"); return }
            for (arg in e.arguments) genExpression(arg)
            emit(Instr.IntOp(ServerScriptOpcode.JUMP_WITH_PARAMS.id, target.id, true))
        }

        // ── variables / locals ────────────────────────────────────────

        fun declareLocal(name: String, type: String, isArray: Boolean, isParam: Boolean): LocalVariableSymbol {
            val sym = LocalVariableSymbol(name, type, isArray, isParam)
            locals[name] = sym
            // Assign an id within its base-type sequence (no-array case).
            localId[sym] = if (type == "string") stringLocals++ else intLocals++
            return sym
        }

        fun resolveLocal(name: String, span: SourceSpan): LocalVariableSymbol =
            locals[name] ?: run { err(span, "unknown local \$$name"); declareLocal(name, "int", false, false) }

        fun pushLocal(local: LocalVariableSymbol): Instr =
            Instr.IntOp(
                if (local.isString) ServerScriptOpcode.PUSH_STRING_LOCAL.id else ServerScriptOpcode.PUSH_INT_LOCAL.id,
                varId(local), true,
            )

        fun popLocal(local: LocalVariableSymbol): Instr =
            Instr.IntOp(
                if (local.isString) ServerScriptOpcode.POP_STRING_LOCAL.id else ServerScriptOpcode.POP_INT_LOCAL.id,
                varId(local), true,
            )

        fun pushGameVar(e: GameVariableExpression): Instr {
            val v = symbols.variable(e.name) ?: run { err(e.span, "unknown var %${e.name}"); return pushInt(0) }
            val op = gameVarOp(v.kind, pop = false)
            val operand = v.id or (if (e.dot) (1 shl 16) else 0)
            return Instr.IntOp(op, operand, true)
        }

        fun popGameVar(e: GameVariableExpression): Instr {
            val v = symbols.variable(e.name) ?: run { err(e.span, "unknown var %${e.name}"); return Instr.IntOp(ServerScriptOpcode.POP_INT_DISCARD.id, 0, false) }
            val op = gameVarOp(v.kind, pop = true)
            val operand = v.id or (if (e.dot) (1 shl 16) else 0)
            return Instr.IntOp(op, operand, true)
        }

        fun varId(local: LocalVariableSymbol): Int = localId[local] ?: 0

        val localId = HashMap<LocalVariableSymbol, Int>()

        // ── helpers ───────────────────────────────────────────────────

        fun pushInt(v: Int) = Instr.IntOp(ServerScriptOpcode.PUSH_CONSTANT_INT.id, v, true)

        fun constInt(e: ExpressionNode): Int = when (e) {
            is IntegerLiteral -> e.value
            is BooleanLiteral -> if (e.value) 1 else 0
            is ConstantVariableExpression -> symbols.constant(e.name)?.intValue ?: 0
            is Identifier -> symbols.config(e.name)?.id ?: 0
            else -> { err(e.span, "switch case key must be a constant"); 0 }
        }

        fun emit(instr: Instr) {
            instr.line = curLine
            instructions += instr
        }

        fun place(label: Label) { label.target = instructions.size }

        fun err(span: SourceSpan, msg: String) = diagnostics.error(sourceName, span, msg)
    }

    private fun BinaryOp.isComparison(): Boolean = this in COMPARISONS

    /** Branch opcode for a comparison, inverted when [whenTrue] is false. */
    private fun branchOpcode(op: BinaryOp, whenTrue: Boolean): Int {
        val effective = if (whenTrue) op else op.inverse()
        return when (effective) {
            BinaryOp.EQUAL -> ServerScriptOpcode.BRANCH_EQUALS
            BinaryOp.NOT_EQUAL -> ServerScriptOpcode.BRANCH_NOT
            BinaryOp.LESS -> ServerScriptOpcode.BRANCH_LESS_THAN
            BinaryOp.GREATER -> ServerScriptOpcode.BRANCH_GREATER_THAN
            BinaryOp.LESS_EQUAL -> ServerScriptOpcode.BRANCH_LESS_THAN_OR_EQUALS
            BinaryOp.GREATER_EQUAL -> ServerScriptOpcode.BRANCH_GREATER_THAN_OR_EQUALS
            else -> ServerScriptOpcode.BRANCH_EQUALS
        }.id
    }

    private fun BinaryOp.inverse(): BinaryOp = when (this) {
        BinaryOp.EQUAL -> BinaryOp.NOT_EQUAL
        BinaryOp.NOT_EQUAL -> BinaryOp.EQUAL
        BinaryOp.LESS -> BinaryOp.GREATER_EQUAL
        BinaryOp.GREATER -> BinaryOp.LESS_EQUAL
        BinaryOp.LESS_EQUAL -> BinaryOp.GREATER
        BinaryOp.GREATER_EQUAL -> BinaryOp.LESS
        else -> this
    }

    private fun gameVarOp(kind: VarKind, pop: Boolean): Int = profile.gameVar(kind, pop)

    /** First char of a type name as the array element type code. */
    private fun typeChar(type: String): Int = when (type) {
        "int" -> 'i'.code
        "string" -> 's'.code
        else -> type.firstOrNull()?.code ?: 'i'.code
    }

    /** Pack a `level_mx_mz_lx_lz` coord literal into the runtime coord int. */
    private fun packCoord(raw: String): Int {
        val p = raw.split('_').map { it.toIntOrNull() ?: 0 }
        if (p.size != 5) return 0
        val (level, mx, mz, lx, lz) = p
        val x = mx * 64 + lx
        val z = mz * 64 + lz
        return (z and 0x3FFF) or ((x and 0x3FFF) shl 14) or ((level and 0x3) shl 28)
    }

    private companion object {
        val COMPARISONS = setOf(
            BinaryOp.EQUAL, BinaryOp.NOT_EQUAL, BinaryOp.LESS,
            BinaryOp.GREATER, BinaryOp.LESS_EQUAL, BinaryOp.GREATER_EQUAL,
        )
        val CALC_OPS = setOf(
            BinaryOp.ADD, BinaryOp.SUB, BinaryOp.MUL, BinaryOp.DIV,
            BinaryOp.MOD, BinaryOp.AND, BinaryOp.OR,
        )
    }
}
