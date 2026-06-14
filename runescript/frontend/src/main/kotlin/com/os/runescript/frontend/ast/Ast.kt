package com.os.runescript.frontend.ast

import com.os.runescript.frontend.lexer.SourceSpan

/**
 * RuneScript AST. Shapes follow `RuneScriptParser.g4` (the Neptune grammar
 * RuneScriptTS mirrors). Kept deliberately plain so the IntelliJ plugin can
 * walk these nodes for navigation/inspection.
 */
sealed interface Node {
    val span: SourceSpan
}

// ── top level ─────────────────────────────────────────────────────────

class ScriptFileNode(val scripts: List<ScriptNode>, override val span: SourceSpan) : Node

class ScriptNode(
    val trigger: String,
    val triggerSpan: SourceSpan,
    /** Subject after the comma, e.g. `_`, a config name, or a coord. */
    val subject: String,
    val subjectSpan: SourceSpan,
    /** `*` after the name — a global/override marker. */
    val global: Boolean,
    val parameters: List<ParameterNode>,
    val returnTypes: List<String>,
    val statements: List<StatementNode>,
    override val span: SourceSpan,
) : Node

class ParameterNode(val type: String, val name: String, override val span: SourceSpan) : Node

// ── statements ────────────────────────────────────────────────────────

sealed interface StatementNode : Node

class BlockStatement(val statements: List<StatementNode>, override val span: SourceSpan) : StatementNode

class ReturnStatement(val values: List<ExpressionNode>, override val span: SourceSpan) : StatementNode

class IfStatement(
    val condition: ExpressionNode,
    val then: StatementNode,
    val otherwise: StatementNode?,
    override val span: SourceSpan,
) : StatementNode

class WhileStatement(
    val condition: ExpressionNode,
    val body: StatementNode,
    override val span: SourceSpan,
) : StatementNode

class SwitchStatement(
    val type: String,
    val subject: ExpressionNode,
    val cases: List<SwitchCaseNode>,
    override val span: SourceSpan,
) : StatementNode

class SwitchCaseNode(
    /** null = `default`. */
    val keys: List<ExpressionNode>?,
    val statements: List<StatementNode>,
    override val span: SourceSpan,
) : Node

class DeclarationStatement(
    val type: String,
    val name: String,
    val initializer: ExpressionNode?,
    override val span: SourceSpan,
) : StatementNode

class AssignmentStatement(
    val targets: List<ExpressionNode>,
    val values: List<ExpressionNode>,
    override val span: SourceSpan,
) : StatementNode

class ExpressionStatement(val expression: ExpressionNode, override val span: SourceSpan) : StatementNode

class EmptyStatement(override val span: SourceSpan) : StatementNode

// ── expressions ───────────────────────────────────────────────────────

sealed interface ExpressionNode : Node

class IntegerLiteral(val value: Int, override val span: SourceSpan) : ExpressionNode
class CoordLiteral(val raw: String, override val span: SourceSpan) : ExpressionNode
class BooleanLiteral(val value: Boolean, override val span: SourceSpan) : ExpressionNode
class CharLiteral(val value: Char, override val span: SourceSpan) : ExpressionNode
class NullLiteral(override val span: SourceSpan) : ExpressionNode

/** A plain string with no interpolation. */
class StringLiteral(val value: String, override val span: SourceSpan) : ExpressionNode

/** A string containing `<expr>` interpolation and/or tags. */
class JoinedStringExpression(val parts: List<ExpressionNode>, override val span: SourceSpan) : ExpressionNode

/** A bare identifier — resolves to a config symbol, constant, or script name. */
class Identifier(val name: String, override val span: SourceSpan) : ExpressionNode

class LocalVariableExpression(val name: String, override val span: SourceSpan) : ExpressionNode
class LocalArrayVariableExpression(val name: String, val index: ExpressionNode, override val span: SourceSpan) : ExpressionNode

/** `%var` or `.%var`. */
class GameVariableExpression(val name: String, val dot: Boolean, override val span: SourceSpan) : ExpressionNode

/** `^constant`. */
class ConstantVariableExpression(val name: String, override val span: SourceSpan) : ExpressionNode

class CommandCallExpression(
    val name: String,
    val arguments: List<ExpressionNode>,
    /** For `name*(args)(triggers)` clientscript-arg form. */
    val clientTriggers: List<ExpressionNode>?,
    override val span: SourceSpan,
) : ExpressionNode

class ProcCallExpression(val name: String, val arguments: List<ExpressionNode>, override val span: SourceSpan) : ExpressionNode
class JumpCallExpression(val name: String, val arguments: List<ExpressionNode>, override val span: SourceSpan) : ExpressionNode

class BinaryExpression(
    val left: ExpressionNode,
    val operator: BinaryOp,
    val right: ExpressionNode,
    override val span: SourceSpan,
) : ExpressionNode

/** `calc(...)` arithmetic. Same node shape; flagged so codegen knows context. */
class CalcExpression(val expression: ExpressionNode, override val span: SourceSpan) : ExpressionNode

enum class BinaryOp(val symbol: String) {
    ADD("+"), SUB("-"), MUL("*"), DIV("/"), MOD("%"),
    AND("&"), OR("|"),
    EQUAL("="), NOT_EQUAL("!"),
    LESS("<"), GREATER(">"), LESS_EQUAL("<="), GREATER_EQUAL(">="),
}
