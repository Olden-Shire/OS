package com.os1.runescript.plugin.action

import com.intellij.openapi.actionSystem.ActionUpdateThread
import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.ui.Messages
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.testFramework.LightVirtualFile
import com.os1.runescript.plugin.cli.Os1Cli
import com.os1.runescript.plugin.lang.RuneScriptFileType

/**
 * Decompiles a raw cs2 bytecode file (e.g. a cache group dumped from archive 12) into
 * structured RuneScript via the Rust `app cs2src` CLI — the same verified pipeline the
 * packer uses — and opens the result in a new editor tab.
 */
class DecompileClientScriptAction : AnAction() {
    override fun getActionUpdateThread(): ActionUpdateThread = ActionUpdateThread.BGT

    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return
        val file: VirtualFile? = e.getData(CommonDataKeys.VIRTUAL_FILE)
        if (file == null || file.isDirectory) {
            Messages.showErrorDialog(project, "Select a cs2 bytecode file first.", "Decompile ClientScript")
            return
        }
        if (Os1Cli.findBinary(project) == null) {
            Messages.showErrorDialog(project, Os1Cli.BUILD_HINT, "Decompile ClientScript")
            return
        }

        ApplicationManager.getApplication().executeOnPooledThread {
            val result = Os1Cli.run(project, "cs2src", file.path, timeoutSeconds = 60)
            ApplicationManager.getApplication().invokeLater {
                when {
                    result == null -> Messages.showErrorDialog(project, Os1Cli.BUILD_HINT, "Decompile ClientScript")
                    !result.ok -> Messages.showErrorDialog(
                        project,
                        result.stderr.ifBlank { "cs2src failed (exit ${result.exitCode})" },
                        "Decompile ClientScript",
                    )
                    else -> {
                        val name = file.nameWithoutExtension
                        val scratch = LightVirtualFile("$name.cs2", RuneScriptFileType.INSTANCE, result.stdout)
                        FileEditorManager.getInstance(project).openFile(scratch, true)
                    }
                }
            }
        }
    }
}
