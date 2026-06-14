package com.os.runescript.plugin.editor

import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.editor.Document
import com.intellij.openapi.editor.EditorFactory
import com.intellij.openapi.editor.event.DocumentEvent
import com.intellij.openapi.editor.event.DocumentListener
import com.intellij.openapi.editor.ex.EditorEx
import com.intellij.openapi.editor.highlighter.EditorHighlighterFactory
import com.intellij.openapi.fileEditor.FileDocumentManager
import com.intellij.openapi.fileEditor.FileEditor
import com.intellij.openapi.fileEditor.FileEditorState
import com.intellij.openapi.project.Project
import com.intellij.openapi.util.Disposer
import com.intellij.openapi.util.UserDataHolderBase
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.util.Alarm
import com.os.runescript.plugin.asm.Cs2AsmFileType
import com.os.runescript.plugin.cli.OsCli
import java.beans.PropertyChangeListener
import java.nio.file.Files
import javax.swing.JComponent

/**
 * Read-only assembly pane for the side-by-side cs2 editor. Whenever the source
 * document changes (debounced), the current buffer is compiled through the Rust CLI
 * (`app cs2asm`) and the resulting listing replaces the pane's content. Compile/parse
 * errors are shown in place of the listing — effectively live diagnostics from the
 * real compiler.
 */
class Cs2AsmPreviewEditor(
    private val project: Project,
    private val file: VirtualFile,
    sourceDocument: Document,
) : UserDataHolderBase(), FileEditor {
    private val document: Document = EditorFactory.getInstance().createDocument("")
    private val viewer: EditorEx =
        EditorFactory.getInstance().createViewer(document, project) as EditorEx
    private val alarm = Alarm(Alarm.ThreadToUse.POOLED_THREAD, this)

    init {
        viewer.highlighter = EditorHighlighterFactory.getInstance()
            .createEditorHighlighter(project, Cs2AsmFileType.INSTANCE)
        viewer.settings.apply {
            isLineNumbersShown = true
            isLineMarkerAreaShown = false
            isFoldingOutlineShown = false
        }
        sourceDocument.addDocumentListener(
            object : DocumentListener {
                override fun documentChanged(event: DocumentEvent) = scheduleRefresh(event.document)
            },
            this,
        )
        scheduleRefresh(sourceDocument)
    }

    private fun scheduleRefresh(source: Document) {
        val text = source.text
        alarm.cancelAllRequests()
        alarm.addRequest({ refresh(text) }, 400)
    }

    /** Pooled thread: write the buffer to a temp file, compile, publish the listing. */
    private fun refresh(sourceText: String) {
        val listing = compile(sourceText)
        ApplicationManager.getApplication().invokeLater {
            if (viewer.isDisposed) return@invokeLater
            ApplicationManager.getApplication().runWriteAction {
                document.setText(listing.replace("\r\n", "\n"))
            }
        }
    }

    private fun compile(sourceText: String): String {
        if (OsCli.findBinary(project) == null) return OsCli.BUILD_HINT
        val tmp = Files.createTempFile("os_cs2_preview", ".cs2")
        return try {
            Files.writeString(tmp, sourceText)
            val args = buildList {
                add("cs2asm")
                add(tmp.toString())
                contentRoot()?.let {
                    add("--content")
                    add(it)
                }
            }
            val result = OsCli.run(project, *args.toTypedArray())
                ?: return OsCli.BUILD_HINT
            if (result.ok) result.stdout else "; ── does not compile ──\n${result.stderr}"
        } finally {
            Files.deleteIfExists(tmp)
        }
    }

    /** Walk up to the Content root (the dir owning `pack/`) for name/signature tables. */
    private fun contentRoot(): String? {
        var dir = file.parent
        while (dir != null) {
            if (dir.findChild("pack")?.isDirectory == true) return dir.path
            dir = dir.parent
        }
        return null
    }

    override fun getComponent(): JComponent = viewer.component
    override fun getPreferredFocusedComponent(): JComponent = viewer.contentComponent
    override fun getName(): String = "CS2 Assembly"
    override fun setState(state: FileEditorState) {}
    override fun isModified(): Boolean = false
    override fun isValid(): Boolean = true
    override fun addPropertyChangeListener(listener: PropertyChangeListener) {}
    override fun removePropertyChangeListener(listener: PropertyChangeListener) {}
    override fun getFile(): VirtualFile = file

    override fun dispose() {
        Disposer.dispose(alarm)
        if (!viewer.isDisposed) EditorFactory.getInstance().releaseEditor(viewer)
    }

    companion object {
        fun sourceDocumentFor(file: VirtualFile): Document? =
            FileDocumentManager.getInstance().getDocument(file)
    }
}
