package com.os.runescript.plugin.config

import com.intellij.openapi.editor.colors.TextAttributesKey
import com.intellij.openapi.fileTypes.SyntaxHighlighter
import com.intellij.openapi.options.colors.AttributesDescriptor
import com.intellij.openapi.options.colors.ColorDescriptor
import com.intellij.openapi.options.colors.ColorSettingsPage
import javax.swing.Icon

/**
 * Settings → Editor → Color Scheme → OSRS Config. Lets users recolour the
 * config-text attributes (comment / section / key / value / number) with a live
 * demo over a sample npc/interface config.
 */
class ConfigColorSettingsPage : ColorSettingsPage {
    override fun getIcon(): Icon? = null
    override fun getHighlighter(): SyntaxHighlighter = ConfigSyntaxHighlighter()
    override fun getDisplayName(): String = "OSRS Config"
    override fun getAttributeDescriptors(): Array<AttributesDescriptor> = DESCRIPTORS
    override fun getColorDescriptors(): Array<ColorDescriptor> = ColorDescriptor.EMPTY_ARRAY
    override fun getAdditionalHighlightingTagToDescriptorMap(): Map<String, TextAttributesKey>? = null

    override fun getDemoText(): String = """
        // npc 0
        name = Hans
        vislevel = 0
        size = 1
        walkanims = 819, 820, 821, 822
        recol = 8078/792 8741/1950 43072/4550
        op1 = Talk-to

        [com_2 close_button]
        type=6
        model=com_i2595
        layer=com_0
        x=92
        width=32
        colour=0xff0000
    """.trimIndent()

    companion object {
        private val DESCRIPTORS = arrayOf(
            AttributesDescriptor("Comment", ConfigColors.COMMENT),
            AttributesDescriptor("Section header", ConfigColors.SECTION),
            AttributesDescriptor("Key", ConfigColors.KEY),
            AttributesDescriptor("Assignment", ConfigColors.EQ),
            AttributesDescriptor("Number", ConfigColors.NUMBER),
            AttributesDescriptor("Value", ConfigColors.VALUE),
            AttributesDescriptor("Component reference (com_N)", ConfigColors.COMREF),
            AttributesDescriptor("Pack reference (seq_/model_/renamed)", ConfigColors.PACKREF),
        )
    }
}
