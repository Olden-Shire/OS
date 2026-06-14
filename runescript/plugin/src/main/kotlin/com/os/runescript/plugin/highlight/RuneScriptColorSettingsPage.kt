package com.os.runescript.plugin.highlight

import com.intellij.openapi.editor.colors.TextAttributesKey
import com.intellij.openapi.fileTypes.SyntaxHighlighter
import com.intellij.openapi.options.colors.AttributesDescriptor
import com.intellij.openapi.options.colors.ColorDescriptor
import com.intellij.openapi.options.colors.ColorSettingsPage
import javax.swing.Icon

/**
 * Settings → Editor → Color Scheme → RuneScript. Lists every RuneScript text
 * attribute (lexer- and annotator-driven) so users can recolour them, with a
 * live demo. Annotator-applied colours (command/config/trigger/var names) are
 * shown via `<tag>` markers mapped below; lexer colours come from the
 * highlighter directly.
 */
class RuneScriptColorSettingsPage : ColorSettingsPage {
    override fun getIcon(): Icon? = null
    override fun getHighlighter(): SyntaxHighlighter = RuneScriptSyntaxHighlighter()
    override fun getDisplayName(): String = "RuneScript"

    override fun getAttributeDescriptors(): Array<AttributesDescriptor> = DESCRIPTORS

    override fun getColorDescriptors(): Array<ColorDescriptor> = ColorDescriptor.EMPTY_ARRAY

    override fun getAdditionalHighlightingTagToDescriptorMap(): Map<String, TextAttributesKey> = TAGS

    override fun getDemoText(): String = """
        // A server script (.rs2)
        [<trigger>proc</trigger>,<config>welcome_screen</config>]
        <command>if_opentop</command>(<config>welcome</config>);
        <command>if_opensub</command>(<config>welcome:account_info_layer</config>, <config>account_info</config>, 1);
        if (<gamevar>%</gamevar><gamevar>option_brightness</gamevar> = ^<constant>brightness_dark</constant>) {
            <gamevar>%</gamevar><gamevar>option_brightness</gamevar> = 2;
        }
        <local>${'$'}</local><local>count</local> = <command>inv_total</command>(inv, <config>coins</config>);
        return;
    """.trimIndent()

    companion object {
        private val DESCRIPTORS = arrayOf(
            AttributesDescriptor("Keyword", RsColors.KEYWORD),
            AttributesDescriptor("String", RsColors.STRING),
            AttributesDescriptor("Number", RsColors.NUMBER),
            AttributesDescriptor("Comment", RsColors.COMMENT),
            AttributesDescriptor("Command", RsColors.COMMAND),
            AttributesDescriptor("Config reference", RsColors.CONFIG),
            AttributesDescriptor("Trigger", RsColors.TRIGGER),
            AttributesDescriptor("Local variable", RsColors.LOCAL_VAR),
            AttributesDescriptor("Game variable", RsColors.GAME_VAR),
            AttributesDescriptor("Constant", RsColors.CONSTANT),
            AttributesDescriptor("Operator", RsColors.OPERATOR),
            AttributesDescriptor("Braces", RsColors.BRACES),
            AttributesDescriptor("Parentheses", RsColors.PARENS),
        )

        private val TAGS = mapOf(
            "trigger" to RsColors.TRIGGER,
            "command" to RsColors.COMMAND,
            "config" to RsColors.CONFIG,
            "local" to RsColors.LOCAL_VAR,
            "gamevar" to RsColors.GAME_VAR,
            "constant" to RsColors.CONSTANT,
        )
    }
}
