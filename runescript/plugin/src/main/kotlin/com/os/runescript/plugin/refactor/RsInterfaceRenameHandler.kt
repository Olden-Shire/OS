package com.os.runescript.plugin.refactor

import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.actionSystem.DataContext
import com.intellij.openapi.command.WriteCommandAction
import com.intellij.openapi.editor.Document
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.fileEditor.FileDocumentManager
import com.intellij.openapi.project.Project
import com.intellij.openapi.ui.Messages
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.psi.PsiDocumentManager
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.search.FileTypeIndex
import com.intellij.psi.search.GlobalSearchScope
import com.intellij.psi.util.elementType
import com.intellij.refactoring.rename.RenameHandler
import com.os.runescript.frontend.lexer.TokenType as Fe
import com.os.runescript.plugin.lang.RuneScriptFileType
import com.os.runescript.plugin.lang.RuneScriptTokens
import com.os.runescript.plugin.symbol.PackSymbolService

/**
 * Rename refactoring for interface symbols — `if_549` (root) and
 * `if_549:com_2` (component) — invoked with Shift+F6 on the identifier.
 *
 * An interface symbol's name lives in four places; this renames all of them as
 * one undoable command:
 *  - `Content/pack/interface.pack` — the symbol table (root + component lines)
 *  - the `.if` filename — hybrid `{id}.if` / `{id}_{name}.if`
 *  - the `.if` component section header — `[com_N]` / `[com_N name]`
 *  - every `.rs2`/`.cs2` reference token
 *
 * Names are tooling-only (they never reach the packed cache), so a rename is
 * CRC-neutral — mirrors `crates/cache/src/content/rename.rs`, the jaged/CLI path.
 *
 * The platform's normal rename targets a PSI declaration; an interface's
 * "declaration" is a pack line (no PSI element), so we own the whole operation
 * via a [RenameHandler] rather than a PsiElement processor.
 */
class RsInterfaceRenameHandler : RenameHandler {

    override fun isAvailableOnDataContext(dataContext: DataContext): Boolean =
        targetFromContext(dataContext) != null

    override fun isRenaming(dataContext: DataContext): Boolean =
        isAvailableOnDataContext(dataContext)

    override fun invoke(project: Project, editor: Editor?, file: PsiFile?, dataContext: DataContext?) {
        val ctx = dataContext ?: return
        val target = targetFromContext(ctx) ?: return
        runRename(project, target)
    }

    // Non-editor invocation (e.g. from a tree) — not supported for pack symbols.
    override fun invoke(project: Project, elements: Array<out PsiElement>, dataContext: DataContext?) {}

    // ── target detection ──────────────────────────────────────────────────

    /** What the caret is on: a root interface, or one component of one. */
    private sealed interface Target {
        val id: Int

        /** Root `if_549` → `welcome`. */
        data class Root(override val id: Int, val oldName: String) : Target

        /** Component `if_549:com_2` → `if_549:close`. */
        data class Component(
            override val id: Int,
            val parentName: String,
            val child: Int,
            val oldComp: String,
        ) : Target
    }

    private fun targetFromContext(dataContext: DataContext): Target? {
        val project = CommonDataKeys.PROJECT.getData(dataContext) ?: return null
        val editor = CommonDataKeys.EDITOR.getData(dataContext) ?: return null
        val file = CommonDataKeys.PSI_FILE.getData(dataContext) ?: return null
        if (file.fileType !is RuneScriptFileType) return null

        val ident = RuneScriptTokens.of(Fe.IDENTIFIER)
        val selection = editor.selectionModel
        val caret = editor.caretModel.offset

        // Locate the interface identifier token. Probe from the selection start
        // (or caret), backing up one when the offset sits on the token's end
        // boundary (where findElementAt returns the following element).
        fun identAt(offset: Int): PsiElement? =
            file.findElementAt(offset)?.takeIf { it.elementType == ident }
                ?: file.findElementAt((offset - 1).coerceAtLeast(0))?.takeIf { it.elementType == ident }
        val probe = if (selection.hasSelection()) selection.selectionStart else caret
        val token = identAt(probe) ?: return null
        val text = token.text
        val symbols = PackSymbolService.get(project)

        // `parent:child` is one token. Decide which the user means from how far
        // the caret/selection REACHES: past the colon (a selection covering the
        // component, or a caret in the child half) → rename just that component;
        // staying within the `parent` segment → rename the ROOT (cascades).
        val colon = text.indexOf(':')
        val colonAbs = token.textRange.startOffset + colon
        val reach = if (selection.hasSelection()) selection.selectionEnd else caret
        if (colon >= 0 && reach > colonAbs) {
            val id = symbols.interfaceId(text) ?: return null
            return Target.Component(id, text.substring(0, colon), id and 0xFFFF, text.substring(colon + 1))
        }
        val rootName = if (colon >= 0) text.substring(0, colon) else text
        val id = symbols.interfaceId(rootName) ?: return null
        return Target.Root(id, rootName)
    }

