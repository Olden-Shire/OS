package com.os.runescript.plugin.asm

import com.intellij.lexer.LexerBase
import com.intellij.psi.tree.IElementType
import com.intellij.psi.tree.IFileElementType
import com.intellij.psi.tree.TokenSet

class Cs2AsmTokenType(debugName: String) : IElementType(debugName, Cs2AsmLanguage)

object Cs2AsmTokens {
    val COMMENT = Cs2AsmTokenType("COMMENT")
    val DIRECTIVE = Cs2AsmTokenType("DIRECTIVE")
    val LABEL = Cs2AsmTokenType("LABEL")
    val MNEMONIC = Cs2AsmTokenType("MNEMONIC")
    val IDENT = Cs2AsmTokenType("IDENT")
    val NUMBER = Cs2AsmTokenType("NUMBER")
    val STRING = Cs2AsmTokenType("STRING")
    val WHITE_SPACE = Cs2AsmTokenType("WHITE_SPACE")
    val BAD = Cs2AsmTokenType("BAD")

    val FILE = IFileElementType(Cs2AsmLanguage)
    val COMMENTS = TokenSet.create(COMMENT)
    val STRINGS = TokenSet.create(STRING)
    val WHITESPACES = TokenSet.create(WHITE_SPACE)
}

/**
 * Hand-rolled highlighting lexer for the line-oriented asm format. Tracks whether the
 * cursor sits at the head of a line so the first identifier becomes a [Cs2AsmTokens.MNEMONIC]
 * and later ones (operands, label references) plain [Cs2AsmTokens.IDENT]s.
 */
class Cs2AsmLexer : LexerBase() {
    private var buffer: CharSequence = ""
    private var endOffset = 0
    private var tokenStart = 0
    private var tokenEnd = 0
    private var tokenType: IElementType? = null

    /** True until the first non-whitespace token of the current line is produced. */
    private var atLineHead = true

    override fun start(buffer: CharSequence, startOffset: Int, endOffset: Int, initialState: Int) {
        this.buffer = buffer
        this.endOffset = endOffset
        tokenStart = startOffset
        tokenEnd = startOffset
        atLineHead = initialState == 0
        advance()
    }

    override fun getState(): Int = if (atLineHead) 0 else 1
    override fun getTokenType(): IElementType? = tokenType
    override fun getTokenStart(): Int = tokenStart
    override fun getTokenEnd(): Int = tokenEnd
    override fun getBufferSequence(): CharSequence = buffer
    override fun getBufferEnd(): Int = endOffset

    override fun advance() {
        tokenStart = tokenEnd
        if (tokenStart >= endOffset) {
            tokenType = null
            return
        }
        val c = buffer[tokenStart]
        var i = tokenStart

        fun isIdent(ch: Char) = ch.isLetterOrDigit() || ch == '_'

        when {
            c == '\n' || c.isWhitespace() -> {
                while (i < endOffset && buffer[i].isWhitespace()) {
                    if (buffer[i] == '\n') atLineHead = true
                    i++
                }
                tokenType = Cs2AsmTokens.WHITE_SPACE
            }
            c == ';' || (c == '/' && i + 1 < endOffset && buffer[i + 1] == '/') -> {
                while (i < endOffset && buffer[i] != '\n') i++
                tokenType = Cs2AsmTokens.COMMENT
                atLineHead = false
            }
            c == '"' -> {
                i++
                while (i < endOffset && buffer[i] != '"' && buffer[i] != '\n') {
                    if (buffer[i] == '\\' && i + 1 < endOffset) i++
                    i++
                }
                if (i < endOffset && buffer[i] == '"') i++
                tokenType = Cs2AsmTokens.STRING
                atLineHead = false
            }
            c == '.' && atLineHead && i + 1 < endOffset && isIdent(buffer[i + 1]) -> {
                i++
                while (i < endOffset && isIdent(buffer[i])) i++
                tokenType = Cs2AsmTokens.DIRECTIVE
                atLineHead = false
            }
            c.isDigit() || c == '-' || c == '*' -> {
                i++
                while (i < endOffset && (buffer[i].isDigit() || buffer[i] == '-')) i++
                tokenType = Cs2AsmTokens.NUMBER
                atLineHead = false
            }
            isIdent(c) -> {
                while (i < endOffset && isIdent(buffer[i])) i++
                if (i < endOffset && buffer[i] == ':') {
                    i++
                    tokenType = Cs2AsmTokens.LABEL
                } else if (atLineHead) {
                    tokenType = Cs2AsmTokens.MNEMONIC
                } else {
                    tokenType = Cs2AsmTokens.IDENT
                }
                atLineHead = false
            }
            else -> {
                i++
                tokenType = Cs2AsmTokens.BAD
                atLineHead = false
            }
        }
        tokenEnd = i
    }
}
