package com.os.runescript.compiler

import com.os.runescript.compiler.codegen.CodeGenerator
import com.os.runescript.compiler.codegen.SubjectMode
import com.os.runescript.compiler.codegen.Trigger
import com.os.runescript.compiler.writer.BinaryWriter
import com.os.runescript.compiler.writer.PackWriter
import com.os.runescript.frontend.ast.ScriptNode
import com.os.runescript.frontend.diagnostics.Diagnostics
import com.os.runescript.frontend.lexer.Lexer
import com.os.runescript.frontend.parser.Parser
import com.os.runescript.frontend.symbol.ScriptSymbol
import com.os.runescript.frontend.symbol.SymbolTable
import java.io.File

/**
 * The end-to-end server-script compilation: lex + parse every `.rs2`, register
 * scripts (assigning ids + trigger lookup keys), generate bytecode against the
 * pack-backed symbol table, then write the `server/script.{dat,idx}` pack.
 */
class Compiler(
    private val symbols: SymbolTable,
    private val diagnostics: Diagnostics,
) {
    private data class Registered(
        val node: ScriptNode,
        val symbol: ScriptSymbol,
        val lookupKey: Long,
        val sourceName: String,
    )

    fun compile(sourceFiles: List<File>, outDir: File): Boolean {
        val registered = mutableListOf<Registered>()
        var nextId = 0

        // Pass 1: parse + register every script so calls can resolve forward.
        for (file in sourceFiles) {
            val text = file.readText()
            val toks = Lexer(text, file.name, diagnostics).tokenize()
            val ast = Parser(toks, file.name, diagnostics).parseFile()
            for (node in ast.scripts) {
                val trigger = Trigger.byName(node.trigger)
                if (trigger == null) {
                    diagnostics.error(file.name, node.triggerSpan, "unknown trigger '${node.trigger}'")
                    continue
                }
                val lookupKey = computeLookupKey(trigger, node, file.name)
                val refName = if (trigger.subjectMode == SubjectMode.NAME) node.subject
                              else "[${node.trigger},${node.subject}]"
                val symbol = ScriptSymbol(refName, nextId++, node.trigger, trigger.subjectType)
                if (symbols.scriptsByName.containsKey(refName)) {
                    diagnostics.error(file.name, node.span, "duplicate script '$refName'")
                }
                symbols.scriptsByName[refName] = symbol
                registered += Registered(node, symbol, lookupKey, file.name)
            }
        }

        if (diagnostics.hasErrors()) return false

        // Pass 2: code generation.
        val gen = CodeGenerator(symbols, diagnostics)
        val blobs = HashMap<Int, ByteArray>()
        for (r in registered) {
            val compiled = gen.generate(r.node, r.symbol, r.lookupKey, r.sourceName)
            if (diagnostics.hasErrors()) continue
            blobs[compiled.id] = BinaryWriter.write(compiled)
        }

        if (diagnostics.hasErrors()) return false

        PackWriter.write(outDir, blobs)
        return true
    }

    private fun computeLookupKey(trigger: Trigger, node: ScriptNode, sourceName: String): Long {
        return when (trigger.subjectMode) {
            SubjectMode.NAME -> -1L
            SubjectMode.NONE -> {
                if (node.subject != "_") {
                    diagnostics.warning(sourceName, node.subjectSpan, "trigger '${node.trigger}' takes no subject")
                }
                trigger.id.toLong()
            }
            SubjectMode.TYPE -> {
                val subjectId = resolveSubjectId(trigger, node, sourceName)
                // type kind 2 = direct config subject (1 = category; not yet
                // authored). Long: component subjects pack (interface<<16)|child,
                // which exceeds Int once shifted past bit 10.
                trigger.id.toLong() or (2L shl 8) or (subjectId.toLong() shl 10)
            }
        }
    }

    private fun resolveSubjectId(trigger: Trigger, node: ScriptNode, sourceName: String): Int {
        val subject = node.subject
        // Component subjects: `interface:child` → (interface << 16) | child.
        // The interface half resolves through interface.pack (or a literal
        // id); the child is the numeric component index within it.
        if (trigger.subjectType == "component") {
            val parts = subject.split(':')
            if (parts.size != 2) {
                diagnostics.error(sourceName, node.subjectSpan,
                    "component subject must be 'interface:child' (e.g. if_378:6 or welcome:com_6), got '$subject'")
                return 0
            }
            val ifaceId = parts[0].toIntOrNull() ?: symbols.config("interface", parts[0])?.id
            if (ifaceId == null) {
                diagnostics.error(sourceName, node.subjectSpan, "unknown interface '${parts[0]}'")
                return 0
            }
            // Child may be a numeric index (welcome:6) or a named component
            // (welcome:com_6). Named components resolve through interface.pack as
            // the full `interface:child` key, which already encodes (iface<<16)|child.
            val child = parts[1].toIntOrNull()
            if (child != null) {
                return (ifaceId shl 16) or child
            }
            val named = symbols.config("interface", subject)?.id
            if (named != null) {
                return named
            }
            diagnostics.error(sourceName, node.subjectSpan,
                "unknown component '$subject' (no '${parts[1]}' in interface '${parts[0]}')")
            return 0
        }
        // coord / mapzone subjects are literal ids.
        if (subject.contains('_') && subject.all { it.isDigit() || it == '_' }) {
            return subject.replace("_", "").toIntOrNull() ?: 0
        }
        val type = trigger.subjectType
        val sym = if (type != null) symbols.config(type, subject) else symbols.config(subject)
        if (sym == null) {
            diagnostics.error(sourceName, node.subjectSpan, "unknown ${type ?: "config"} subject '$subject'")
            return 0
        }
        return sym.id
    }
}