    // ── rename ──────────────────────────────────────────────────────────────

    private fun runRename(project: Project, target: Target) {
        val (title, label, oldValue) = when (target) {
            is Target.Root -> Triple("Rename Interface", "New interface name:", target.oldName)
            is Target.Component -> Triple("Rename Component", "New component name:", target.oldComp)
        }
        val newName = Messages.showInputDialog(project, label, title, null, oldValue, IdentValidator)
            ?.trim()
            ?: return
        if (newName.isEmpty() || newName == oldValue) return

        // Files this rename touched — saved (and the symbol cache invalidated)
        // afterwards so a SECOND rename sees the new names: PackSymbolService
        // reads interface.pack from DISK and caches it, so without this the
        // renamed symbol (`welcome`, `welcome:com_2`) wouldn't resolve again.
        val touched = LinkedHashSet<VirtualFile>()
        WriteCommandAction.runWriteCommandAction(project, title, null, {
            when (target) {
                is Target.Root -> renameRoot(project, target, newName, touched)
                is Target.Component -> renameComponent(project, target, newName, touched)
            }
        })
        val fdm = FileDocumentManager.getInstance()
        for (vf in touched) {
            if (vf.isValid) fdm.getCachedDocument(vf)?.let(fdm::saveDocument)
        }
        PackSymbolService.get(project).invalidate()
    }

    private fun renameRoot(project: Project, t: Target.Root, newName: String, touched: MutableSet<VirtualFile>) {
        // 1. interface.pack — root line + cascade `oldName:comp` components.
        editInterfacePack(project, touched) { key, value ->
            when {
                value == t.oldName -> "$key=$newName"
                value.startsWith("${t.oldName}:") -> "$key=$newName:${value.substringAfter(':')}"
                else -> null
            }
        }
        // 2. rename the `.if` file (hybrid stem), located by id.
        findIfFile(project, t.id)?.let { vf ->
            val newFile = "${ifStem(t.id, newName)}.if"
            if (vf.name != newFile) runCatching { vf.rename(this, newFile) }
        }
        // 3. references: `oldName` and `oldName:...` tokens.
        renameRefs(project, touched) { tokenText ->
            when {
                tokenText == t.oldName -> newName
                tokenText.startsWith("${t.oldName}:") -> "$newName:${tokenText.substringAfter(':')}"
                else -> null
            }
        }
    }

    private fun renameComponent(project: Project, t: Target.Component, newName: String, touched: MutableSet<VirtualFile>) {
        val oldFull = "${t.parentName}:${t.oldComp}"
        val newFull = "${t.parentName}:$newName"
        // 1. interface.pack — the one component line (matched by key + value).
        editInterfacePack(project, touched) { key, value ->
            if (key == t.id && value == oldFull) "$key=$newFull" else null
        }
        // 2. `.if` section header `[com_N]` / `[com_N old]` → `[com_N new]`.
        findIfFile(project, t.id ushr 16)?.let { vf ->
            editDocument(project, vf, touched) { text -> rewriteSectionHeader(text, t.child, newName) }
        }
        // 3. references: the whole `parent:old` token.
        renameRefs(project, touched) { tokenText -> if (tokenText == oldFull) newFull else null }
    }

    // ── editing primitives ──────────────────────────────────────────────────

