package com.os1.runescript.frontend.lexer

/**
 * Lexical token kinds for RuneScript. Mirrors the ANTLR `RuneScriptLexer`
 * grammar (the Lost City / Neptune lexer that RuneScriptTS re-implements),
 * kept faithful so the IntelliJ plugin can reuse this same token set for
 * highlighting.
 */
enum class TokenType {
    // punctuation
    LPAREN, RPAREN, COLON, SEMICOLON, COMMA, LBRACK, RBRACK, LBRACE, RBRACE,

    // operators
    PLUS, MINUS, MUL, DIV, DOTMOD, MOD, AND, OR, EQ, EXCL,
    DOLLAR, CARET, TILDE, AT, GT, GTE, LT, LTE,

    // keywords
    IF, ELSE, WHILE, CASE, DEFAULT, RETURN, CALC,

    // compound type keywords
    TYPE_ARRAY,   // `<ident>array`   e.g. `intarray`
    DEF_TYPE,     // `def_<ident>`    e.g. `def_int`
    SWITCH_TYPE,  // `switch_<ident>` e.g. `switch_int`

    // literals
    INTEGER_LITERAL, HEX_LITERAL, BIN_LITERAL,
    COORD_LITERAL, MAPZONE_LITERAL, BOOLEAN_LITERAL, CHAR_LITERAL, NULL_LITERAL,

    IDENTIFIER,

    // string mode
    QUOTE_OPEN, QUOTE_CLOSE, STRING_TEXT, STRING_TAG,
    STRING_EXPR_START, STRING_EXPR_END,

    EOF,
}

/** A character span in the source, used for diagnostics and IDE navigation. */
data class SourceSpan(val start: Int, val end: Int, val line: Int, val column: Int) {
    companion object {
        val NONE = SourceSpan(0, 0, 0, 0)
    }
}

data class Token(
    val type: TokenType,
    /** The raw matched text. For strings this is the decoded text. */
    val text: String,
    val span: SourceSpan,
) {
    override fun toString(): String = "$type(${text.replace("\n", "\\n")})"
}
