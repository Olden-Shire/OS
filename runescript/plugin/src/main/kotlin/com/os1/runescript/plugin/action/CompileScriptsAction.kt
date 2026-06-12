package com.os1.runescript.plugin.action

import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.progress.ProgressIndicator
import com.intellij.openapi.progress.ProgressManager
import com.intellij.openapi.progress.Task
import com.intellij.openapi.ui.Messages
import com.os1.runescript.compiler.Compiler
import com.os1.runescript.frontend.diagnostics.Diagnostics
import com.os1.runescript.frontend.symbol.SymbolTable
import com.os1.runescript.plugin.symbol.PackSymbolService
import java.io.File

/** Compiles the project's server-script source set to `data/pack`. */
class CompileScriptsAction : AnAction() {
    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return
        val base = project.basePath?.let { File(it) } ?: return
        val src = base.resolve("content/scripts")
        val out = base.resolve("data/pack")
        val commandPack = base.resolve("runescript/data/symbols/command.pack")
        val packs = listOf(base.resolve("Content/pack")).filter { it.isDirectory }

        if (!src.isDirectory) { Messages.showErrorDialog(project, "No source dir: $src", "Compile RuneScript"); return }
        if (!commandPack.isFile) { Messages.showErrorDialog(project, "No command pack: $commandPack", "Compile RuneScript"); return }

        ProgressManager.getInstance().run(object : Task.Backgroundable(project, "Compiling RuneScript", true) {
            override fun run(indicator: ProgressIndicator) {
                val sources = src.walkTopDown().filter { it.isFile && it.extension == "rs2" }.toList()
                val symbols = SymbolTable.load(commandPack, packs)
                val diagnostics = Diagnostics()
                val ok = Compiler(symbols, diagnostics).compile(sources, out)
                PackSymbolService.get(project).invalidate()

                val message = if (ok && !diagnostics.hasErrors())
                    "Compiled ${sources.size} file(s) -> ${out.resolve("server")}"
                else
                    "Failed (${diagnostics.errorCount} error(s)):\n" +
                        diagnostics.all.take(20).joinToString("\n") { "${it.sourceName}:${it.span.line}: ${it.message}" }

                com.intellij.openapi.application.ApplicationManager.getApplication().invokeLater {
                    if (ok && !diagnostics.hasErrors()) Messages.showInfoMessage(project, message, "Compile RuneScript")
                    else Messages.showErrorDialog(project, message, "Compile RuneScript")
                }
            }
        })
    }
}
