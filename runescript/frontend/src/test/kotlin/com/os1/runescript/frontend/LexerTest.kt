package com.os1.runescript.frontend

import com.os1.runescript.frontend.diagnostics.Diagnostics
import com.os1.runescript.frontend.lexer.Lexer
import com.os1.runescript.frontend.lexer.TokenType
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse

class LexerTest {
    private fun lex(src: String): List<TokenType> {
        val diag = Diagnostics()
        val toks = Lexer(src, "test.rs2", diag).tokenize()
        assertFalse(diag.hasErrors(), "lexer errors: ${diag.all}")
        return toks.map { it.type }
    }

    @Test fun triggerHeader() {
        val t = lex("[login,_]")
        assertEquals(
            listOf(TokenType.LBRACK, TokenType.IDENTIFIER, TokenType.COMMA, TokenType.IDENTIFIER, TokenType.RBRACK, TokenType.EOF),
            t,
        )
    }

    @Test fun commandCallWithString() {
        val t = lex("""mes("Welcome to RuneScape.");""")
        assertEquals(
            listOf(
                TokenType.IDENTIFIER, TokenType.LPAREN,
                TokenType.QUOTE_OPEN, TokenType.STRING_TEXT, TokenType.QUOTE_CLOSE,
                TokenType.RPAREN, TokenType.SEMICOLON, TokenType.EOF,
            ),
            t,
        )
    }

    @Test fun varsConstantsAndProc() {
        val t = lex("%musicplay = ^true; ~initalltabs;")
        assertEquals(
            listOf(
                TokenType.MOD, TokenType.IDENTIFIER, TokenType.EQ, TokenType.CARET, TokenType.BOOLEAN_LITERAL, TokenType.SEMICOLON,
                TokenType.TILDE, TokenType.IDENTIFIER, TokenType.SEMICOLON, TokenType.EOF,
            ),
            t,
        )
    }

    @Test fun numbersAndKeywords() {
        val t = lex("if (%x >= 2) { def_int \$y = -5; }")
        assertEquals(
            listOf(
                TokenType.IF, TokenType.LPAREN, TokenType.MOD, TokenType.IDENTIFIER, TokenType.GTE, TokenType.INTEGER_LITERAL, TokenType.RPAREN,
                TokenType.LBRACE, TokenType.DEF_TYPE, TokenType.DOLLAR, TokenType.IDENTIFIER, TokenType.EQ, TokenType.INTEGER_LITERAL, TokenType.SEMICOLON, TokenType.RBRACE,
                TokenType.EOF,
            ),
            t,
        )
    }
}
