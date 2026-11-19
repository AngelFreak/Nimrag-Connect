package dev.nimrag.app.engine.store

import android.system.ErrnoException
import android.system.Os
import android.system.OsConstants
import java.io.File
import java.io.IOException

/**
 * Phone storage for watch files: `filesDir/watch/<sha256>.fit`. [save] returns only after
 * the bytes are durable (temp file, fsync, rename, directory fsync) and the SQLite row is
 * committed; the caller then sends `FileStored`, which is what permits the archive request.
 * Bytes that are already stored (same hash) count as stored.
 */
class FileStore(private val dir: File, private val db: EngineDb) {
    data class Saved(val sha256: String, val alreadyStored: Boolean)

    fun fileFor(sha256: String): File = File(dir, "$sha256.fit")

    /** Throws on any failure; the caller must then answer `FileStoreFailed`. */
    fun save(
        dataType: Int,
        subType: Int,
        fileIndex: Int,
        fileNumber: Int,
        garminTimestamp: Long,
        data: ByteArray,
        nowMs: Long,
    ): Saved {
        val sha = DurableWrite.sha256Hex(data)
        DurableWrite.writeAtomic(dir, "$sha.fit", data, ::syncDirectory)
        val isNew = db.insertFile(
            FileRow(
                sha256 = sha,
                size = data.size.toLong(),
                dataType = dataType,
                subType = subType,
                fileIndex = fileIndex,
                fileNumber = fileNumber,
                watchTimestamp = garminTimestamp,
                receivedAt = nowMs,
                uploadedAt = null,
                attempts = 0,
                nextAttemptAt = 0,
                lastError = null,
            ),
        )
        return Saved(sha, alreadyStored = !isNew)
    }

    private fun syncDirectory(directory: File) {
        try {
            val fd = Os.open(directory.path, OsConstants.O_RDONLY, 0)
            try {
                Os.fsync(fd)
            } finally {
                Os.close(fd)
            }
        } catch (e: ErrnoException) {
            throw IOException("fsync of the directory failed: ${e.message}", e)
        }
    }
}
