package com.os.runescript.compiler

import com.os.runescript.compiler.clientscript.Cs2Decoder
import com.os.runescript.compiler.clientscript.Cs2Script
import com.os.runescript.compiler.clientscript.Cs2Writer
import com.os.runescript.compiler.clientscript.Decompiler
import com.os.runescript.compiler.codegen.CodeGenerator
import com.os.runescript.compiler.codegen.OpcodeProfile
import com.os.runescript.frontend.diagnostics.Diagnostics
import com.os.runescript.frontend.lexer.Lexer
import com.os.runescript.frontend.parser.Parser
import com.os.runescript.frontend.symbol.ScriptSymbol
import com.os.runescript.frontend.symbol.SymbolTable
import java.io.File
import kotlin.test.Test
import kotlin.test.assertContains
import kotlin.test.assertEquals
import kotlin.test.assertFalse

/**
 * Compiler <-> decompiler fixpoint: source -> cs2 bytes -> decompile -> source'
 * -> cs2 bytes'. The two byte images must be identical for the constructs the
 * compiler emits.
 */
class Cs2RoundTripTest {

    private fun compile(src: String): ByteArray {
        val diag = Diagnostics()
        val toks = Lexer(src, "t.cs2", diag).tokenize()
        val ast = Parser(toks, "t.cs2", diag).parseFile()
        assertFalse(diag.hasErrors(), "parse: ${diag.all}")
        val symbols = SymbolTable.load(commandPack(), emptyList())
        val gen = CodeGenerator(symbols, diag, OpcodeProfile.CLIENT)
        val node = ast.scripts.single()
        symbols.scriptsByName[node.subject] = ScriptSymbol(node.subject, 0, node.trigger)
        val compiled = gen.generate(node, symbols.scriptsByName[node.subject]!!, -1, "t.cs2")
        assertFalse(diag.hasErrors(), "codegen: ${diag.all}")
        return Cs2Writer.write(compiled)
    }

    private fun decode(bytes: ByteArray): Cs2Script = Cs2Decoder.decode(bytes)!!

    private fun roundTrip(src: String): String {
        val bytes1 = compile(src)
        val decompiled = Decompiler(decode(bytes1), "test").decompile()
        val bytes2 = compile(decompiled)
        assertEquals(
            bytes1.toList(), bytes2.toList(),
            "byte fixpoint failed.\n--- decompiled ---\n$decompiled",
        )
        return decompiled
    }

    @Test fun ifElseFixpoint() {
        val out = roundTrip(
            """
            [clientscript,test]
            def_int ${'$'}int0 = 1;
            if (${'$'}int0 > 2) {
                ${'$'}int0 = 5;
            } else {
                ${'$'}int0 = 9;
            }
            """.trimIndent()
        )
        assertContains(out, "if (\$int0 > 2)")
        assertContains(out, "} else {")
    }

    @Test fun whileFixpoint() {
        val out = roundTrip(
            """
            [clientscript,counter]
            def_int ${'$'}int0 = 0;
            while (${'$'}int0 < 10) {
                ${'$'}int0 = calc(${'$'}int0 + 1);
            }
            """.trimIndent()
        )
        assertContains(out, "while (\$int0 < 10)")
        assertContains(out, "calc(\$int0 + 1)")
    }

    @Test fun commandFixpoint() {
        val out = roundTrip(
            """
            [clientscript,greet]
            mes("hello");
            """.trimIndent()
        )
        assertContains(out, "mes(\"hello\");")
    }

    private fun commandPack(): File =
        listOf(File("../data/symbols/clientscript_command.pack"), File("data/symbols/clientscript_command.pack"))
            .first { it.exists() }
}
