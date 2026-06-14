package com.os.runescript.frontend

import com.os.runescript.frontend.ast.*
import com.os.runescript.frontend.diagnostics.Diagnostics
import com.os.runescript.frontend.lexer.Lexer
import com.os.runescript.frontend.parser.Parser
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class ParserTest {
    private fun parse(src: String): ScriptFileNode {
        val diag = Diagnostics()
        val toks = Lexer(src, "test.rs2", diag).tokenize()
        val file = Parser(toks, "test.rs2", diag).parseFile()
        assertFalse(diag.hasErrors(), "errors: ${diag.all}")
        return file
    }

    @Test fun basicLogin() {
        val file = parse(
            """
            [login,_]
            mes("Welcome to RuneScape.");
            %musicplay = 1;
            if (%option_brightness = 0) {
                %option_brightness = 2;
            }
            ~initalltabs;

            [proc,initalltabs]
            return;
            """.trimIndent()
        )
        assertEquals(2, file.scripts.size)
        val login = file.scripts[0]
        assertEquals("login", login.trigger)
        assertEquals("_", login.subject)

        val first = login.statements[0] as ExpressionStatement
        val call = first.expression as CommandCallExpression
        assertEquals("mes", call.name)
        assertEquals("Welcome to RuneScape.", (call.arguments[0] as StringLiteral).value)

        val assign = login.statements[1] as AssignmentStatement
        assertEquals("musicplay", (assign.targets[0] as GameVariableExpression).name)
        assertEquals(1, (assign.values[0] as IntegerLiteral).value)

        val iff = login.statements[2] as IfStatement
        val cond = iff.condition as BinaryExpression
        assertEquals(BinaryOp.EQUAL, cond.operator)

        val proc = login.statements[3] as ExpressionStatement
        assertEquals("initalltabs", (proc.expression as ProcCallExpression).name)
    }

    @Test fun calcAndPrecedence() {
        val file = parse("[proc,t]\n\$x = calc(1 + 2 * 3);")
        val assign = file.scripts[0].statements[0] as AssignmentStatement
        val calc = assign.values[0] as CalcExpression
        val add = calc.expression as BinaryExpression
        // 1 + (2 * 3) — multiply binds tighter
        assertEquals(BinaryOp.ADD, add.operator)
        assertEquals(BinaryOp.MUL, (add.right as BinaryExpression).operator)
    }

    @Test fun conditionPrecedence() {
        val file = parse("[proc,t]\nif (%a = 1 & %b > 2) { return; }")
        val iff = file.scripts[0].statements[0] as IfStatement
        // & is lower precedence than = and >, so top node is AND
        val top = iff.condition as BinaryExpression
        assertEquals(BinaryOp.AND, top.operator)
        assertEquals(BinaryOp.EQUAL, (top.left as BinaryExpression).operator)
        assertEquals(BinaryOp.GREATER, (top.right as BinaryExpression).operator)
    }

    @Test fun interfaceComponentAndConstants() {
        val file = parse("[proc,t]\nif_settab(stats, ^tab_skills);")
        val call = (file.scripts[0].statements[0] as ExpressionStatement).expression as CommandCallExpression
        assertEquals("if_settab", call.name)
        assertEquals("stats", (call.arguments[0] as Identifier).name)
        assertEquals("tab_skills", (call.arguments[1] as ConstantVariableExpression).name)
    }

    @Test fun procWithArgs() {
        val file = parse("[login,_]\n~update_all(inv_getobj(worn, ^wearpos_rhand));")
        val proc = (file.scripts[0].statements[0] as ExpressionStatement).expression as ProcCallExpression
        assertEquals("update_all", proc.name)
        val inner = proc.arguments[0] as CommandCallExpression
        assertEquals("inv_getobj", inner.name)
        assertTrue(inner.arguments[1] is ConstantVariableExpression)
    }
}
