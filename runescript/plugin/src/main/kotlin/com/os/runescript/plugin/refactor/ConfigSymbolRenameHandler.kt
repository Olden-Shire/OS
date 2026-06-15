package com.os.runescript.plugin.refactor

import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.actionSystem.DataContext
import com.intellij.openapi.command.WriteCommandAction
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.fileEditor.FileDocumentManager
import com.intellij.openapi.fileEditor.impl.LoadTextUtil
import com.intellij.openapi.project.Project
import com.intellij.openapi.ui.InputValidator
import com.intellij.openapi.ui.Messages
import com.intellij.openapi.vfs.LocalFileSystem
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.psi.PsiDocumentManager
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.PsiManager
import com.intellij.psi.search.FileTypeIndex
import com.intellij.psi.search.GlobalSearchScope
import com.intellij.psi.search.PsiSearchHelper
import com.intellij.psi.search.UsageSearchContext
import com.intellij.psi.util.elementType
import com.intellij.refactoring.rename.RenameHandler
import com.os.runescript.plugin.config.ConfigFileType
import com.os.runescript.plugin.config.ConfigTokens
import com.os.runescript.plugin.symbol.PackSymbolService

/**
 * Rename refactoring for a CONFIG named reference — a `*.pack` symbol referenced
 * by name in a config file, e.g. `readyanim = seq_447`, `models = model_1491`,
 * `npc_goblin` … (Shift+F6 on the value).
 *
 * The name lives in three places; this renames all of them as one undoable
 * command:
 *  - `Content/pack/<type>.pack` — the id→name line
 *  - the sharded config/model file — its stem IS the pack name
 *    (`Content/config/seq/.../seq_447.seq`, `Content/models/.../model_1491.ob2`)
 *  - every reference to that name in any config file (`.npc`/`.loc`/`.obj`/…)
 *
 * Names are tooling-only (config refs resolve back to ids on repack), so this is
 * CRC-neutral — mirrors `crates/cache/src/content/rename.rs` / the CLI examples.
 * Interface component refs (`if_549:com_2`) are handled by
 * [RsInterfaceRenameHandler] and skipped here.
 */
class ConfigSymbolRenameHandler : RenameHandler {

    override fun isAvailableOnDataContext(dataContext: DataContext): Boolean =
        targetFromContext(dataContext) != null

    override fun isRenaming(dataContext: DataContext): Boolean =
        isAvailableOnDataContext(dataContext)

    override fun invoke(project: Project, editor: Editor?, file: PsiFile?, dataContext: DataContext?) {
        val ctx = dataContext ?: return
        val target = targetFromContext(ctx) ?: return
        runRename(project, target)
    }

    override fun invoke(project: Project, elements: Array<out PsiElement>, dataContext: DataContext?) {}

    /** A pack symbol referenced in a config file: its config type, id and name. */
    private data class Target(val type: String, val id: Int, val oldName: String)

    private fun targetFromContext(dataContext: DataContext): Target? {
        val project = CommonDataKeys.PROJECT.getData(dataContext) ?: return null
        val editor = CommonDataKeys.EDITOR.getData(dataContext) ?: return null
        val file = CommonDataKeys.PSI_FILE.getData(dataContext) ?: return null
        if (file.fileType !is ConfigFileType) return null

        val selection = editor.selectionModel
        val probe = if (selection.hasSelection()) selection.selectionStart else editor.caretModel.offset
        // A value token (`seq_447`); back up one if the caret sits on its end.
        fun valueAt(offset: Int): PsiElement? =
            file.findElementAt(offset)?.takeIf { it.elementType == ConfigTokens.VALUE }
                ?: file.findElementAt((offset - 1).coerceAtLeast(0))?.takeIf { it.elementType == ConfigTokens.VALUE }
        val token = valueAt(probe) ?: return null
        val name = token.text
        // Only flat pack symbols (numeric-stub names never resolve). Interfaces
        // are renamed via RsInterfaceRenameHandler (their refs aren't flat names).
        val (type, id) = PackSymbolService.get(project).configRef(name) ?: return null
        if (type == "interface") return null
        return Target(type, id, name)
    }

    private fun runRename(project: Project, target: Target) {
        val newName = Messages.showInputDialog(
            project, "New ${target.type} name:", "Rename ${target.type}", null, target.oldName, IdentValidator,
        )?.trim() ?: return
        if (newName.isEmpty() || newName == target.oldName) return

        val touched = LinkedHashSet<VirtualFile>()
        WriteCommandAction.runWriteCommandAction(project, "Rename ${target.type}", null, {
            // 1. <type>.pack — the id=name line.
            editPack(project, "${target.type}.pack", touched) { key, value ->
                if (key == target.id && value == target.oldName) "$key=$newName" else null
            }
            // 2. the sharded config/model file — stem is the pack name.
            findSymbolFile(project, target.type, target.id, target.oldName)?.let { vf ->
                val ext = vf.extension?.let { ".$it" } ?: ""
                runCatching { vf.rename(this, "$newName$ext") }
            }
            // 3. references in every config file.
            renameConfigRefs(project, target.oldName, newName, touched)
        })
        val fdm = FileDocumentManager.getInstance()
        for (vf in touched) if (vf.isValid) fdm.getCachedDocument(vf)?.let(fdm::saveDocument)
        PackSymbolService.get(project).invalidate()
    }

