package com.os1.runescript.compiler

import com.os1.runescript.compiler.clientscript.Cs2Decoder
import com.os1.runescript.compiler.clientscript.Cs2Writer
import com.os1.runescript.compiler.codegen.CodeGenerator
import com.os1.runescript.compiler.codegen.OpcodeProfile
import com.os1.runescript.frontend.diagnostics.Diagnostics
import com.os1.runescript.frontend.lexer.Lexer
import com.os1.runescript.frontend.parser.Parser
import com.os1.runescript.frontend.symbol.ScriptSymbol
import com.os1.runescript.frontend.symbol.SymbolTable
import java.io.File
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull

class ClientScriptCompileTest {

    private fun compileToCs2(src: String): com.os1.runescript.compiler.clientscript.Cs2Script {
        val diag = Diagnostics()
        val toks = Lexer(src, "test.cs2", diag).tokenize()
        val ast = Parser(toks, "test.cs2", diag).parseFile()
        assertFalse(diag.hasErrors(), "frontend errors: ${diag.all}")

        val symbols = SymbolTable.load(commandPack(), emptyList())
        val gen = CodeGenerator(symbols, diag, OpcodeProfile.CLIENT)
        val node = ast.scripts.single()
        val symbol = ScriptSymbol(node.subject, 0, node.trigger)
        symbols.scriptsByName[node.subject] = symbol
        val compiled = gen.generate(node, symbol, lookupKey = -1, sourceName = "test.cs2")
        assertFalse(diag.hasErrors(), "codegen errors: ${diag.all}")

        val bytes = Cs2Writer.write(compiled)
        return Cs2Decoder.decode(bytes) ?: error("decode failed")
    }

    @Test fun coreLanguageRoundTrips() {
        val cs2 = compileToCs2(
            """
            [clientscript,test]
            def_int ${'$'}x = calc(1 + 2);
            if (${'$'}x > 2) {
                ${'$'}x = 5;
            }
            """.trimIndent()
        )

        assertNull(cs2.name, "official caches ship no name")
        assertEquals(1, cs2.intLocalCount, "one int local")
        assertEquals(0, cs2.stringLocalCount)

        // Expected cs2 opcode stream:
        //   push_const_int 1, push_const_int 2, add(4000), pop_int_local 0   -- calc(1+2) -> $x
        //   push_int_local 0, push_const_int 2, branch_le(31) -> else        -- if $x > 2
        //   push_const_int 5, pop_int_local 0                                -- $x = 5
        //   return(21)
        assertEquals(
            listOf(0, 0, 4000, 34, 33, 0, 31, 0, 34, 21),
            cs2.opcodes.toList(),
        )
        // arithmetic uses the cs2 base (4000), not the server base (4600).
        assertEquals(4000, cs2.opcodes[2])
        // the `>` condition compiled to its inverse branch_le to skip the body.
        assertEquals(31, cs2.opcodes[6])
    }

    @Test fun stringAndCommandRoundTrip() {
        // mes(string) — a cs2 command (op>=100, 1-byte secondary flag).
        val cs2 = compileToCs2(
            """
            [clientscript,greet]
            mes("hello");
            """.trimIndent()
        )
        // push_const_string "hello"(3), mes(command), return(21)
        assertEquals(3, cs2.opcodes[0])
        assertEquals("hello", cs2.stringOperands[0])
        assertEquals(21, cs2.opcodes.last())
    }

    private fun commandPack(): File {
        // runescript/data/symbols/clientscript_command.pack relative to the module.
        val candidates = listOf(
            File("../data/symbols/clientscript_command.pack"),
            File("data/symbols/clientscript_command.pack"),
        )
        return candidates.firstOrNull { it.exists() }
            ?: error("clientscript_command.pack not found (cwd=${File(".").absolutePath})")
    }
}
