package com.os.runescript.plugin.config

import com.intellij.extapi.psi.ASTWrapperPsiElement
import com.intellij.extapi.psi.PsiFileBase
import com.intellij.lang.ASTNode
import com.intellij.lang.Language
import com.intellij.lang.ParserDefinition
import com.intellij.lang.PsiParser
import com.intellij.lexer.LexerBase
import com.intellij.openapi.editor.DefaultLanguageHighlighterColors as D
import com.intellij.openapi.editor.colors.TextAttributesKey
import com.intellij.openapi.editor.colors.TextAttributesKey.createTextAttributesKey
import com.intellij.openapi.fileTypes.LanguageFileType
import com.intellij.openapi.fileTypes.SyntaxHighlighter
import com.intellij.openapi.fileTypes.SyntaxHighlighterBase
import com.intellij.openapi.fileTypes.SyntaxHighlighterFactory
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.psi.FileViewProvider
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.TokenType
import com.intellij.psi.tree.IElementType
import com.intellij.psi.tree.IFileElementType
import com.intellij.psi.tree.TokenSet
import javax.swing.Icon

/**
 * One "config text" language for every readable config the cache unpacks to —
 * `.if` interfaces, `.npc`/`.loc`/`.obj`/`.seq`/`.flo`/`.flu`/`.idk`/`.inv`/
 * `.spot`/`.varbit`/`.varp`/`.enum`. They share a `key = value` shape with
 * `//` comments and optional `[section]` headers, so one lexer/highlighter
 * colours them all: comment, section, key, `=`, numbers, value text.
 */
object ConfigLanguage : Language("RsConfig")

class ConfigFileType private constructor() : LanguageFileType(ConfigLanguage) {
    override fun getName(): String = "RsConfig"
    override fun getDescription(): String = "OSRS config (interface / npc / loc / obj / …)"
    override fun getDefaultExtension(): String = "npc"
    override fun getIcon(): Icon? = null

    companion object {
        @JvmField val INSTANCE = ConfigFileType()
        const val EXTENSIONS = "if;npc;loc;obj;seq;flo;flu;idk;inv;spot;varbit;varp;enum"
    }
}

object ConfigColors {
    val COMMENT = key("RSCFG_COMMENT", D.LINE_COMMENT)
    val SECTION = key("RSCFG_SECTION", D.METADATA)
    val KEY = key("RSCFG_KEY", D.KEYWORD)
    val EQ = key("RSCFG_EQ", D.OPERATION_SIGN)
    val NUMBER = key("RSCFG_NUMBER", D.NUMBER)
    val VALUE = key("RSCFG_VALUE", D.STRING)
    // A `com_N` component reference (e.g. `layer=com_0`) — explicit gold/yellow
    // default, navigable to its `[com_N]` section. Customisable in the page.
    val COMREF = createTextAttributesKey(
        "RSCFG_COMREF",
        com.intellij.openapi.editor.markup.TextAttributes(
            java.awt.Color(0xE8BF6A), null, null, null, java.awt.Font.PLAIN,
        ),
    )
    private fun key(name: String, fallback: TextAttributesKey) = createTextAttributesKey(name, fallback)
}

object ConfigTokens {
    val COMMENT = IElementType("RSCFG_COMMENT", ConfigLanguage)
    val SECTION = IElementType("RSCFG_SECTION", ConfigLanguage)
    val KEY = IElementType("RSCFG_KEY", ConfigLanguage)
    val EQ = IElementType("RSCFG_EQ", ConfigLanguage)
    val NUMBER = IElementType("RSCFG_NUMBER", ConfigLanguage)
    val VALUE = IElementType("RSCFG_VALUE", ConfigLanguage)
    val COMREF = IElementType("RSCFG_COMREF", ConfigLanguage)
    val TEXT = IElementType("RSCFG_TEXT", ConfigLanguage)
    val FILE = IFileElementType(ConfigLanguage)
    val COMMENTS = TokenSet.create(COMMENT)

    /** A component reference value: `com_<digits>`. */
    val COMREF_RE = Regex("com_\\d+")
}

/** Line-aware lexer: a `key` before the first `=` on a line, values after it. */
class ConfigLexer : LexerBase() {
    private data class Seg(val type: IElementType, val start: Int, val end: Int)

    private var buffer: CharSequence = ""
    private var endOffset = 0
    private var segments: List<Seg> = emptyList()
    private var index = 0

