package com.os.runescript.frontend.lexer

import com.os.runescript.frontend.diagnostics.Diagnostics

/**
 * Hand-written lexer faithful to `RuneScriptLexer.g4`. Produces a flat token
 * stream (string interpolation is emitted inline via STRING_EXPR_START/END
 * with a mode stack, exactly like the ANTLR lexer's `pushMode`/`popMode`).
 */
class Lexer(
    private val src: String,
    private val sourceName: String,
    private val diagnostics: Diagnostics,
) {
    private var pos = 0
    private var line = 1
    private var lineStart = 0

    // Mode stack: false = default, true = inside a string literal.
    private val stringMode = ArrayDeque<Boolean>().apply { addLast(false) }
    // String nesting depth (for `<expr>` interpolation that re-enters default).
    private var depth = 0

    private val tokens = mutableListOf<Token>()

    fun tokenize(): List<Token> {
        while (pos < src.length) {
            if (stringMode.last()) lexStringMode() else lexDefaultMode()
        }
        tokens += Token(TokenType.EOF, "", span(pos, pos))
        return tokens
    }

    // ── default mode ──────────────────────────────────────────────────

    private fun lexDefaultMode() {
        skipTrivia()
        if (pos >= src.length) return
        val c = src[pos]
        val start = pos

        when {
            c == '"' -> { advance(); emit(TokenType.QUOTE_OPEN, "\"", start); depth++; pushString() }
            c == '(' -> punct(TokenType.LPAREN, start)
            c == ')' -> punct(TokenType.RPAREN, start)
            c == ':' -> punct(TokenType.COLON, start)
            c == ';' -> punct(TokenType.SEMICOLON, start)
            c == ',' -> punct(TokenType.COMMA, start)
            c == '[' -> punct(TokenType.LBRACK, start)
            c == ']' -> punct(TokenType.RBRACK, start)
            c == '{' -> punct(TokenType.LBRACE, start)
            c == '}' -> punct(TokenType.RBRACE, start)
            c == '+' -> punct(TokenType.PLUS, start)
            c == '*' -> punct(TokenType.MUL, start)
            c == '/' -> punct(TokenType.DIV, start)
            c == '&' -> punct(TokenType.AND, start)
            c == '|' -> punct(TokenType.OR, start)
            c == '=' -> punct(TokenType.EQ, start)
            c == '!' -> punct(TokenType.EXCL, start)
            c == '$' -> punct(TokenType.DOLLAR, start)
            c == '^' -> punct(TokenType.CARET, start)
            c == '~' -> punct(TokenType.TILDE, start)
            c == '@' -> punct(TokenType.AT, start)
            c == '%' -> punct(TokenType.MOD, start)
            c == '.' && peek(1) == '%' -> { advance(); advance(); emit(TokenType.DOTMOD, ".%", start) }
            c == '>' -> {
                // Inside an interpolation expression `>` closes it.
                if (depth > 0) { advance(); emit(TokenType.STRING_EXPR_END, ">", start); popToString() }
                else if (peek(1) == '=') { advance(); advance(); emit(TokenType.GTE, ">=", start) }
                else punct(TokenType.GT, start)
            }
            c == '<' && peek(1) == '=' -> { advance(); advance(); emit(TokenType.LTE, "<=", start) }
            c == '<' -> punct(TokenType.LT, start)
            c == '-' && peek(1)?.isDigit() == true -> lexNumberWord(start) // negative integer
            c == '\'' -> lexCharLiteral(start)
            isWordChar(c) -> lexWord(start)
            else -> {
                diagnostics.error(sourceName, span(start, pos + 1), "unexpected character '$c'")
                advance()
            }
        }
    }

    private fun punct(type: TokenType, start: Int) {
        advance()
        emit(type, src.substring(start, pos), start)
    }

    /** A maximal run of identifier/number characters, then classified. */
    private fun lexWord(start: Int) {
        while (pos < src.length && isWordChar(src[pos])) advance()
        classifyWord(start)
    }

    /** Negative integer: `-` already at start, consume `-` then digits. */
    private fun lexNumberWord(start: Int) {
        advance() // '-'
        while (pos < src.length && src[pos].isDigit()) advance()
        emit(TokenType.INTEGER_LITERAL, src.substring(start, pos), start)
    }

    private fun classifyWord(start: Int) {
        val text = src.substring(start, pos)
        val type = when {
            text == "if" -> TokenType.IF
            text == "else" -> TokenType.ELSE
            text == "while" -> TokenType.WHILE
            text == "case" -> TokenType.CASE
            text == "default" -> TokenType.DEFAULT
            text == "return" -> TokenType.RETURN
            text == "calc" -> TokenType.CALC
            text == "true" || text == "false" -> TokenType.BOOLEAN_LITERAL
            text == "null" -> TokenType.NULL_LITERAL
            HEX.matches(text) -> TokenType.HEX_LITERAL
            BIN.matches(text) -> TokenType.BIN_LITERAL
            COORD.matches(text) -> TokenType.COORD_LITERAL
            MAPZONE.matches(text) -> TokenType.MAPZONE_LITERAL
            INTEGER.matches(text) -> TokenType.INTEGER_LITERAL
            text.startsWith("def_") && text.length > 4 -> TokenType.DEF_TYPE
            text.startsWith("switch_") && text.length > 7 -> TokenType.SWITCH_TYPE
            text.endsWith("array") && text.length > 5 -> TokenType.TYPE_ARRAY
            else -> TokenType.IDENTIFIER
        }
        emit(type, text, start)
    }

    private fun lexCharLiteral(start: Int) {
        advance() // opening '
        if (pos < src.length && src[pos] == '\\') advance()
        if (pos < src.length) advance() // the char
        if (pos < src.length && src[pos] == '\'') advance() // closing '
        emit(TokenType.CHAR_LITERAL, src.substring(start, pos), start)
    }

    // ── string mode ───────────────────────────────────────────────────

    private fun lexStringMode() {
        val start = pos
        val c = src[pos]
        when {
            c == '"' -> { advance(); emit(TokenType.QUOTE_CLOSE, "\"", start); depth--; popString() }
            c == '<' -> lexStringAngle(start)
            else -> {
                val sb = StringBuilder()
                while (pos < src.length) {
                    val ch = src[pos]
                    if (ch == '"' || ch == '<') break
                    if (ch == '\\' && pos + 1 < src.length) {
                        val n = src[pos + 1]
                        if (n == '\\' || n == '"' || n == '<') { sb.append(n); advance(); advance(); continue }
                    }
                    if (ch == '\n') { newline() } else sb.append(ch)
                    advance()
                }
                emit(TokenType.STRING_TEXT, sb.toString(), start)
            }
        }
    }

    /** A `<...>` inside a string: a formatting tag (kept as text) or `<expr>`. */
    private fun lexStringAngle(start: Int) {
        // Lookahead for a known tag: `<tag...>` or `</tag>`.
        val rest = src.substring(pos)
        val tag = TAG_PATTERN.find(rest)
        if (tag != null && tag.range.first == 0) {
            // Treat the whole tag as literal string text (runtime parses it).
            repeat(tag.value.length) { advance() }
            emit(TokenType.STRING_TAG, tag.value, start)
        } else {
            advance() // '<'
            emit(TokenType.STRING_EXPR_START, "<", start)
            pushDefaultFromString()
        }
    }

    // ── mode stack helpers ────────────────────────────────────────────

    private fun pushString() { stringMode.addLast(true) }
    private fun popString() { stringMode.removeLast() }
    private fun pushDefaultFromString() { stringMode.addLast(false) }
    private fun popToString() { stringMode.removeLast() }

    // ── trivia ────────────────────────────────────────────────────────

    private fun skipTrivia() {
        while (pos < src.length) {
            val c = src[pos]
            when {
                c == '\n' -> { newline(); advance() }
                c == ' ' || c == '\t' || c == '\r' -> advance()
                c == '/' && peek(1) == '/' -> { while (pos < src.length && src[pos] != '\n') advance() }
                c == '/' && peek(1) == '*' -> {
                    advance(); advance()
                    while (pos < src.length && !(src[pos] == '*' && peek(1) == '/')) {
                        if (src[pos] == '\n') newline()
                        advance()
                    }
                    if (pos < src.length) { advance(); advance() }
                }
                else -> return
            }
        }
    }

    // ── primitives ────────────────────────────────────────────────────

    private fun isWordChar(c: Char): Boolean =
        c in 'a'..'z' || c in 'A'..'Z' || c in '0'..'9' || c == '_' || c == '+' || c == '.' || c == ':'

    private fun peek(n: Int): Char? = src.getOrNull(pos + n)
    private fun advance() { pos++ }
    private fun newline() { line++; lineStart = pos + 1 }

    private fun emit(type: TokenType, text: String, start: Int) {
        tokens += Token(type, text, span(start, pos))
    }

    private fun span(start: Int, end: Int): SourceSpan =
        SourceSpan(start, end, line, start - lineStart + 1)

    companion object {
        private val HEX = Regex("0[xX][0-9a-fA-F]+")
        private val BIN = Regex("0[bB][01]+")
        private val INTEGER = Regex("-?[0-9]+")
        private val COORD = Regex("[0-9]+_[0-9]+_[0-9]+_[0-9]+_[0-9]+")
        private val MAPZONE = Regex("[0-9]+_[0-9]+_[0-9]+")
        private val TAG_PATTERN = Regex("^</?(br|col|str|shad|u|img|gt|lt|p)(=[^<>]*|,[^<>]*)?>")
    }
}
