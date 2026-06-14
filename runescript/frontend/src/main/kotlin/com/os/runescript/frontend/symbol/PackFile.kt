package com.os.runescript.frontend.symbol

import java.io.File

/**
 * Reader for Lost City `.pack` files — one `id=name` record per line, `//`
 * comments. This is the on-disk metadata format shared by the cache pipeline
 * (`crates/cache`), this compiler, and the future IntelliJ plugin: a single
 * authoritative name<->id mapping per type, sourced from our pack.
 */
object PackFile {
    data class Entry(val id: Int, val name: String)

    fun read(file: File): List<Entry> {
        if (!file.exists()) return emptyList()
        val entries = mutableListOf<Entry>()
        file.forEachLine { raw ->
            val line = raw.substringBefore("//").trim()
            if (line.isEmpty()) return@forEachLine
            val eq = line.indexOf('=')
            require(eq > 0) { "${file.name}: missing '=' in '$raw'" }
            val id = line.substring(0, eq).trim().toIntOrNull()
                ?: error("${file.name}: invalid id in '$raw'")
            entries += Entry(id, line.substring(eq + 1).trim())
        }
        return entries
    }
}
