package com.os.runescript.plugin.config

import com.intellij.codeInsight.completion.CompletionContributor
import com.intellij.codeInsight.completion.CompletionParameters
import com.intellij.codeInsight.completion.CompletionProvider
import com.intellij.codeInsight.completion.CompletionResultSet
import com.intellij.codeInsight.completion.CompletionType
import com.intellij.codeInsight.lookup.LookupElementBuilder
import com.intellij.openapi.Disposable
import com.intellij.openapi.components.Service
import com.intellij.openapi.components.service
import com.intellij.openapi.project.Project
import com.intellij.openapi.util.TextRange
import com.intellij.openapi.vfs.VfsUtilCore
import com.intellij.openapi.vfs.VirtualFileManager
import com.intellij.openapi.vfs.newvfs.BulkFileListener
import com.intellij.openapi.vfs.newvfs.events.VFileEvent
import com.intellij.patterns.PlatformPatterns
import com.intellij.psi.search.FileTypeIndex
import com.intellij.psi.search.GlobalSearchScope
import com.intellij.util.ProcessingContext
import java.util.concurrent.ConcurrentHashMap

/**
 * Key completion for config files. The valid keys per config type live in the
 * Rust `config_text` schemas; rather than duplicate them, we HARVEST the keys
 * actually used across the project's config files of the same extension
 * (`.npc` → npc keys, `.if` → interface keys), cached per type and dropped when
 * a config file changes. Offered only at the key position (line start, before
 * the `=`).
 */
class ConfigCompletionContributor : CompletionContributor() {
    init {
        extend(
            CompletionType.BASIC,
            PlatformPatterns.psiElement().withLanguage(ConfigLanguage),
            object : CompletionProvider<CompletionParameters>() {
                override fun addCompletions(
                    parameters: CompletionParameters,
                    context: ProcessingContext,
                    result: CompletionResultSet,
                ) {
                    val ext = parameters.originalFile.virtualFile?.extension ?: return
                    if (atKeyPosition(parameters)) {
                        for (key in ConfigKeyService.get(parameters.position.project).keysFor(ext)) {
                            result.addElement(LookupElementBuilder.create(key).withTypeText(ext))
                        }
                        return
                    }
                    // Value position: offer this file's `com_N` components (for
                    // layer/overlayer refs). Gated on a `c…` prefix to stay quiet
                    // on numeric values.
                    if (valuePrefix(parameters).startsWith("c", ignoreCase = true)) {
                        for ((com, name) in componentSections(parameters.originalFile.text)) {
                            val lb = LookupElementBuilder.create(com).withTypeText("component")
                            result.addElement(if (name != null) lb.withTailText(" $name", true) else lb)
                        }
                    }
                }
            },
        )
    }

    /** True when the caret is before the first `=` on its line (the key half),
     *  and not on a comment or section line. */
    private fun atKeyPosition(parameters: CompletionParameters): Boolean {
        val doc = parameters.editor.document
        val offset = parameters.offset
        val lineStart = doc.getLineStartOffset(doc.getLineNumber(offset))
        val prefix = doc.getText(TextRange(lineStart, offset))
        if (prefix.contains('=')) return false
        val trimmed = prefix.trimStart()
        return !trimmed.startsWith("//") && !trimmed.startsWith("[")
    }

    /** The value being typed on this line (text after the last `=`, to caret). */
    private fun valuePrefix(parameters: CompletionParameters): String {
        val doc = parameters.editor.document
        val offset = parameters.offset
        val lineStart = doc.getLineStartOffset(doc.getLineNumber(offset))
        return doc.getText(TextRange(lineStart, offset)).substringAfterLast('=', "").trim()
    }

    /** `[com_N name]` section headers in `text` → `(com_N, name?)`. */
    private fun componentSections(text: String): List<Pair<String, String?>> {
        val out = ArrayList<Pair<String, String?>>()
        for (raw in text.lineSequence()) {
            val t = raw.trim()
            if (t.startsWith("[") && t.endsWith("]")) {
                val parts = t.substring(1, t.length - 1).trim().split(Regex("\\s+"), limit = 2)
                if (parts[0].matches(Regex("com_\\d+"))) out.add(parts[0] to parts.getOrNull(1))
            }
        }
        return out
    }
}

/** Per-extension harvested key vocabulary, cached and VFS-invalidated. */
@Service(Service.Level.PROJECT)
class ConfigKeyService(private val project: Project) : Disposable {
    private val cache = ConcurrentHashMap<String, Set<String>>()

    init {
        project.messageBus.connect(this).subscribe(
            VirtualFileManager.VFS_CHANGES,
            object : BulkFileListener {
                override fun after(events: List<VFileEvent>) {
                    if (events.any { it.path.substringAfterLast('.') in EXTS }) cache.clear()
                }
            },
        )
    }

    fun keysFor(ext: String): Set<String> = cache.getOrPut(ext) { harvest(ext) }

    private fun harvest(ext: String): Set<String> {
        val keys = sortedSetOf<String>()
        val scope = GlobalSearchScope.projectScope(project)
        val files = FileTypeIndex.getFiles(ConfigFileType.INSTANCE, scope)
            .filter { it.extension.equals(ext, ignoreCase = true) }
        for (vf in files.take(500)) {
            val text = runCatching { VfsUtilCore.loadText(vf) }.getOrNull() ?: continue
            for (raw in text.lineSequence()) {
                val line = raw.trim()
                if (line.isEmpty() || line.startsWith("//") || line.startsWith("[")) continue
                val eq = line.indexOf('=')
                if (eq <= 0) continue
                val key = line.substring(0, eq).trim()
                if (key.isNotEmpty() && key.all { it.isLetterOrDigit() || it == '_' }) keys.add(key)
            }
        }
        return keys
    }

    override fun dispose() {}

    companion object {
        private val EXTS = ConfigFileType.EXTENSIONS.split(';').toSet()
        fun get(project: Project): ConfigKeyService = project.service()
    }
}
