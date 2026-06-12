package com.os1.runescript.plugin.asm

import com.intellij.icons.AllIcons
import com.intellij.lang.Language
import com.intellij.openapi.fileTypes.LanguageFileType
import javax.swing.Icon

/**
 * The `.cs2asm` clientscript assembly listing — the packer's faithful fallback format
 * and the right-hand pane of the side-by-side cs2 editor. One instruction per line,
 * branch targets as labels, `.directives` for the header:
 *
 * ```
 * .int_args 1
 *   push_varbit 3756
 *   branch_less_than label_00
 * label_00:
 *   gosub script_113
 *   return
 * ```
 */
object Cs2AsmLanguage : Language("Cs2Asm")

class Cs2AsmFileType private constructor() : LanguageFileType(Cs2AsmLanguage) {
    override fun getName(): String = "Cs2Asm"
    override fun getDescription(): String = "ClientScript assembly listing (.cs2asm)"
    override fun getDefaultExtension(): String = "cs2asm"
    override fun getIcon(): Icon = AllIcons.FileTypes.Text

    companion object {
        @JvmField
        val INSTANCE = Cs2AsmFileType()
    }
}
