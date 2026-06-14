package com.os.runescript.plugin.psi

import com.intellij.extapi.psi.ASTWrapperPsiElement
import com.intellij.extapi.psi.PsiFileBase
import com.intellij.lang.ASTNode
import com.intellij.lang.ParserDefinition
import com.intellij.lang.PsiParser
import com.intellij.lexer.Lexer
import com.intellij.openapi.project.Project
import com.intellij.psi.FileViewProvider
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.tree.IFileElementType
import com.intellij.psi.tree.TokenSet
import com.os.runescript.plugin.lang.RuneScriptFileType
import com.os.runescript.plugin.lang.RuneScriptLanguage
import com.os.runescript.plugin.lang.RuneScriptLexerAdapter
import com.os.runescript.plugin.lang.RuneScriptTokens

class RuneScriptParserDefinition : ParserDefinition {
    override fun createLexer(project: Project?): Lexer = RuneScriptLexerAdapter()
    override fun createParser(project: Project?): PsiParser = RuneScriptPsiParser()
    override fun getFileNodeType(): IFileElementType = RuneScriptTokens.FILE
    override fun getCommentTokens(): TokenSet = RuneScriptTokens.COMMENTS
    override fun getStringLiteralElements(): TokenSet = RuneScriptTokens.STRINGS
    override fun createElement(node: ASTNode): PsiElement = ASTWrapperPsiElement(node)
    override fun createFile(viewProvider: FileViewProvider): PsiFile = RuneScriptPsiFile(viewProvider)
}

/**
 * Layer-1 parser: a flat tree under the file node. Highlighting + the annotator
 * operate on the token stream; structured PSI (references, find-usages) builds
 * on top of this in a follow-up.
 */
class RuneScriptPsiParser : PsiParser {
    override fun parse(root: com.intellij.psi.tree.IElementType, builder: com.intellij.lang.PsiBuilder): ASTNode {
        val rootMarker = builder.mark()
        while (!builder.eof()) {
            builder.advanceLexer()
        }
        rootMarker.done(root)
        return builder.treeBuilt
    }
}

class RuneScriptPsiFile(viewProvider: FileViewProvider) :
    PsiFileBase(viewProvider, RuneScriptLanguage) {
    override fun getFileType() = RuneScriptFileType.INSTANCE
    override fun toString(): String = "RuneScript File"
}
