package com.os.runescript.plugin.highlight

import com.intellij.lexer.Lexer
import com.intellij.openapi.editor.DefaultLanguageHighlighterColors as D
import com.intellij.openapi.editor.colors.TextAttributesKey
import com.intellij.openapi.editor.colors.TextAttributesKey.createTextAttributesKey
import com.intellij.openapi.fileTypes.SyntaxHighlighter
import com.intellij.openapi.fileTypes.SyntaxHighlighterBase
import com.intellij.openapi.fileTypes.SyntaxHighlighterFactory
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.psi.tree.IElementType
import com.os.runescript.frontend.lexer.TokenType as Fe
import com.os.runescript.plugin.lang.RuneScriptLexerAdapter
import com.os.runescript.plugin.lang.RuneScriptTokens

object RsColors {
    val KEYWORD = key("RS_KEYWORD", D.KEYWORD)
    val STRING = key("RS_STRING", D.STRING)
    val NUMBER = key("RS_NUMBER", D.NUMBER)
    val COMMENT = key("RS_COMMENT", D.LINE_COMMENT)
    val LOCAL_VAR = key("RS_LOCAL_VAR", D.LOCAL_VARIABLE)
    val GAME_VAR = key("RS_GAME_VAR", D.INSTANCE_FIELD)
    val CONSTANT = key("RS_CONSTANT", D.CONSTANT)
    val COMMAND = key("RS_COMMAND", D.FUNCTION_CALL)
    val CONFIG = key("RS_CONFIG", D.STATIC_FIELD)
    val TRIGGER = key("RS_TRIGGER", D.METADATA)
    val OPERATOR = key("RS_OPERATOR", D.OPERATION_SIGN)
    val BRACES = key("RS_BRACES", D.BRACES)
    val PARENS = key("RS_PARENS", D.PARENTHESES)

    private fun key(name: String, fallback: TextAttributesKey) = createTextAttributesKey(name, fallback)
}

class RuneScriptSyntaxHighlighter : SyntaxHighlighterBase() {
    override fun getHighlightingLexer(): Lexer = RuneScriptLexerAdapter()

    override fun getTokenHighlights(tokenType: IElementType): Array<TextAttributesKey> = when (tokenType) {
        RuneScriptTokens.LINE_COMMENT, RuneScriptTokens.BLOCK_COMMENT -> pack(RsColors.COMMENT)
        in RuneScriptTokens.KEYWORDS -> pack(RsColors.KEYWORD)
        in RuneScriptTokens.NUMBERS -> pack(RsColors.NUMBER)
        in RuneScriptTokens.STRINGS -> pack(RsColors.STRING)
        RuneScriptTokens.of(Fe.DOLLAR) -> pack(RsColors.LOCAL_VAR)
        RuneScriptTokens.of(Fe.MOD), RuneScriptTokens.of(Fe.DOTMOD) -> pack(RsColors.GAME_VAR)
        RuneScriptTokens.of(Fe.CARET) -> pack(RsColors.CONSTANT)
        RuneScriptTokens.of(Fe.TILDE), RuneScriptTokens.of(Fe.AT) -> pack(RsColors.COMMAND)
        RuneScriptTokens.of(Fe.LBRACK), RuneScriptTokens.of(Fe.RBRACK) -> pack(RsColors.TRIGGER)
        RuneScriptTokens.of(Fe.LBRACE), RuneScriptTokens.of(Fe.RBRACE) -> pack(RsColors.BRACES)
        RuneScriptTokens.of(Fe.LPAREN), RuneScriptTokens.of(Fe.RPAREN) -> pack(RsColors.PARENS)
        RuneScriptTokens.of(Fe.PLUS), RuneScriptTokens.of(Fe.MINUS), RuneScriptTokens.of(Fe.MUL),
        RuneScriptTokens.of(Fe.DIV), RuneScriptTokens.of(Fe.EQ), RuneScriptTokens.of(Fe.EXCL),
        RuneScriptTokens.of(Fe.LT), RuneScriptTokens.of(Fe.GT), RuneScriptTokens.of(Fe.LTE),
        RuneScriptTokens.of(Fe.GTE), RuneScriptTokens.of(Fe.AND), RuneScriptTokens.of(Fe.OR),
        -> pack(RsColors.OPERATOR)
        else -> emptyArray()
    }
}

class RuneScriptSyntaxHighlighterFactory : SyntaxHighlighterFactory() {
    override fun getSyntaxHighlighter(project: Project?, virtualFile: VirtualFile?): SyntaxHighlighter =
        RuneScriptSyntaxHighlighter()
}
