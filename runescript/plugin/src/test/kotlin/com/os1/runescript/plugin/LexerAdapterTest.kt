package com.os1.runescript.plugin

import com.intellij.psi.TokenType
import com.os1.runescript.frontend.lexer.TokenType as Fe
import com.os1.runescript.plugin.lang.RuneScriptLexerAdapter
import com.os1.runescript.plugin.lang.RuneScriptTokens
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/** Verifies the editor lexer covers the buffer gap-free and tags tokens. */
class LexerAdapterTest {
    private fun lex(text: String): List<Triple<Int, Int, com.intellij.psi.tree.IElementType?>> {
        val lexer = RuneScriptLexerAdapter()
        lexer.start(text, 0, text.length, 0)
        val out = ArrayList<Triple<Int, Int, com.intellij.psi.tree.IElementType?>>()
        while (lexer.tokenType != null) {
            out.add(Triple(lexer.tokenStart, lexer.tokenEnd, lexer.tokenType))
            lexer.advance()
        }
        return out
    }

    @Test fun coversBufferGapFree() {
        val text = """
            [login,_]
            // welcome
            mes("Welcome to RuneScape.");
            if (%musicplay = 1) {
                return;
            }
        """.trimIndent()

        val toks = lex(text)
        assertTrue(toks.isNotEmpty(), "lexer produced no tokens")

        // No gaps / overlaps: each token starts where the previous ended,
        // and they span the whole buffer.
        var pos = 0
        for ((s, e, _) in toks) {
            assertEquals(pos, s, "gap/overlap at offset $pos")
            assertTrue(e > s, "empty token at $s")
            pos = e
        }
        assertEquals(text.length, pos, "lexer did not cover the whole buffer")
    }

    @Test fun tagsKeyTokens() {
        val toks = lex("mes(\"hi\"); // c\nif (%x = 1) {}")
        val types = toks.map { it.third }
        assertTrue(RuneScriptTokens.of(Fe.IF) in types, "missing IF keyword token")
        assertTrue(RuneScriptTokens.LINE_COMMENT in types, "comment not tagged")
        assertTrue(RuneScriptTokens.of(Fe.QUOTE_OPEN) in types, "string not tagged")
        assertTrue(TokenType.WHITE_SPACE in types, "whitespace not tagged")
    }
}