    /** Rewrite `interface.pack` line by line via `map(key, value) -> newLine?`. */
    private fun editInterfacePack(project: Project, touched: MutableSet<VirtualFile>, map: (Int, String) -> String?) {
        val vf = packDir(project)?.findChild("interface.pack") ?: return
        editDocument(project, vf, touched) { text ->
            buildString {
                for (line in text.lines()) {
                    val body = line.substringBefore("//").trim()
                    val eq = body.indexOf('=')
                    val rewritten = if (eq > 0) {
                        val key = body.substring(0, eq).trim().toIntOrNull()
                        val value = body.substring(eq + 1).trim()
                        if (key != null) map(key, value) else null
                    } else null
                    append(rewritten ?: line)
                    append('\n')
                }
            }.let { if (text.endsWith('\n')) it else it.dropLast(1) }
        }
    }

    /** Apply `transform` to every RuneScript file, replacing whole identifier
     *  tokens via `map(tokenText) -> newText?`. Edits run end-to-start so
     *  offsets stay valid. */
    private fun renameRefs(project: Project, touched: MutableSet<VirtualFile>, map: (String) -> String?) {
        val ident = RuneScriptTokens.of(Fe.IDENTIFIER)
        val psiManager = com.intellij.psi.PsiManager.getInstance(project)
        val files = FileTypeIndex.getFiles(RuneScriptFileType.INSTANCE, GlobalSearchScope.projectScope(project))
        for (vf in files) {
            val psi = psiManager.findFile(vf) ?: continue
            val edits = ArrayList<Pair<IntRange, String>>()
            collectLeaves(psi) { leaf ->
                if (leaf.elementType == ident) {
                    map(leaf.text)?.let { edits += leaf.textRange.startOffset..leaf.textRange.endOffset to it }
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

    /** Read `vf`'s document, run `transform`, and write it back if changed. */
    private fun editDocument(project: Project, vf: VirtualFile, touched: MutableSet<VirtualFile>, transform: (String) -> String) {
        val doc: Document = FileDocumentManager.getInstance().getDocument(vf) ?: return
        val updated = transform(doc.text)
        if (updated != doc.text) {
            doc.setText(updated)
            PsiDocumentManager.getInstance(project).commitDocument(doc)
            touched += vf
        }
    }

    private fun collectLeaves(element: PsiElement, visit: (PsiElement) -> Unit) {
        val children = element.children
        if (children.isEmpty()) {
            visit(element)
        } else {
            for (child in children) collectLeaves(child, visit)
        }
    }

    // ── path / format helpers ───────────────────────────────────────────────

    private fun packDir(project: Project): VirtualFile? =
        contentRoot(project)?.findChild("pack")

    private fun contentRoot(project: Project): VirtualFile? {
        val base = project.basePath ?: return null
        return com.intellij.openapi.vfs.LocalFileSystem.getInstance()
            .findFileByPath("$base/Content")
    }

    /** `{id}.if`, else the first `{id}_*.if` (a renamed file). */
    private fun findIfFile(project: Project, id: Int): VirtualFile? {
        val dir = contentRoot(project)?.findChild("interfaces") ?: return null
        dir.findChild("$id.if")?.let { return it }
        val prefix = "${id}_"
        return dir.children.firstOrNull { it.name.startsWith(prefix) && it.name.endsWith(".if") }
    }

    /** Hybrid stem: bare `{id}` for the default `if_{id}`, else `{id}_{name}`. */
    private fun ifStem(id: Int, name: String): String =
        if (name == "if_$id") id.toString() else "${id}_$name"

    /** Rewrite the `[com_{child}]` / `[com_{child} old]` header to carry `name`. */
    private fun rewriteSectionHeader(text: String, child: Int, name: String): String =
        buildString {
            val want = "com_$child"
            for (line in text.lines()) {
                val t = line.trim()
                val inside = t.removeSurrounding("[", "]").takeIf { t.startsWith('[') && t.endsWith(']') }
                if (inside != null && inside.split(Regex("\\s+")).firstOrNull() == want) {
                    append("[com_$child $name]")
                } else {
                    append(line)
                }
                append('\n')
            }
        }.let { if (text.endsWith('\n')) it else it.dropLast(1) }

    private object IdentValidator : com.intellij.openapi.ui.InputValidator {
        override fun checkInput(input: String?): Boolean {
            val s = input?.trim().orEmpty()
            return s.isNotEmpty() &&
                (s[0].isLetter() || s[0] == '_') &&
                s.all { it.isLetterOrDigit() || it == '_' }
        }
        override fun canClose(input: String?): Boolean = checkInput(input)
    }
}
