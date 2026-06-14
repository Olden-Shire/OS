package com.os.runescript.plugin.lang

import com.intellij.psi.tree.IElementType
import com.intellij.psi.tree.IFileElementType
import com.intellij.psi.tree.TokenSet
import com.os.runescript.frontend.lexer.TokenType as FeTokenType

/** An IElementType backed by a frontend lexer token kind. */
class RuneScriptTokenType(debugName: String) : IElementType(debugName, RuneScriptLanguage)

/** A composite (parse-tree node) element type. */
class RuneScriptElementType(debugName: String) : IElementType(debugName, RuneScriptLanguage)

object RuneScriptTokens {
    /** One platform token type per frontend token kind. */
    private val byFrontend: Map<FeTokenType, IElementType> =
        FeTokenType.entries.associateWith { RuneScriptTokenType(it.name) }

    fun of(t: FeTokenType): IElementType = byFrontend.getValue(t)

    // Trivia (reconstructed by the lexer adapter; not produced by the frontend).
    val LINE_COMMENT = RuneScriptTokenType("LINE_COMMENT")
    val BLOCK_COMMENT = RuneScriptTokenType("BLOCK_COMMENT")

    val FILE = IFileElementType(RuneScriptLanguage)

    val COMMENTS = TokenSet.create(LINE_COMMENT, BLOCK_COMMENT)
    val STRINGS = TokenSet.create(
        of(FeTokenType.STRING_TEXT),
        of(FeTokenType.QUOTE_OPEN),
        of(FeTokenType.QUOTE_CLOSE),
    )
    val KEYWORDS = TokenSet.create(
        of(FeTokenType.IF), of(FeTokenType.ELSE), of(FeTokenType.WHILE),
        of(FeTokenType.CASE), of(FeTokenType.DEFAULT), of(FeTokenType.RETURN),
        of(FeTokenType.CALC), of(FeTokenType.DEF_TYPE), of(FeTokenType.SWITCH_TYPE),
        of(FeTokenType.TYPE_ARRAY),
    )
    val NUMBERS = TokenSet.create(
        of(FeTokenType.INTEGER_LITERAL), of(FeTokenType.HEX_LITERAL),
        of(FeTokenType.BIN_LITERAL), of(FeTokenType.COORD_LITERAL),
        of(FeTokenType.MAPZONE_LITERAL), of(FeTokenType.BOOLEAN_LITERAL),
        of(FeTokenType.CHAR_LITERAL), of(FeTokenType.NULL_LITERAL),
    )
}
