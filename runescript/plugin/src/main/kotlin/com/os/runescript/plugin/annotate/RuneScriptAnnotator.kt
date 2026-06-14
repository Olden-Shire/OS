package com.os.runescript.plugin.annotate

import com.intellij.lang.annotation.AnnotationHolder
import com.intellij.lang.annotation.Annotator
import com.intellij.lang.annotation.HighlightSeverity
import com.intellij.psi.PsiElement
import com.intellij.psi.TokenType
import com.intellij.psi.util.elementType
import com.os.runescript.compiler.codegen.SubjectMode
import com.os.runescript.compiler.codegen.Trigger
import com.os.runescript.frontend.lexer.TokenType as Fe
import com.os.runescript.plugin.highlight.RsColors
import com.os.runescript.plugin.lang.RuneScriptTokens
import com.os.runescript.plugin.symbol.PackSymbolService

/**
 * Semantic annotator over the flat token stream:
 *  - paints command-call heads, warning on commands not in our pack metadata;
 *  - flags identifiers used as CALL ARGUMENTS that resolve to no known pack
 *    symbol (a misspelled interface/config/var) as an error.
 *
 * The argument check is deliberately narrow — only inside a call's parens, and
 * not for `$local` / `^const` / `~proc` / `@label` / the `_` wildcard — so it
 * doesn't false-positive on locals, script names, or trigger subjects.
 */
class RuneScriptAnnotator : Annotator {
    private val identifier = RuneScriptTokens.of(Fe.IDENTIFIER)
    private val lparen = RuneScriptTokens.of(Fe.LPAREN)
    private val rparen = RuneScriptTokens.of(Fe.RPAREN)
    private val lbrack = RuneScriptTokens.of(Fe.LBRACK)
    private val comma = RuneScriptTokens.of(Fe.COMMA)
    private val dollar = RuneScriptTokens.of(Fe.DOLLAR)
    private val caret = RuneScriptTokens.of(Fe.CARET)
    private val tilde = RuneScriptTokens.of(Fe.TILDE)
    private val at = RuneScriptTokens.of(Fe.AT)
    private val mod = RuneScriptTokens.of(Fe.MOD)
    private val dotmod = RuneScriptTokens.of(Fe.DOTMOD)

    /** Statement/header boundaries that end a backward argument scan. */
    private val boundaries = setOf(
        RuneScriptTokens.of(Fe.SEMICOLON), RuneScriptTokens.of(Fe.LBRACE),
        RuneScriptTokens.of(Fe.RBRACE), RuneScriptTokens.of(Fe.LBRACK),
        RuneScriptTokens.of(Fe.RBRACK),
    )

    /** Structured-cs2 builtins that aren't pack commands. */
    private val builtins = setOf("join")

    override fun annotate(element: PsiElement, holder: AnnotationHolder) {
        if (element.elementType != identifier) return
        // engine.rs2 is a command-signature DECLARATION file ([command,name]
        // (params)(returns)), not a script — the compiler excludes it from
        // codegen, so skip script validation/coloring here too.
        if (isEngineDeclarations(element)) return
        val name = element.text
        val next = nextSignificant(element)
        val prev = prevSignificant(element)

        // ── trigger header: [trigger,subject] ────────────────────────────────
        // Server (.rs2) triggers are validated against the engine registry;
        // client (.cs2) scripts use `[clientscript,name]`, which isn't in that
        // (server) registry — colour those, don't flag them.
        val clientScript = isClientScript(element)
        if (prev?.elementType == lbrack) {
            if (!clientScript && Trigger.byName(name) == null) {
                holder.newAnnotation(HighlightSeverity.ERROR, "Unknown trigger '$name'")
                    .range(element).create()
            } else {
                color(holder, element, RsColors.TRIGGER)
            }
            return
        }
        if (prev?.elementType == comma) {
            val trig = prevSignificant(prev)
            if (trig != null && trig.elementType == identifier && prevSignificant(trig)?.elementType == lbrack) {
                if (clientScript) {
                    color(holder, element, RsColors.COMMAND) // clientscript name
                } else {
                    validateSubject(trig.text, element, holder)
                }
                return
            }
        }

        // ── sigil-prefixed name: colour by sigil; validate %vars ─────────────
        when (prev?.elementType) {
            dollar -> { color(holder, element, RsColors.LOCAL_VAR); return }
            caret -> { color(holder, element, RsColors.CONSTANT); return }
            tilde, at -> { color(holder, element, RsColors.COMMAND); return } // proc / label ref
            mod, dotmod -> {
                color(holder, element, RsColors.GAME_VAR)
                val symbols = PackSymbolService.get(element.project)
                if (!clientScript && symbols.hasMetadata() && !symbols.isKnownSymbol(name)) {
                    holder.newAnnotation(HighlightSeverity.ERROR, "Unknown var '$name'")
                        .range(element).create()
                }
                return
            }
        }

        // ── command head: `name(` ────────────────────────────────────────────
        if (next?.elementType == lparen) {
            if (name in builtins) return
            color(holder, element, RsColors.COMMAND)
            val symbols = PackSymbolService.get(element.project)
            if (!symbols.hasMetadata()) return
            if (!symbols.isKnownCommand(name)) {
                holder.newAnnotation(HighlightSeverity.WARNING, "Unknown command '$name'")
                    .range(element).create()
            } else if (!clientScript) {
                // Arity check (server typed commands) — mirrors the compiler's
                // `arguments.size != paramTypes.size`.
                val want = symbols.commandParamCount(name)
                val got = if (want != null) countArgs(element) else null
                if (want != null && got != null && got != want) {
                    holder.newAnnotation(
                        HighlightSeverity.ERROR,
                        "command '$name' expects $want argument(s), got $got",
                    ).range(element).create()
                }
            }
            return
        }

        // ── argument reference: colour known configs, flag unknown names ──────
        if (name == "_" || name in builtins) return
        if (!insideCallArgs(element)) return
        val symbols = PackSymbolService.get(element.project)
        if (!symbols.hasMetadata()) return
        when {
            symbols.isKnownSymbol(name) -> color(holder, element, RsColors.CONFIG)
            // Only red-flag unknowns in server scripts; .cs2 may reference
            // client vars/scripts not present in our packs.
            !clientScript -> holder.newAnnotation(HighlightSeverity.ERROR, "Unknown name '$name'")
                .range(element).create()
        }
    }

