package com.os1.runescript.frontend.diagnostics

import com.os1.runescript.frontend.lexer.SourceSpan

enum class Severity { ERROR, WARNING, INFO }

data class Diagnostic(
    val severity: Severity,
    val message: String,
    val span: SourceSpan,
    val sourceName: String,
)

/** Accumulates diagnostics across a compilation. */
class Diagnostics {
    private val entries = mutableListOf<Diagnostic>()

    val all: List<Diagnostic> get() = entries
    val errorCount: Int get() = entries.count { it.severity == Severity.ERROR }
    fun hasErrors(): Boolean = errorCount > 0

    fun error(sourceName: String, span: SourceSpan, message: String) {
        entries += Diagnostic(Severity.ERROR, message, span, sourceName)
    }

    fun warning(sourceName: String, span: SourceSpan, message: String) {
        entries += Diagnostic(Severity.WARNING, message, span, sourceName)
    }

    fun printAll() {
        for (d in entries) {
            val tag = when (d.severity) {
                Severity.ERROR -> "error"
                Severity.WARNING -> "warning"
                Severity.INFO -> "info"
            }
            System.err.println("${d.sourceName}:${d.span.line}:${d.span.column}: $tag: ${d.message}")
        }
    }
}

class CompileException(message: String) : RuntimeException(message)
