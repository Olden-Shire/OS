package com.os.runescript.plugin.annotate

import com.intellij.lang.annotation.AnnotationHolder
import com.intellij.lang.annotation.ExternalAnnotator
import com.intellij.lang.annotation.HighlightSeverity
import com.intellij.openapi.util.TextRange
import com.intellij.psi.PsiFile
import com.os.runescript.frontend.diagnostics.Diagnostics
import com.os.runescript.frontend.diagnostics.Severity
import com.os.runescript.frontend.lexer.Lexer
import com.os.runescript.frontend.parser.Parser

/**
 * Real error highlighting for `.rs2` server scripts: runs the shared frontend
 * lexer + parser (the same one the CLI compiler uses) off the EDT and surfaces
 * its diagnostics as editor annotations. Because it's the actual compiler
 * grammar, valid scripts produce no false positives.
 *
 * Scoped to `.rs2` — the `.cs2` decompiled clientscript format isn't this
 * grammar, so it's left to the lighter command annotator.
 */
class RuneScriptErrorAnnotator : ExternalAnnotator<RuneScriptErrorAnnotator.Input, List<RuneScriptErrorAnnotator.Diag>>() {

    data class Input(val text: String, val name: String)
    data class Diag(val start: Int, val end: Int, val severity: Severity, val message: String)

    override fun collectInformation(file: PsiFile): Input? {
        if (!file.name.endsWith(".rs2", ignoreCase = true)) return null
        // engine.rs2 is a command-signature declaration file, not a script —
        // parsing it as one would flag every `[command,...]` line.
        if (file.name.equals("engine.rs2", ignoreCase = true)) return null
        return Input(file.text, file.name)
    }

    override fun doAnnotate(info: Input?): List<Diag> {
        info ?: return emptyList()
        val diagnostics = Diagnostics()
        try {
            val tokens = Lexer(info.text, info.name, diagnostics).tokenize()
            Parser(tokens, info.name, diagnostics).parseFile()
        } catch (_: Throwable) {
            // Parser may throw on malformed input — keep whatever it reported.
        }
        return diagnostics.all.map { Diag(it.span.start, it.span.end, it.severity, it.message) }
    }

    override fun apply(file: PsiFile, results: List<Diag>?, holder: AnnotationHolder) {
        val len = file.textLength
        for (d in results.orEmpty()) {
            val start = d.start.coerceIn(0, len)
            val end = d.end.coerceIn(start, len)
            val range = when {
                end > start -> TextRange(start, end)
                start > 0 -> TextRange(start - 1, start) // zero-width (e.g. EOF) → mark prior char
                len > 0 -> TextRange(0, 1)
                else -> continue
            }
            val severity = when (d.severity) {
                Severity.ERROR -> HighlightSeverity.ERROR
                Severity.WARNING -> HighlightSeverity.WARNING
                Severity.INFO -> HighlightSeverity.WEAK_WARNING
            }
            holder.newAnnotation(severity, d.message).range(range).create()
        }
    }
}
