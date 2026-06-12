package com.os1.runescript.plugin.asm

import com.intellij.extapi.psi.ASTWrapperPsiElement
import com.intellij.extapi.psi.PsiFileBase
import com.intellij.lang.ASTNode
import com.intellij.lang.ParserDefinition
import com.intellij.lang.PsiParser
import com.intellij.openapi.editor.DefaultLanguageHighlighterColors
import com.intellij.openapi.editor.colors.TextAttributesKey
import com.intellij.openapi.fileTypes.SyntaxHighlighterBase
import com.intellij.openapi.fileTypes.SyntaxHighlighterFactory
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.psi.FileViewProvider
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.tree.IElementType
import com.intellij.psi.tree.IFileElementType
import com.intellij.psi.tree.TokenSet

class Cs2AsmSyntaxHighlighter : SyntaxHighlighterBase() {
    override fun getHighlightingLexer() = Cs2AsmLexer()

    override fun getTokenHighlights(tokenType: IElementType?): Array<TextAttributesKey> =
        when (tokenType) {
            Cs2AsmTokens.COMMENT -> pack(DefaultLanguageHighlighterColors.LINE_COMMENT)
            Cs2AsmTokens.DIRECTIVE -> pack(DefaultLanguageHighlighterColors.METADATA)
            Cs2AsmTokens.LABEL -> pack(DefaultLanguageHighlighterColors.LABEL)
            Cs2AsmTokens.MNEMONIC -> pack(DefaultLanguageHighlighterColors.KEYWORD)
            Cs2AsmTokens.IDENT -> pack(DefaultLanguageHighlighterColors.IDENTIFIER)
            Cs2AsmTokens.NUMBER -> pack(DefaultLanguageHighlighterColors.NUMBER)
            Cs2AsmTokens.STRING -> pack(DefaultLanguageHighlighterColors.STRING)
            else -> TextAttributesKey.EMPTY_ARRAY
        }
}

class Cs2AsmSyntaxHighlighterFactory : SyntaxHighlighterFactory() {
    override fun getSyntaxHighlighter(project: Project?, virtualFile: VirtualFile?) =
        Cs2AsmSyntaxHighlighter()
}

/** Layer-1 flat parser, same approach as the RuneScript one — tokens under a file node. */
class Cs2AsmParserDefinition : ParserDefinition {
    override fun createLexer(project: Project?) = Cs2AsmLexer()
    override fun createParser(project: Project?) = PsiParser { root, builder ->
        val marker = builder.mark()
        while (!builder.eof()) builder.advanceLexer()
        marker.done(root)
        builder.treeBuilt
    }

    override fun getFileNodeType(): IFileElementType = Cs2AsmTokens.FILE
    override fun getCommentTokens(): TokenSet = Cs2AsmTokens.COMMENTS
    override fun getStringLiteralElements(): TokenSet = Cs2AsmTokens.STRINGS
    override fun getWhitespaceTokens(): TokenSet = Cs2AsmTokens.WHITESPACES
    override fun createElement(node: ASTNode): PsiElement = ASTWrapperPsiElement(node)
    override fun createFile(viewProvider: FileViewProvider): PsiFile = Cs2AsmPsiFile(viewProvider)
}

class Cs2AsmPsiFile(viewProvider: FileViewProvider) : PsiFileBase(viewProvider, Cs2AsmLanguage) {
    override fun getFileType() = Cs2AsmFileType.INSTANCE
    override fun toString(): String = "Cs2Asm File"
}
