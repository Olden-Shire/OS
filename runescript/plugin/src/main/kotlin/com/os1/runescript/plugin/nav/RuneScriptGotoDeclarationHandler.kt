package com.os1.runescript.plugin.nav

import com.intellij.codeInsight.navigation.actions.GotoDeclarationHandler
import com.intellij.openapi.editor.Editor
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiManager
import com.intellij.psi.search.FileTypeIndex
import com.intellij.psi.search.FilenameIndex
import com.intellij.psi.search.GlobalSearchScope
import com.intellij.psi.util.elementType
import com.os1.runescript.frontend.lexer.TokenType as Fe
import com.os1.runescript.plugin.lang.RuneScriptFileType
import com.os1.runescript.plugin.lang.RuneScriptTokens
import com.os1.runescript.plugin.symbol.PackSymbolService

/**
 * Ctrl-click / go-to-declaration for script references: `~proc` jumps to its
 * `[proc,name]` header and `@label` to its `[label,name]` header, searching the
 * project's `.rs2` files. Scripts are defined by a header in source (unlike
 * pack-backed configs), so navigation resolves to that header's name token.
 */
class RuneScriptGotoDeclarationHandler : GotoDeclarationHandler {
    private val ident = RuneScriptTokens.of(Fe.IDENTIFIER)
    private val tilde = RuneScriptTokens.of(Fe.TILDE)
    private val at = RuneScriptTokens.of(Fe.AT)
    private val dollar = RuneScriptTokens.of(Fe.DOLLAR)
    private val caret = RuneScriptTokens.of(Fe.CARET)
    private val comma = RuneScriptTokens.of(Fe.COMMA)
    private val lbrack = RuneScriptTokens.of(Fe.LBRACK)

    override fun getGotoDeclarationTargets(source: PsiElement?, offset: Int, editor: Editor?): Array<PsiElement>? {
        source ?: return null
        if (source.elementType != ident) return null
        val name = source.text
        val prev = prevSignificant(source)?.elementType

        // `~proc` / `@label` → their source header.
        when (prev) {
            tilde -> return findScriptHeader(source, "proc", name)?.let { arrayOf(it) }
            at -> return findScriptHeader(source, "label", name)?.let { arrayOf(it) }
            dollar, caret -> return null // local / constant — not a config
        }

        // config / var → its definition file: interface → .if, npc → .npc,
        // varp → .varp, etc. (the `%`/`.%` sigil case lands here too).
        val ref = PackSymbolService.get(source.project).configRef(name) ?: return null
        return resolveConfigFile(source, ref.first, ref.second, name)?.let { arrayOf(it) }
    }

    /** The Content definition file for a config: `interfaces/{stem}.if` (stem is
     *  the hybrid id/name), else `config/{type}/{name}.{type}` (resolved by
     *  filename, so it's found regardless of any shard subdir). */
    private fun resolveConfigFile(context: PsiElement, type: String, id: Int, name: String): PsiElement? {
        val project = context.project
        val filename = when {
            type == "interface" && name.contains(':') ->
                "${ifStem(id ushr 16, name.substringBefore(':'))}.if"
            type == "interface" -> "${ifStem(id, name)}.if"
            else -> "$name.$type"
        }
        val scope = GlobalSearchScope.projectScope(project)
        val vf = FilenameIndex.getVirtualFilesByName(filename, scope).firstOrNull() ?: return null
        return PsiManager.getInstance(project).findFile(vf)
    }

    /** Hybrid `.if` stem: bare `{id}` for the default `if_{id}`, else `{id}_{name}`. */
    private fun ifStem(id: Int, name: String): String =
        if (name == "if_$id") id.toString() else "${id}_$name"

    /** Find the `[kind,name]` header's name token across project `.rs2` files. */
    private fun findScriptHeader(context: PsiElement, kind: String, name: String): PsiElement? {
        val project = context.project
        val psiManager = PsiManager.getInstance(project)
        val files = FileTypeIndex.getFiles(RuneScriptFileType.INSTANCE, GlobalSearchScope.projectScope(project))
        for (vf in files) {
            if (!vf.name.endsWith(".rs2", ignoreCase = true)) continue
            val psi = psiManager.findFile(vf) ?: continue
            var found: PsiElement? = null
            collectLeaves(psi) { leaf ->
                if (found == null && leaf.elementType == ident && leaf.text == name) {
                    val c = prevSignificant(leaf)
                    if (c?.elementType == comma) {
                        val k = prevSignificant(c)
                        if (k?.elementType == ident && k.text == kind &&
                            prevSignificant(k)?.elementType == lbrack
                        ) {
                            found = leaf
                        }
                    }
                }
            }
            found?.let { return it }
        }
        return null
    }

    private fun collectLeaves(element: PsiElement, visit: (PsiElement) -> Unit) {
        val children = element.children
        if (children.isEmpty()) visit(element) else for (c in children) collectLeaves(c, visit)
    }

    private fun prevSignificant(element: PsiElement): PsiElement? {
        var e = element.prevSibling
        while (e != null && (e.elementType == com.intellij.psi.TokenType.WHITE_SPACE ||
                    e.elementType in RuneScriptTokens.COMMENTS)) {
            e = e.prevSibling
        }
        return e
    }
}
