package com.os.runescript.plugin.config

import com.intellij.lang.annotation.AnnotationHolder
import com.intellij.lang.annotation.Annotator
import com.intellij.lang.annotation.HighlightSeverity
import com.intellij.psi.PsiElement
import com.intellij.psi.util.elementType
import com.os.runescript.plugin.symbol.PackSymbolService

/**
 * Paints config VALUE tokens that resolve to a known `*.pack` symbol in
 * "var purple" (the same colour as a RuneScript `%var`), to signal at a glance
 * that the value is a pack reference rather than a plain string.
 *
 * This has to be a project-aware annotator, not a lexer rule: the defaults look
 * like `seq_447` / `model_1491`, but renamed refs (`goblin_ready`, `bronze_axe`)
 * are arbitrary identifiers indistinguishable from ordinary string values by
 * shape alone. Only a [PackSymbolService] lookup can tell them apart.
 *
 * Interface component refs (`com_N`) get their own gold colour from the lexer,
 * so they're left untouched here.
 */
class ConfigPackRefAnnotator : Annotator {
    override fun annotate(element: PsiElement, holder: AnnotationHolder) {
        if (element.elementType != ConfigTokens.VALUE) return
        val symbols = PackSymbolService.get(element.project)
        if (!symbols.hasMetadata()) return
        val (type, _) = symbols.configRef(element.text) ?: return
        if (type == "interface") return
        holder.newSilentAnnotation(HighlightSeverity.INFORMATION)
            .range(element).textAttributes(ConfigColors.PACKREF).create()
    }
}