    /** Replace every `VALUE` token equal to `oldName` with `newName` across config files. */
    private fun renameConfigRefs(project: Project, oldName: String, newName: String, touched: MutableSet<VirtualFile>) {
        val psiManager = PsiManager.getInstance(project)
        for (vf in candidateFiles(project, oldName)) {
            if (vf.fileType !is ConfigFileType) continue
            val psi = psiManager.findFile(vf) ?: continue
            val edits = ArrayList<Pair<IntRange, String>>()
            collectLeaves(psi) { leaf ->
                if (leaf.elementType == ConfigTokens.VALUE && leaf.text == oldName) {
                    edits += leaf.textRange.startOffset..leaf.textRange.endOffset to newName
                }
            }
            if (edits.isEmpty()) continue
            val doc = FileDocumentManager.getInstance().getDocument(vf) ?: continue
            for ((range, text) in edits.sortedByDescending { it.first.first }) {
                doc.replaceString(range.first, range.last, text)
            }
            PsiDocumentManager.getInstance(project).commitDocument(doc)
            touched += vf
        }
    }

    /**
     * Config files that may reference `name`. Uses the word index so we only
     * parse the handful of files that actually contain the token — parsing
     * every config file (tens of thousands) was what made rename crawl. Falls
     * back to a plain text scan if the index didn't tokenise the name.
     */
    private fun candidateFiles(project: Project, name: String): Collection<VirtualFile> {
        val scope = GlobalSearchScope.projectScope(project)
        val hits = LinkedHashSet<VirtualFile>()
        PsiSearchHelper.getInstance(project)
            .processCandidateFilesForText(scope, UsageSearchContext.ANY, true, name) { vf ->
                hits.add(vf)
                true
            }
        if (hits.isNotEmpty()) return hits
        return FileTypeIndex.getFiles(ConfigFileType.INSTANCE, scope)
            .filter { LoadTextUtil.loadText(it).contains(name) }
    }

    /** Rewrite `<type>.pack` line by line via `map(key, value) -> newLine?`. */
    private fun editPack(project: Project, packName: String, touched: MutableSet<VirtualFile>, map: (Int, String) -> String?) {
        val vf = packDir(project)?.findChild(packName) ?: return
        val doc = FileDocumentManager.getInstance().getDocument(vf) ?: return
        val updated = buildString {
            for (line in doc.text.lines()) {
                val eq = line.indexOf('=')
                val rewritten = if (eq > 0) {
                    val key = line.substring(0, eq).trim().toIntOrNull()
                    val value = line.substring(eq + 1).trim()
                    if (key != null) map(key, value) else null
                } else null
                append(rewritten ?: line)
                append('\n')
            }
        }.let { if (doc.text.endsWith('\n')) it else it.dropLast(1) }
        if (updated != doc.text) {
            doc.setText(updated)
            PsiDocumentManager.getInstance(project).commitDocument(doc)
            touched += vf
        }
    }

    /** Locate a symbol's on-disk file by stem: models in Content/models (.ob2),
     *  config types in Content/config/<type> (.<type>). Sharded types bucket
     *  files into a `%05d` id-dir (mirrors the Rust packer), so we go straight
     *  there instead of walking the whole tree; falls back to a recursive scan
     *  for flat/unexpected layouts. */
    private fun findSymbolFile(project: Project, type: String, id: Int, name: String): VirtualFile? {
        val root = contentRoot(project) ?: return null
        val (dir, ext) = when (type) {
            "model" -> root.findChild("models") to "ob2"
            "anim" -> root.findChild("anims") to "anim"
            else -> root.findChild("config")?.findChild(type) to type
        }
        val base = dir ?: return null
        val wantName = "$name.$ext"
        // Fast path: the id's shard dir, or directly under base (un-sharded).
        val shard = "%05d".format(id / 1000 * 1000)
        (base.findChild(shard)?.findChild(wantName) ?: base.findChild(wantName))?.let { return it }
        // Fallback: walk (covers any layout drift).
        val stack = ArrayDeque(listOf(base))
        while (stack.isNotEmpty()) {
            val d = stack.removeLast()
            for (child in d.children) {
                if (child.isDirectory) stack.addLast(child)
                else if (child.name == wantName) return child
            }
        }
        return null
    }

    private fun collectLeaves(element: PsiElement, visit: (PsiElement) -> Unit) {
        val children = element.children
        if (children.isEmpty()) visit(element) else for (c in children) collectLeaves(c, visit)
    }

    private fun packDir(project: Project): VirtualFile? = contentRoot(project)?.findChild("pack")

    private fun contentRoot(project: Project): VirtualFile? {
        val base = project.basePath ?: return null
        return LocalFileSystem.getInstance().findFileByPath("$base/Content")
    }

    private object IdentValidator : InputValidator {
        override fun checkInput(input: String?): Boolean {
            val s = input?.trim().orEmpty()
            return s.isNotEmpty() && (s[0].isLetter() || s[0] == '_') && s.all { it.isLetterOrDigit() || it == '_' }
        }
        override fun canClose(input: String?): Boolean = checkInput(input)
    }
}
