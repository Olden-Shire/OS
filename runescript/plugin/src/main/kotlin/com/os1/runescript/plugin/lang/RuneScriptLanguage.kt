package com.os1.runescript.plugin.lang

import com.intellij.icons.AllIcons
import com.intellij.lang.Language
import com.intellij.openapi.fileTypes.LanguageFileType
import javax.swing.Icon

/** One language for both server scripts (.rs2) and clientscripts (.cs2). */
object RuneScriptLanguage : Language("RuneScript")

/**
 * Single file type covering both `.rs2` (server) and `.cs2` (client). The
 * server/client distinction is made per-file by extension where it matters
 * (command set), not by a separate language. Canonical `class` + companion
 * `INSTANCE` so the platform's `fieldName="INSTANCE"` lookup is unambiguous.
 */
class RuneScriptFileType private constructor() : LanguageFileType(RuneScriptLanguage) {
    override fun getName(): String = "RuneScript"
    override fun getDescription(): String = "RuneScript (server .rs2 / client .cs2)"
    override fun getDefaultExtension(): String = "rs2"
    override fun getIcon(): Icon = AllIcons.FileTypes.Text

    companion object {
        @JvmField
        val INSTANCE = RuneScriptFileType()

        fun isClientScript(extension: String?): Boolean = extension.equals("cs2", ignoreCase = true)
    }
}