    private fun color(holder: AnnotationHolder, element: PsiElement, key: com.intellij.openapi.editor.colors.TextAttributesKey) {
        holder.newSilentAnnotation(HighlightSeverity.INFORMATION).range(element).textAttributes(key).create()
    }

    /** A `.cs2` clientscript file — uses client triggers (`[clientscript,…]`)
     *  not in the server trigger registry, so skip server trigger validation. */
    private fun isClientScript(element: PsiElement): Boolean =
        element.containingFile?.name?.endsWith(".cs2", ignoreCase = true) == true

    /** `engine.rs2` — the command-signature declaration file, not a script. */
    private fun isEngineDeclarations(element: PsiElement): Boolean =
        element.containingFile?.name.equals("engine.rs2", ignoreCase = true)

    /** Validate a trigger's subject against its declared subject mode. */
    private fun validateSubject(triggerName: String, subjectEl: PsiElement, holder: AnnotationHolder) {
        val trig = Trigger.byName(triggerName) ?: return // unknown trigger already flagged
        val subj = subjectEl.text
        when (trig.subjectMode) {
            SubjectMode.NONE ->
                if (subj != "_") holder.newAnnotation(
                    HighlightSeverity.ERROR, "Trigger '$triggerName' takes no subject — use '_'"
                ).range(subjectEl).create()
            SubjectMode.NAME -> color(holder, subjectEl, RsColors.COMMAND) // the script's own name
            SubjectMode.TYPE -> {
                if (subj == "_") return // wildcard catch-all is allowed
                val type = trig.subjectType ?: return
                val symbols = PackSymbolService.get(subjectEl.project)
                when {
                    !symbols.hasMetadata() -> {}
                    symbols.isKnownConfig(type, subj) -> color(holder, subjectEl, RsColors.CONFIG)
                    else -> holder.newAnnotation(HighlightSeverity.ERROR, "Unknown $type '$subj'")
                        .range(subjectEl).create()
                }
            }
        }
    }

    /** Count a call's top-level arguments: `cmd()` → 0, `cmd(a,b)` → 2,
     *  `cmd(foo(x,y))` → 1. Returns null if the call is malformed/unterminated
     *  (so the arity check is skipped rather than firing on broken input). */
    private fun countArgs(head: PsiElement): Int? {
        var e = nextSignificant(head)
        if (e?.elementType != lparen) return null
        var depth = 1
        var commas = 0
        var sawContent = false
        var steps = 0
        e = nextSignificant(e)
        while (e != null && steps < 2000) {
            val t = e.elementType
            when {
                t == lparen -> { depth++; sawContent = true }
                t == rparen -> { depth--; if (depth == 0) return if (sawContent) commas + 1 else 0 }
                t == comma && depth == 1 -> { commas++; sawContent = true }
                t in boundaries -> return null // unterminated call — bail
                else -> sawContent = true
            }
            e = nextSignificant(e)
            steps++
        }
        return null
    }

    /** Is the element inside an open `(` ... argument list (and not inside a
     *  `[` header)? Walks back over significant tokens tracking paren depth. */
    private fun insideCallArgs(element: PsiElement): Boolean {
        var e = prevSignificant(element)
        var depth = 0
        var steps = 0
        while (e != null && steps < 400) {
            val t = e.elementType
            when {
                t == rparen -> depth++
                t == lparen -> { if (depth == 0) return true; depth-- }
                t in boundaries -> return false
            }
            e = prevSignificant(e)
            steps++
        }
        return false
    }

    private fun nextSignificant(element: PsiElement): PsiElement? = step(element, forward = true)
    private fun prevSignificant(element: PsiElement): PsiElement? = step(element, forward = false)

    private fun step(element: PsiElement, forward: Boolean): PsiElement? {
        var e = if (forward) element.nextSibling else element.prevSibling
        while (e != null && (e.elementType == TokenType.WHITE_SPACE || e.elementType in RuneScriptTokens.COMMENTS)) {
            e = if (forward) e.nextSibling else e.prevSibling
        }
        return e
    }
}
