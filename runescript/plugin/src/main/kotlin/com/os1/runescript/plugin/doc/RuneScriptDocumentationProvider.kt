package com.os1.runescript.plugin.doc

import com.intellij.lang.documentation.AbstractDocumentationProvider
import com.intellij.openapi.editor.Editor
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.util.elementType
import com.os1.runescript.frontend.lexer.TokenType as Fe
import com.os1.runescript.plugin.lang.RuneScriptTokens
import com.os1.runescript.plugin.symbol.PackSymbolService

/**
 * Hover documentation for pack-backed symbols: mousing over (or Ctrl+Q on) an
 * identifier that resolves to a command/var/config/constant shows its kind and
 * id — e.g. `interface welcome — id 549`, `varp coins — id 95`.
 *
 * Symbols are pack lines, not PSI declarations, so there's no reference to
 * resolve; [getCustomDocumentationElement] hands the raw identifier token at
 * the cursor straight to the renderer.
 */
class RuneScriptDocumentationProvider : AbstractDocumentationProvider() {
    private val ident = RuneScriptTokens.of(Fe.IDENTIFIER)

    override fun getCustomDocumentationElement(
        editor: Editor,
        file: PsiFile,
        contextElement: PsiElement?,
        targetOffset: Int,
    ): PsiElement? = contextElement?.takeIf {
        it.elementType == ident && PackSymbolService.get(it.project).describe(it.text) != null
    }

    override fun getQuickNavigateInfo(element: PsiElement?, originalElement: PsiElement?): String? =
        render(element, originalElement)

    override fun generateDoc(element: PsiElement?, originalElement: PsiElement?): String? =
        render(element, originalElement)

    /** Rich HTML (commands lay params out vertically so long signatures fit). */
    private fun render(element: PsiElement?, original: PsiElement?): String? {
        val el = listOfNotNull(original, element).firstOrNull { it.elementType == ident } ?: return null
        return PackSymbolService.get(el.project).docHtml(el.text)
    }
}
