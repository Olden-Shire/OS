package com.os1.runescript.plugin.config

import com.intellij.codeInsight.navigation.actions.GotoDeclarationHandler
import com.intellij.openapi.editor.Editor
import com.intellij.psi.PsiElement
import com.intellij.psi.util.elementType

/**
 * Ctrl-click on a `com_N` component reference in a config file (e.g.
 * `layer=com_0` in an `.if`) jumps to its `[com_N …]` section header in the
 * same file.
 */
class ConfigGotoDeclarationHandler : GotoDeclarationHandler {
    override fun getGotoDeclarationTargets(source: PsiElement?, offset: Int, editor: Editor?): Array<PsiElement>? {
        source ?: return null
        if (source.elementType != ConfigTokens.COMREF) return null
        val want = source.text // "com_0"
        val file = source.containingFile ?: return null
        var target: PsiElement? = null
        collectLeaves(file) { leaf ->
            if (target == null && leaf.elementType == ConfigTokens.SECTION) {
                val inside = leaf.text.trim().removeSurrounding("[", "]")
                if (inside.split(Regex("\\s+")).firstOrNull() == want) target = leaf
            }
        }
        return target?.let { arrayOf(it) }
    }

    private fun collectLeaves(element: PsiElement, visit: (PsiElement) -> Unit) {
        val children = element.children
        if (children.isEmpty()) visit(element) else for (c in children) collectLeaves(c, visit)
    }
}
