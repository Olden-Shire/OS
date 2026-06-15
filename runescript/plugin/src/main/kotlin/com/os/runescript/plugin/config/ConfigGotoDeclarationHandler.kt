package com.os.runescript.plugin.config

import com.intellij.codeInsight.navigation.actions.GotoDeclarationHandler
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.vfs.LocalFileSystem
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiManager
import com.intellij.psi.util.elementType
import com.os.runescript.plugin.symbol.PackSymbolService

/**
 * Ctrl-click navigation in a config file:
 *  - a `com_N` component ref (`layer=com_0` in an `.if`) → its `[com_N …]`
 *    section header in the same file.
 *  - a config named ref (`readyanim=seq_447`, `models=model_1491`, npc/loc/obj
 *    names) → the referenced config/model file it names.
 */
class ConfigGotoDeclarationHandler : GotoDeclarationHandler {
    override fun getGotoDeclarationTargets(source: PsiElement?, offset: Int, editor: Editor?): Array<PsiElement>? {
        source ?: return null
        if (source.elementType == ConfigTokens.VALUE) return valueTarget(source)
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

    /** Resolve a config named-ref value to the file it names. */
    private fun valueTarget(source: PsiElement): Array<PsiElement>? {
        val project = source.project
        val (type, _) = PackSymbolService.get(project).configRef(source.text) ?: return null
        if (type == "interface") return null
        val base = project.basePath ?: return null
        val root = LocalFileSystem.getInstance().findFileByPath("$base/Content") ?: return null
        val (dir, ext) = when (type) {
            "model" -> root.findChild("models") to "ob2"
            "anim" -> root.findChild("anims") to "anim"
            else -> root.findChild("config")?.findChild(type) to type
        }
        val want = "${source.text}.$ext"
        val vf = (dir ?: return null).let { findByName(it, want) } ?: return null
        return PsiManager.getInstance(project).findFile(vf)?.let { arrayOf<PsiElement>(it) }
    }

    private fun findByName(dir: VirtualFile, name: String): VirtualFile? {
        val stack = ArrayDeque(listOf(dir))
        while (stack.isNotEmpty()) {
            for (child in stack.removeLast().children) {
                if (child.isDirectory) stack.addLast(child) else if (child.name == name) return child
            }
        }
        return null
    }

    private fun collectLeaves(element: PsiElement, visit: (PsiElement) -> Unit) {
        val children = element.children
        if (children.isEmpty()) visit(element) else for (c in children) collectLeaves(c, visit)
    }
}
