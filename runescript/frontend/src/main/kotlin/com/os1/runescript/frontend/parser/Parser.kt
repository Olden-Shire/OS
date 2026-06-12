package com.os1.runescript.frontend.parser

import com.os1.runescript.frontend.ast.*
import com.os1.runescript.frontend.diagnostics.CompileException
import com.os1.runescript.frontend.diagnostics.Diagnostics
import com.os1.runescript.frontend.lexer.SourceSpan
import com.os1.runescript.frontend.lexer.Token
import com.os1.runescript.frontend.lexer.TokenType
import com.os1.runescript.frontend.lexer.TokenType.*

/**
 * Recursive-descent parser faithful to `RuneScriptParser.g4`. Conditions and
 * `calc` arithmetic use precedence climbing with the grammar's operator
 * precedence.
 */
class Parser(
    private val tokens: List<Token>,
    private val sourceName: String,
    private val diagnostics: Diagnostics,
) {
    private var pos = 0

    fun parseFile(): ScriptFileNode {
        val start = peek().span
        val scripts = mutableListOf<ScriptNode>()
        while (!check(EOF)) {
            try {
                scripts += parseScript()
            } catch (e: CompileException) {
                diagnostics.error(sourceName, peek().span, e.message ?: "parse error")
                recover()
            }
        }
        return ScriptFileNode(scripts, spanFrom(start))
    }

    // ── script header ─────────────────────────────────────────────────

    private fun parseScript(): ScriptNode {
        val start = expect(LBRACK).span
        val triggerTok = expectIdentifier("trigger")
        expect(COMMA)
        // scriptName: identifier (identifier)*
        val subjectStart = peek().span
        val sb = StringBuilder()
        while (isIdentifierLike() && !check(MUL) && !check(RBRACK)) {
            sb.append(next().text)
        }
        if (sb.isEmpty()) throw CompileException("expected script subject")
        val global = match(MUL)
        expect(RBRACK)

        val parameters = mutableListOf<ParameterNode>()
        val returnTypes = mutableListOf<String>()
        if (check(LPAREN)) {
            expect(LPAREN)
            if (!check(RPAREN)) parseParameterList(parameters)
            expect(RPAREN)
            if (check(LPAREN)) {
                expect(LPAREN)
                if (!check(RPAREN)) {
                    returnTypes += next().text
                    while (match(COMMA)) returnTypes += next().text
                }
                expect(RPAREN)
            }
        }

        val statements = mutableListOf<StatementNode>()
        while (!check(LBRACK) && !check(EOF)) {
            statements += parseStatement()
        }
        return ScriptNode(
            triggerTok.text, triggerTok.span, sb.toString(), subjectStart, global,
            parameters, returnTypes, statements, spanFrom(start),
        )
    }

    private fun parseParameterList(out: MutableList<ParameterNode>) {
        do {
            val typeTok = next() // IDENTIFIER or TYPE_ARRAY
            expect(DOLLAR)
            val nameTok = next()
            out += ParameterNode(typeTok.text, nameTok.text, spanFrom(typeTok.span))
        } while (match(COMMA))
    }

    // ── statements ────────────────────────────────────────────────────

    private fun parseStatement(): StatementNode {
        return when (peek().type) {
            LBRACE -> parseBlock()
            RETURN -> parseReturn()
            IF -> parseIf()
            WHILE -> parseWhile()
            SWITCH_TYPE -> parseSwitch()
            DEF_TYPE -> parseDeclaration()
            SEMICOLON -> { val s = next().span; EmptyStatement(s) }
            DOLLAR, MOD, DOTMOD -> parseAssignmentOrExpression()
            else -> parseExpressionStatement()
        }
    }

    private fun parseBlock(): StatementNode {
        val start = expect(LBRACE).span
        val stmts = mutableListOf<StatementNode>()
        while (!check(RBRACE) && !check(EOF)) stmts += parseStatement()
        expect(RBRACE)
        return BlockStatement(stmts, spanFrom(start))
    }

    private fun parseReturn(): StatementNode {
        val start = expect(RETURN).span
        val values = mutableListOf<ExpressionNode>()
        if (match(LPAREN)) {
            if (!check(RPAREN)) {
                values += parseExpression()
                while (match(COMMA)) values += parseExpression()
            }
            expect(RPAREN)
        }
        expect(SEMICOLON)
        return ReturnStatement(values, spanFrom(start))
    }

    private fun parseIf(): StatementNode {
        val start = expect(IF).span
        expect(LPAREN)
        val cond = parseCondition()
        expect(RPAREN)
        val then = parseStatement()
        val otherwise = if (match(ELSE)) parseStatement() else null
        return IfStatement(cond, then, otherwise, spanFrom(start))
    }

    private fun parseWhile(): StatementNode {
        val start = expect(WHILE).span
        expect(LPAREN)
        val cond = parseCondition()
        expect(RPAREN)
        val body = parseStatement()
        return WhileStatement(cond, body, spanFrom(start))
    }

    private fun parseSwitch(): StatementNode {
        val typeTok = expect(SWITCH_TYPE)
        val type = typeTok.text.removePrefix("switch_")
        expect(LPAREN)
        val subject = parseExpression()
        expect(RPAREN)
        expect(LBRACE)
        val cases = mutableListOf<SwitchCaseNode>()
        while (check(CASE)) {
            val caseStart = expect(CASE).span
            val keys: List<ExpressionNode>? = if (match(DEFAULT)) {
                null
            } else {
                val ks = mutableListOf(parseExpression())
                while (match(COMMA)) ks += parseExpression()
                ks
            }
            expect(COLON)
            val stmts = mutableListOf<StatementNode>()
            while (!check(CASE) && !check(RBRACE) && !check(EOF)) stmts += parseStatement()
            cases += SwitchCaseNode(keys, stmts, spanFrom(caseStart))
        }
        expect(RBRACE)
        return SwitchStatement(type, subject, cases, spanFrom(typeTok.span))
    }

    private fun parseDeclaration(): StatementNode {
        val typeTok = expect(DEF_TYPE)
        val type = typeTok.text.removePrefix("def_")
        expect(DOLLAR)
        val name = next().text
        // array declaration: def_TYPE $name (size);
        if (check(LPAREN)) {
            expect(LPAREN)
            val size = parseExpression()
            expect(RPAREN)
            expect(SEMICOLON)
            // model array decl as a declaration with the size as initializer
            return DeclarationStatement(type + "array", name, size, spanFrom(typeTok.span))
        }
        val init = if (match(EQ)) parseExpression() else null
        expect(SEMICOLON)
        return DeclarationStatement(type, name, init, spanFrom(typeTok.span))
    }

    private fun parseAssignmentOrExpression(): StatementNode {
        val start = peek().span
        val first = parseVariable()
        if (check(EQ) || check(COMMA)) {
            val targets = mutableListOf(first)
            while (match(COMMA)) targets += parseVariable()
            expect(EQ)
            val values = mutableListOf(parseExpression())
            while (match(COMMA)) values += parseExpression()
            expect(SEMICOLON)
            return AssignmentStatement(targets, values, spanFrom(start))
        }
        expect(SEMICOLON)
        return ExpressionStatement(first, spanFrom(start))
    }

    private fun parseExpressionStatement(): StatementNode {
        val start = peek().span
        val expr = parseExpression()
        expect(SEMICOLON)
        return ExpressionStatement(expr, spanFrom(start))
    }

    // ── conditions (if/while) — precedence climbing ───────────────────

    private fun parseCondition(): ExpressionNode = parseConditionOr()

    private fun parseConditionOr(): ExpressionNode {
        var left = parseConditionAnd()
        while (check(OR)) {
            val op = next(); val right = parseConditionAnd()
            left = BinaryExpression(left, BinaryOp.OR, right, spanFrom(left.span))
        }
        return left
    }

    private fun parseConditionAnd(): ExpressionNode {
        var left = parseConditionEquality()
        while (check(AND)) {
            next(); val right = parseConditionEquality()
            left = BinaryExpression(left, BinaryOp.AND, right, spanFrom(left.span))
        }
        return left
    }

    private fun parseConditionEquality(): ExpressionNode {
        var left = parseConditionRelational()
        while (check(EQ) || check(EXCL)) {
            val op = if (next().type == EQ) BinaryOp.EQUAL else BinaryOp.NOT_EQUAL
            val right = parseConditionRelational()
            left = BinaryExpression(left, op, right, spanFrom(left.span))
        }
        return left
    }

    private fun parseConditionRelational(): ExpressionNode {
        var left = parseConditionPrimary()
        while (check(LT) || check(GT) || check(LTE) || check(GTE)) {
            val op = when (next().type) {
                LT -> BinaryOp.LESS; GT -> BinaryOp.GREATER; LTE -> BinaryOp.LESS_EQUAL; else -> BinaryOp.GREATER_EQUAL
            }
            val right = parseConditionPrimary()
            left = BinaryExpression(left, op, right, spanFrom(left.span))
        }
        return left
    }

    private fun parseConditionPrimary(): ExpressionNode {
        if (check(LPAREN)) {
            expect(LPAREN)
            val c = parseCondition()
            expect(RPAREN)
            return c
        }
        return parseExpression()
    }

    // ── calc arithmetic — precedence climbing ─────────────────────────

    private fun parseArithmeticOr(): ExpressionNode {
        var left = parseArithmeticAnd()
        while (check(OR)) { next(); left = BinaryExpression(left, BinaryOp.OR, parseArithmeticAnd(), spanFrom(left.span)) }
        return left
    }

    private fun parseArithmeticAnd(): ExpressionNode {
        var left = parseArithmeticAdditive()
        while (check(AND)) { next(); left = BinaryExpression(left, BinaryOp.AND, parseArithmeticAdditive(), spanFrom(left.span)) }
        return left
    }

    private fun parseArithmeticAdditive(): ExpressionNode {
        var left = parseArithmeticMultiplicative()
        while (check(PLUS) || check(MINUS)) {
            val op = if (next().type == PLUS) BinaryOp.ADD else BinaryOp.SUB
            left = BinaryExpression(left, op, parseArithmeticMultiplicative(), spanFrom(left.span))
        }
        return left
    }

    private fun parseArithmeticMultiplicative(): ExpressionNode {
        var left = parseArithmeticPrimary()
        while (check(MUL) || check(DIV) || check(MOD)) {
            val op = when (next().type) { MUL -> BinaryOp.MUL; DIV -> BinaryOp.DIV; else -> BinaryOp.MOD }
            left = BinaryExpression(left, op, parseArithmeticPrimary(), spanFrom(left.span))
        }
        return left
    }

    private fun parseArithmeticPrimary(): ExpressionNode {
        if (check(LPAREN)) {
            expect(LPAREN)
            val a = parseArithmeticOr()
            expect(RPAREN)
            return a
        }
        return parseExpression()
    }

    // ── expressions (primary, no binary outside calc/condition) ───────

    private fun parseExpression(): ExpressionNode {
        val tok = peek()
        return when (tok.type) {
            LPAREN -> { expect(LPAREN); val e = parseExpression(); expect(RPAREN); e }
            CALC -> parseCalc()
            TILDE -> parseProcCall()
            AT -> parseJumpCall()
            DOLLAR, MOD, DOTMOD -> parseVariable()
            CARET -> { expect(CARET); val n = next(); ConstantVariableExpression(n.text, spanFrom(tok.span)) }
            QUOTE_OPEN -> parseString()
            INTEGER_LITERAL -> { next(); IntegerLiteral(parseInt(tok.text), tok.span) }
            HEX_LITERAL -> { next(); IntegerLiteral(parseHex(tok.text), tok.span) }
            BIN_LITERAL -> { next(); IntegerLiteral(tok.text.substring(2).toInt(2), tok.span) }
            COORD_LITERAL -> { next(); CoordLiteral(tok.text, tok.span) }
            BOOLEAN_LITERAL -> { next(); BooleanLiteral(tok.text == "true", tok.span) }
            CHAR_LITERAL -> { next(); CharLiteral(decodeChar(tok.text), tok.span) }
            NULL_LITERAL -> { next(); NullLiteral(tok.span) }
            else -> parseIdentifierOrCall()
        }
    }

    private fun parseCalc(): ExpressionNode {
        val start = expect(CALC).span
        expect(LPAREN)
        val inner = parseArithmeticOr()
        expect(RPAREN)
        return CalcExpression(inner, spanFrom(start))
    }

    private fun parseProcCall(): ExpressionNode {
        val start = expect(TILDE).span
        val name = next().text
        val args = parseOptionalArgs()
        return ProcCallExpression(name, args, spanFrom(start))
    }

    private fun parseJumpCall(): ExpressionNode {
        val start = expect(AT).span
        val name = next().text
        val args = parseOptionalArgs()
        return JumpCallExpression(name, args, spanFrom(start))
    }

    private fun parseVariable(): ExpressionNode {
        val tok = next()
        return when (tok.type) {
            DOLLAR -> {
                val name = next().text
                if (check(LPAREN)) {
                    expect(LPAREN); val idx = parseExpression(); expect(RPAREN)
                    LocalArrayVariableExpression(name, idx, spanFrom(tok.span))
                } else LocalVariableExpression(name, spanFrom(tok.span))
            }
            MOD -> GameVariableExpression(next().text, dot = false, spanFrom(tok.span))
            DOTMOD -> GameVariableExpression(next().text, dot = true, spanFrom(tok.span))
            else -> throw CompileException("expected a variable, got ${tok.type}")
        }
    }

    private fun parseIdentifierOrCall(): ExpressionNode {
        if (!isIdentifierLike()) throw CompileException("unexpected token ${peek().type}")
        val tok = next()
        // command with clientscript args: name*(args)(triggers)
        if (check(MUL) && peekAt(1)?.type == LPAREN) {
            next() // '*'
            val args = parseArgs()
            val triggers = parseArgs()
            return CommandCallExpression(tok.text, args, triggers, spanFrom(tok.span))
        }
        if (check(LPAREN)) {
            val args = parseArgs()
            return CommandCallExpression(tok.text, args, null, spanFrom(tok.span))
        }
        return Identifier(tok.text, tok.span)
    }

    private fun parseString(): ExpressionNode {
        val start = expect(QUOTE_OPEN).span
        val parts = mutableListOf<ExpressionNode>()
        val text = StringBuilder()
        var interpolated = false
        while (!check(QUOTE_CLOSE) && !check(EOF)) {
            when (peek().type) {
                STRING_TEXT -> text.append(next().text)
                STRING_TAG -> text.append(next().text)
                STRING_EXPR_START -> {
                    interpolated = true
                    if (text.isNotEmpty()) { parts += StringLiteral(text.toString(), start); text.clear() }
                    expect(STRING_EXPR_START)
                    parts += parseExpression()
                    expect(STRING_EXPR_END)
                }
                else -> throw CompileException("unexpected token in string: ${peek().type}")
            }
        }
        expect(QUOTE_CLOSE)
        if (!interpolated) return StringLiteral(text.toString(), spanFrom(start))
        if (text.isNotEmpty()) parts += StringLiteral(text.toString(), start)
        return JoinedStringExpression(parts, spanFrom(start))
    }

    private fun parseArgs(): List<ExpressionNode> {
        expect(LPAREN)
        val args = mutableListOf<ExpressionNode>()
        if (!check(RPAREN)) {
            args += parseExpression()
            while (match(COMMA)) args += parseExpression()
        }
        expect(RPAREN)
        return args
    }

    private fun parseOptionalArgs(): List<ExpressionNode> =
        if (check(LPAREN)) parseArgs() else emptyList()

    // ── token helpers ─────────────────────────────────────────────────

    private fun isIdentifierLike(): Boolean = peek().type in IDENTIFIER_LIKE

    private fun peek(): Token = tokens[pos]
    private fun peekAt(n: Int): Token? = tokens.getOrNull(pos + n)
    private fun next(): Token = tokens[pos++]
    private fun check(type: TokenType): Boolean = peek().type == type
    private fun match(type: TokenType): Boolean = if (check(type)) { pos++; true } else false

    private fun expect(type: TokenType): Token {
        if (!check(type)) throw CompileException("expected $type but found ${peek().type} ('${peek().text}')")
        return next()
    }

    private fun expectIdentifier(what: String): Token {
        if (!isIdentifierLike()) throw CompileException("expected $what identifier, found ${peek().type}")
        return next()
    }

    private fun recover() {
        while (!check(EOF) && !check(LBRACK)) pos++
    }

    private fun spanFrom(start: SourceSpan): SourceSpan {
        val end = if (pos > 0) tokens[pos - 1].span.end else start.end
        return SourceSpan(start.start, end, start.line, start.column)
    }

    private fun parseInt(text: String): Int = text.toLongOrNull()?.toInt() ?: 0
    private fun parseHex(text: String): Int = text.substring(2).toLong(16).toInt()

    private fun decodeChar(raw: String): Char {
        val inner = raw.substring(1, raw.length - 1)
        return if (inner.startsWith("\\") && inner.length >= 2) inner[1] else inner.firstOrNull() ?: ' '
    }

    companion object {
        private val IDENTIFIER_LIKE = setOf(
            IDENTIFIER, HEX_LITERAL, BOOLEAN_LITERAL, NULL_LITERAL,
            COORD_LITERAL, MAPZONE_LITERAL, TYPE_ARRAY, SWITCH_TYPE, DEF_TYPE, DEFAULT,
        )
    }
}