    override fun start(buffer: CharSequence, startOffset: Int, endOffset: Int, initialState: Int) {
        this.buffer = buffer
        this.endOffset = endOffset
        val text = buffer.subSequence(startOffset, endOffset).toString()
        val segs = ArrayList<Seg>()
        var i = 0
        var sawEq = false
        fun add(type: IElementType, from: Int, to: Int) = segs.add(Seg(type, startOffset + from, startOffset + to))
        while (i < text.length) {
            val c = text[i]
            val s = i
            when {
                c == '\n' -> { i++; add(TokenType.WHITE_SPACE, s, i); sawEq = false }
                c == ' ' || c == '\t' || c == '\r' -> { while (i < text.length && (text[i] == ' ' || text[i] == '\t' || text[i] == '\r')) i++; add(TokenType.WHITE_SPACE, s, i) }
                c == '/' && i + 1 < text.length && text[i + 1] == '/' -> { while (i < text.length && text[i] != '\n') i++; add(ConfigTokens.COMMENT, s, i) }
                c == '[' -> { while (i < text.length && text[i] != ']' && text[i] != '\n') i++; if (i < text.length && text[i] == ']') i++; add(ConfigTokens.SECTION, s, i) }
                c == '=' -> { i++; sawEq = true; add(ConfigTokens.EQ, s, i) }
                !sawEq -> { while (i < text.length && text[i] != '=' && text[i] != '\n' && !text[i].isWhitespace()) i++; add(ConfigTokens.KEY, s, i) }
                c.isDigit() || (c == '-' && i + 1 < text.length && text[i + 1].isDigit()) -> { i = numEnd(text, s); add(ConfigTokens.NUMBER, s, i) }
                c.isLetter() || c == '_' -> {
                    while (i < text.length && (text[i].isLetterOrDigit() || text[i] == '_')) i++
                    // `com_N` (not `com_iN` model refs) → a component reference.
                    val type = if (ConfigTokens.COMREF_RE.matches(text.substring(s, i))) ConfigTokens.COMREF else ConfigTokens.VALUE
                    add(type, s, i)
                }
                else -> { i++; add(ConfigTokens.TEXT, s, i) }
            }
        }
        segments = segs
        index = 0
    }

    /** End offset of a number starting at `from` (digits, optional 0x hex / '.'). */
    private fun numEnd(text: String, from: Int): Int {
        var j = from
        if (text[j] == '-') j++
        if (j + 1 < text.length && text[j] == '0' && (text[j + 1] == 'x' || text[j + 1] == 'X')) {
            j += 2
            while (j < text.length && (text[j].isLetterOrDigit())) j++
            return j
        }
        while (j < text.length && (text[j].isDigit() || text[j] == '.')) j++
        return j
    }

    override fun getState(): Int = 0
    override fun getTokenType(): IElementType? = segments.getOrNull(index)?.type
    override fun getTokenStart(): Int = segments[index].start
    override fun getTokenEnd(): Int = segments[index].end
    override fun advance() { index++ }
    override fun getBufferSequence(): CharSequence = buffer
    override fun getBufferEnd(): Int = endOffset
}

class ConfigSyntaxHighlighter : SyntaxHighlighterBase() {
    override fun getHighlightingLexer() = ConfigLexer()
    override fun getTokenHighlights(tokenType: IElementType?): Array<TextAttributesKey> = when (tokenType) {
        ConfigTokens.COMMENT -> pack(ConfigColors.COMMENT)
        ConfigTokens.SECTION -> pack(ConfigColors.SECTION)
        ConfigTokens.KEY -> pack(ConfigColors.KEY)
        ConfigTokens.EQ -> pack(ConfigColors.EQ)
        ConfigTokens.NUMBER -> pack(ConfigColors.NUMBER)
        ConfigTokens.VALUE -> pack(ConfigColors.VALUE)
        ConfigTokens.COMREF -> pack(ConfigColors.COMREF)
        else -> TextAttributesKey.EMPTY_ARRAY
    }
}

class ConfigSyntaxHighlighterFactory : SyntaxHighlighterFactory() {
    override fun getSyntaxHighlighter(project: Project?, virtualFile: VirtualFile?): SyntaxHighlighter =
        ConfigSyntaxHighlighter()
}

class ConfigParserDefinition : ParserDefinition {
    override fun createLexer(project: Project?) = ConfigLexer()
    override fun createParser(project: Project?) = PsiParser { root, builder ->
        val marker = builder.mark()
        while (!builder.eof()) builder.advanceLexer()
        marker.done(root)
        builder.treeBuilt
    }

    override fun getFileNodeType(): IFileElementType = ConfigTokens.FILE
    override fun getCommentTokens(): TokenSet = ConfigTokens.COMMENTS
    override fun getStringLiteralElements(): TokenSet = TokenSet.EMPTY
    override fun createElement(node: ASTNode): PsiElement = ASTWrapperPsiElement(node)
    override fun createFile(viewProvider: FileViewProvider): PsiFile = ConfigPsiFile(viewProvider)
}

class ConfigPsiFile(viewProvider: FileViewProvider) : PsiFileBase(viewProvider, ConfigLanguage) {
    override fun getFileType() = ConfigFileType.INSTANCE
    override fun toString(): String = "RsConfig File"
}
