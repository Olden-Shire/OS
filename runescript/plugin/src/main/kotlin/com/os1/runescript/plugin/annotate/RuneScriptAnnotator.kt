package com.os1.runescript.plugin.annotate

import com.intellij.lang.annotation.AnnotationHolder
import com.intellij.lang.annotation.Annotator
import com.intellij.lang.annotation.HighlightSeverity
import com.intellij.psi.PsiElement
import com.intellij.psi.util.elementType
import com.os1.runescript.frontend.lexer.TokenType as Fe
import com.os1.runescript.plugin.highlight.RsColors
import com.os1.runescript.plugin.lang.RuneScriptTokens
import com.os1.runescript.plugin.symbol.PackSymbolService

/**
 * Semantic annotator: paints command-call heads and flags identifiers used as
 * commands that aren't in our pack metadata. Operates on the flat token stream
 * (no structured PSI yet) by looking at each IDENTIFIER and its next
 * significant sibling.
 */
class RuneScriptAnnotator : Annotator {
    private val identifier = RuneScriptTokens.of(Fe.IDENTIFIER)
    private val lparen = RuneScriptTokens.of(Fe.LPAREN)

    /** Sigils whose following `name(` is not a command head: `~proc(`, `@jump(`,
     *  `$array0(idx)`, `%var(...)` (`%` lexes as MOD, `.%` as DOTMOD). */
    private val callSigils = setOf(
        RuneScriptTokens.of(Fe.TILDE),
        RuneScriptTokens.of(Fe.AT),
        RuneScriptTokens.of(Fe.DOLLAR),
        RuneScriptTokens.of(Fe.MOD),
        RuneScriptTokens.of(Fe.DOTMOD),
    )

    /** Builtins of the structured cs2 source format that aren't pack commands. */
    private val builtins = setOf("join")

    override fun annotate(element: PsiElement, holder: AnnotationHolder) {
        if (element.elementType != identifier) return
        val next = nextSignificant(element) ?: return
        if (next.elementType != lparen) return
        if (element.prevSibling?.elementType in callSigils) return

        val name = element.text
        if (name in builtins) return
        holder.newSilentAnnotation(HighlightSeverity.INFORMATION)
            .range(element)
            .textAttributes(RsColors.COMMAND)
            .create()

        val symbols = PackSymbolService.get(element.project)
        if (symbols.hasMetadata() && !symbols.isKnownCommand(name)) {
            holder.newAnnotation(HighlightSeverity.WARNING, "Unknown command '$name'")
                .range(element)
                .create()
        }
    }

    private fun nextSignificant(element: PsiElement): PsiElement? {
        var e = element.nextSibling
        while (e != null && (e.elementType == com.intellij.psi.TokenType.WHITE_SPACE ||
                    e.elementType in RuneScriptTokens.COMMENTS)) {
            e = e.nextSibling
        }
        return e
    }
}
