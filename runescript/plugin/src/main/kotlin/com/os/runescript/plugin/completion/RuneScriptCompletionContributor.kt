package com.os.runescript.plugin.completion

import com.intellij.codeInsight.completion.CompletionContributor
import com.intellij.codeInsight.completion.CompletionParameters
import com.intellij.codeInsight.completion.CompletionProvider
import com.intellij.codeInsight.completion.CompletionResultSet
import com.intellij.codeInsight.completion.CompletionType
import com.intellij.codeInsight.lookup.LookupElementBuilder
import com.intellij.patterns.PlatformPatterns
import com.intellij.psi.PsiElement
import com.intellij.psi.TokenType
import com.intellij.psi.tree.TokenSet
import com.intellij.psi.util.elementType
import com.intellij.util.ProcessingContext
import com.os.runescript.compiler.codegen.SubjectMode
import com.os.runescript.compiler.codegen.Trigger
import com.os.runescript.frontend.lexer.TokenType as Fe
import com.os.runescript.plugin.lang.RuneScriptLanguage
import com.os.runescript.plugin.lang.RuneScriptTokens
import com.os.runescript.plugin.symbol.PackSymbolService

/**
 * Pack-backed completion, context-aware:
 *  - inside a `[` trigger header: trigger names, then subjects of the trigger's
 *    config type (`[opnpc1,<here>` → npcs);
 *  - in a statement/expression: commands always, plus the (tens of thousands
 *    of) configs/vars ONLY in a value position (call arg or RHS).
 */
class RuneScriptCompletionContributor : CompletionContributor() {
    private val identifier = RuneScriptTokens.of(Fe.IDENTIFIER)
    private val lbrack = RuneScriptTokens.of(Fe.LBRACK)
    private val comma = RuneScriptTokens.of(Fe.COMMA)

    init {
        extend(
            CompletionType.BASIC,
            PlatformPatterns.psiElement().withLanguage(RuneScriptLanguage),
            object : CompletionProvider<CompletionParameters>() {
                override fun addCompletions(
                    parameters: CompletionParameters,
                    context: ProcessingContext,
                    result: CompletionResultSet,
                ) = complete(parameters.position, result)
            },
        )
    }

    private fun complete(position: PsiElement, result: CompletionResultSet) {
        val symbols = PackSymbolService.get(position.project)
        val prev = prevSignificant(position)

        // `[<trigger>` — offer trigger names.
        if (prev?.elementType == lbrack) {
            for (t in Trigger.all()) {
                result.addElement(LookupElementBuilder.create(t.name).withTypeText("trigger"))
            }
            return
        }
        // `[<trigger>,<subject>` — offer config names of the trigger's type.
        if (prev?.elementType == comma) {
            val trig = prevSignificant(prev)
            if (trig?.elementType == identifier && prevSignificant(trig)?.elementType == lbrack) {
                val t = Trigger.byName(trig.text)
                val type = t?.subjectType
                if (t?.subjectMode == SubjectMode.TYPE && type != null) {
                    for (n in symbols.configNamesOfType(type)) {
                        result.addElement(LookupElementBuilder.create(n).withTypeText(type))
                    }
                }
                return
            }
        }

        // Statement / expression body: commands always; configs only in a value
        // position (call arg or RHS) — there are tens of thousands.
        for (name in symbols.commandNames()) {
            val sig = symbols.commandSignature(name)
            val element = if (sig != null) {
                LookupElementBuilder.create(name).withTailText(sig, true).withTypeText("command")
            } else {
                LookupElementBuilder.create(name).withTypeText("command")
            }
            result.addElement(element)
        }
        if (inValuePosition(position)) {
            for ((name, type, id) in symbols.configEntries()) {
                result.addElement(LookupElementBuilder.create(name).withTypeText("$type $id"))
            }
        }
    }

    /** Tokens that can immediately precede a value: a call arg (`(` / `,`), an
     *  assignment/comparison/arithmetic RHS, a `%`/`$`/`^` sigil, `return`,
     *  `case`. */
    private val valuePreceders: TokenSet = TokenSet.create(
        RuneScriptTokens.of(Fe.LPAREN), RuneScriptTokens.of(Fe.COMMA),
        RuneScriptTokens.of(Fe.EQ), RuneScriptTokens.of(Fe.LT), RuneScriptTokens.of(Fe.GT),
        RuneScriptTokens.of(Fe.LTE), RuneScriptTokens.of(Fe.GTE),
        RuneScriptTokens.of(Fe.PLUS), RuneScriptTokens.of(Fe.MUL), RuneScriptTokens.of(Fe.DIV),
        RuneScriptTokens.of(Fe.AND), RuneScriptTokens.of(Fe.OR), RuneScriptTokens.of(Fe.EXCL),
        RuneScriptTokens.of(Fe.MOD), RuneScriptTokens.of(Fe.DOTMOD),
        RuneScriptTokens.of(Fe.DOLLAR), RuneScriptTokens.of(Fe.CARET),
        RuneScriptTokens.of(Fe.RETURN), RuneScriptTokens.of(Fe.CASE),
    )

    private fun inValuePosition(position: PsiElement): Boolean {
        val e = prevSignificant(position)
        return e != null && e.elementType in valuePreceders
    }

    private fun prevSignificant(element: PsiElement): PsiElement? {
        var e = element.prevSibling
        while (e != null && (e.elementType == TokenType.WHITE_SPACE || e.elementType in RuneScriptTokens.COMMENTS)) {
            e = e.prevSibling
        }
        return e
    }
}
