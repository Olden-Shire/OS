package com.os1.runescript.plugin.editor

import com.intellij.openapi.fileEditor.FileEditor
import com.intellij.openapi.fileEditor.FileEditorPolicy
import com.intellij.openapi.fileEditor.FileEditorProvider
import com.intellij.openapi.fileEditor.TextEditor
import com.intellij.openapi.fileEditor.TextEditorWithPreview
import com.intellij.openapi.fileEditor.impl.text.TextEditorProvider
import com.intellij.openapi.project.DumbAware
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile

/**
 * Side-by-side editor for structured `.cs2` clientscript source: the regular
 * RuneScript text editor on the left, a live assembly listing (compiled through the
 * Rust `app cs2asm` CLI) on the right. The standard editor/preview/both toggle in the
 * toolbar applies, so "source only" stays one click away.
 */
class Cs2PreviewEditorProvider : FileEditorProvider, DumbAware {
    override fun accept(project: Project, file: VirtualFile): Boolean =
        file.extension.equals("cs2", ignoreCase = true)

    override fun createEditor(project: Project, file: VirtualFile): FileEditor {
        val textEditor = TextEditorProvider.getInstance().createEditor(project, file) as TextEditor
        val sourceDocument = textEditor.editor.document
        val preview = Cs2AsmPreviewEditor(project, file, sourceDocument)
        return TextEditorWithPreview(
            textEditor,
            preview,
            "CS2 Source/Asm",
            TextEditorWithPreview.Layout.SHOW_EDITOR_AND_PREVIEW,
        )
    }

    override fun getEditorTypeId(): String = "os1-cs2-source-asm"

    override fun getPolicy(): FileEditorPolicy = FileEditorPolicy.HIDE_DEFAULT_EDITOR
}
