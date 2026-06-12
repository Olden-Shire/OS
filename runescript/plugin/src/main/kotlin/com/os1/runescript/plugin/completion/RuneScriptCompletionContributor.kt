package com.os1.runescript.plugin.completion

import com.intellij.codeInsight.completion.CompletionContributor
import com.intellij.codeInsight.completion.CompletionParameters
import com.intellij.codeInsight.completion.CompletionProvider
import com.intellij.codeInsight.completion.CompletionResultSet
import com.intellij.codeInsight.completion.CompletionType
import com.intellij.codeInsight.lookup.LookupElementBuilder
import com.intellij.patterns.PlatformPatterns
import com.intellij.util.ProcessingContext
import com.os1.runescript.plugin.lang.RuneScriptLanguage
import com.os1.runescript.plugin.symbol.PackSymbolService

/** Offers pack-backed command and config names. */
class RuneScriptCompletionContributor : CompletionContributor() {
    init {
        extend(
            CompletionType.BASIC,
            PlatformPatterns.psiElement().withLanguage(RuneScriptLanguage),
            object : CompletionProvider<CompletionParameters>() {
                override fun addCompletions(
                    parameters: CompletionParameters,
                    context: ProcessingContext,
                    result: CompletionResultSet,
                ) {
                    val symbols = PackSymbolService.get(parameters.position.project)
                    for (name in symbols.commandNames()) {
                        result.addElement(
                            LookupElementBuilder.create(name).withIcon(null).withTypeText("command")
                        )
                    }
                    for (name in symbols.configNames()) {
                        result.addElement(LookupElementBuilder.create(name).withTypeText("config"))
                    }
                }
            },
        )
    }
}
