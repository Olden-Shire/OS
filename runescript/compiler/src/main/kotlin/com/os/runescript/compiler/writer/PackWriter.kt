package com.os.runescript.compiler.writer

import java.io.ByteArrayOutputStream
import java.io.DataOutputStream
import java.io.File

/**
 * Writes the `server/script.dat` + `server/script.idx` pair the engine loads
 * in `crates/engine/src/script/provider.rs`:
 *
 *   script.dat = int32 count, int32 compilerVersion(27), then each blob in id order
 *   script.idx = int32 count, then int32 size per id (0 = absent)
 *
 * v27: per-script lookup keys are int64 (component subjects pack
 * (interface<<16)|child, which overflows 32 bits once shifted <<10).
 */
object PackWriter {
    const val COMPILER_VERSION = 27

    /** @param blobs id -> serialized script blob. Ids must be dense 0..n-1. */
    fun write(outDir: File, blobs: Map<Int, ByteArray>) {
        val count = (blobs.keys.maxOrNull() ?: -1) + 1

        val dat = ByteArrayOutputStream()
        val datOut = DataOutputStream(dat)
        datOut.writeInt(count)
        datOut.writeInt(COMPILER_VERSION)

        val idx = ByteArrayOutputStream()
        val idxOut = DataOutputStream(idx)
        idxOut.writeInt(count)

        for (id in 0 until count) {
            val blob = blobs[id]
            if (blob == null) {
                idxOut.writeInt(0)
            } else {
                idxOut.writeInt(blob.size)
                datOut.write(blob)
            }
        }

        val serverDir = File(outDir, "server").apply { mkdirs() }
        File(serverDir, "script.dat").writeBytes(dat.toByteArray())
        File(serverDir, "script.idx").writeBytes(idx.toByteArray())
    }
}
