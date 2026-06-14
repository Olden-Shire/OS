package com.os.runescript.plugin.lang

import com.intellij.lexer.LexerBase
import com.intellij.psi.TokenType
import com.intellij.psi.tree.IElementType
import com.os.runescript.frontend.diagnostics.Diagnostics
import com.os.runescript.frontend.lexer.Lexer
import com.os.runescript.frontend.lexer.TokenType as FeTokenType

/**
 * IntelliJ lexer driven by the shared frontend [Lexer]. The frontend drops
 * whitespace/comments, but IntelliJ requires gap-free coverage, so this adapter
 * re-fills the gaps with WHITE_SPACE / comment tokens.
 */
class RuneScriptLexerAdapter : LexerBase() {
    private data class Seg(val type: IElementType, val start: Int, val end: Int)

    private var buffer: CharSequence = ""
    private var endOffset = 0
    private var segments: List<Seg> = emptyList()
    private var index = 0

    override fun start(buffer: CharSequence, startOffset: Int, endOffset: Int, initialState: Int) {
        this.buffer = buffer
        this.endOffset = endOffset
        val text = buffer.subSequence(startOffset, endOffset).toString()
        val toks = Lexer(text, "<editor>", Diagnostics()).tokenize()

        val segs = ArrayList<Seg>()
        var cursor = 0
        for (t in toks) {
            if (t.type == FeTokenType.EOF) break
            val s = t.span.start
            val e = t.span.end
            if (s > cursor) fillGap(segs, text, cursor, s, startOffset)
            if (e > s) segs.add(Seg(RuneScriptTokens.of(t.type), startOffset + s, startOffset + e))
            cursor = maxOf(cursor, e)
        }
        if (cursor < text.length) fillGap(segs, text, cursor, text.length, startOffset)

        segments = segs
        index = 0
    }

    /** Re-tokenize a [from,to) gap into whitespace and comment runs. */
    private fun fillGap(segs: ArrayList<Seg>, text: String, from: Int, to: Int, base: Int) {
        var i = from
        while (i < to) {
            val c = text[i]
            when {
                c == '/' && i + 1 < to && text[i + 1] == '/' -> {
                    var j = i + 2
                    while (j < to && text[j] != '\n') j++
                    segs.add(Seg(RuneScriptTokens.LINE_COMMENT, base + i, base + j))
                    i = j
                }
                c == '/' && i + 1 < to && text[i + 1] == '*' -> {
                    var j = i + 2
                    while (j < to && !(text[j] == '*' && j + 1 < to && text[j + 1] == '/')) j++
                    j = minOf(to, j + 2)
                    segs.add(Seg(RuneScriptTokens.BLOCK_COMMENT, base + i, base + j))
                    i = j
                }
                else -> {
                    var j = i
                    while (j < to && text[j] != '/' && text[j].isWhitespace()) j++
                    if (j == i) j++ // any stray char — keep gap-free
                    segs.add(Seg(TokenType.WHITE_SPACE, base + i, base + j))
                    i = j
                }
            }
        }
    }

    override fun getState(): Int = 0
    override fun getTokenType(): IElementType? = segments.getOrNull(index)?.type
    override fun getTokenStart(): Int = segments[index].start
    override fun getTokenEnd(): Int = segments[index].end
    override fun advance() { index++ }
    override fun getBufferSequence(): CharSequence = buffer
    override fun getBufferEnd(): Int = endOffset
}
