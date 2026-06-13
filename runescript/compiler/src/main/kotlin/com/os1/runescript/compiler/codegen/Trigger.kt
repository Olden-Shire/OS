package com.os1.runescript.compiler.codegen

/** How a trigger's subject participates in the lookup key. */
enum class SubjectMode { NONE, NAME, TYPE }

/**
 * Server trigger metadata. Ids mirror the engine's `trigger.rs`; subject modes
 * mirror RuneScriptTS's `ServerTriggerType`. The lookup key the runtime
 * dispatches on is `trigger.id | (kind << 8) | (subjectId << 10)`.
 */
data class Trigger(
    val name: String,
    val id: Int,
    val subjectMode: SubjectMode,
    /** For TYPE triggers: the config type the subject must be (e.g. "npc"). */
    val subjectType: String? = null,
) {
    companion object {
        // The triggers needed so far. Extend as more script kinds are authored.
        private val ALL = listOf(
            Trigger("proc", 0, SubjectMode.NAME),
            Trigger("label", 1, SubjectMode.NAME),
            Trigger("debugproc", 2, SubjectMode.NAME),
            Trigger("queue", 116, SubjectMode.NAME),
            Trigger("softtimer", 137, SubjectMode.NAME),
            Trigger("timer", 138, SubjectMode.NAME),
            Trigger("walktrigger", 155, SubjectMode.NAME),
            Trigger("login", 157, SubjectMode.NONE),
            Trigger("logout", 158, SubjectMode.NONE),
            Trigger("advancestat", 160, SubjectMode.TYPE, "stat"),
            Trigger("opnpc1", 10, SubjectMode.TYPE, "npc"),
            Trigger("opobj1", 38, SubjectMode.TYPE, "obj"),
            Trigger("oploc1", 66, SubjectMode.TYPE, "loc"),
            Trigger("opheld1", 140, SubjectMode.TYPE, "obj"),
            // Component subjects: `interface:child` (e.g. if_378:6), packed
            // (interface << 16) | child — the id the rev1 client sends in
            // IF_BUTTON (155) and the engine dispatches on.
            Trigger("if_button", 147, SubjectMode.TYPE, "component"),
            Trigger("if_close", 148, SubjectMode.TYPE, "component"),
        )
        private val BY_NAME = ALL.associateBy { it.name }
        fun byName(name: String): Trigger? = BY_NAME[name]

        /** Every known server trigger — for IDE validation + completion. */
        fun all(): List<Trigger> = ALL
    }
}
