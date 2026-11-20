package dev.nimrag.app.engine.store

import android.content.ContentValues
import android.content.Context
import android.database.sqlite.SQLiteDatabase
import android.database.sqlite.SQLiteOpenHelper
import dev.nimrag.app.engine.logic.UploadPolicy

/** One row of the `files` table (IMPLEMENTATION_PLAN.md stage 2d). */
data class FileRow(
    val sha256: String,
    val size: Long,
    val dataType: Int,
    val subType: Int,
    val fileIndex: Int,
    val fileNumber: Int,
    /** Raw Garmin-epoch seconds as the watch gave them; 0 = none. */
    val watchTimestamp: Long,
    val receivedAt: Long,
    val uploadedAt: Long?,
    val attempts: Int,
    val nextAttemptAt: Long,
    val lastError: String?,
)

/** One row of the `copied` table: a watch file copied while Garmin Connect runs alongside (stage 2g). */
data class CopiedRow(
    val dataType: Int,
    val subType: Int,
    val fileNumber: Int,
    val size: Long,
    /** Raw Garmin-epoch seconds, as in the directory entry. */
    val timestamp: Long,
    val sha256: String,
    val copiedAt: Long,
)

data class LogRow(val atMs: Long, val level: Int, val text: String)

/**
 * The phone's SQLite database (framework API, no Room): `files` (stored watch files and
 * their upload state), `copied` (files left for Garmin Connect) and `log`. Safe to call
 * from any thread; every call is one statement or one transaction. Default journal mode and
 * synchronous=FULL, so a committed row is durable before [insertFile] returns.
 */
class EngineDb(context: Context) : SQLiteOpenHelper(context, "nimrag.db", null, VERSION) {
    override fun onCreate(db: SQLiteDatabase) {
        db.execSQL(
            """CREATE TABLE files (
                sha256 TEXT PRIMARY KEY NOT NULL,
                size INTEGER NOT NULL,
                data_type INTEGER NOT NULL,
                sub_type INTEGER NOT NULL,
                file_index INTEGER NOT NULL,
                file_number INTEGER NOT NULL,
                watch_timestamp INTEGER NOT NULL DEFAULT 0,
                received_at INTEGER NOT NULL,
                uploaded_at INTEGER,
                attempts INTEGER NOT NULL DEFAULT 0,
                next_attempt_at INTEGER NOT NULL DEFAULT 0,
                last_error TEXT,
                local_at INTEGER
            )""",
        )
        db.execSQL("CREATE INDEX files_pending ON files(uploaded_at, next_attempt_at)")
        createCopied(db)
        db.execSQL(
            """CREATE TABLE log (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                at INTEGER NOT NULL,
                level INTEGER NOT NULL,
                text TEXT NOT NULL
            )""",
        )
    }

    override fun onUpgrade(db: SQLiteDatabase, oldVersion: Int, newVersion: Int) {
        // 1 -> 2: the `copied` table (Garmin Connect alongside, stage 2g).
        if (oldVersion < 2) createCopied(db)
        // 2 -> 3: when a file went into the local server (personal flavor); NULL = not yet.
        if (oldVersion < 3) db.execSQL("ALTER TABLE files ADD COLUMN local_at INTEGER")
    }

    /**
     * Files copied while Garmin Connect archives them: what the watch's directory entry
     * says (the index is not stable, so it is not kept) and the hash of the stored copy.
     */
    private fun createCopied(db: SQLiteDatabase) {
        db.execSQL(
            """CREATE TABLE IF NOT EXISTS copied (
                data_type INTEGER NOT NULL,
                sub_type INTEGER NOT NULL,
                file_number INTEGER NOT NULL,
                size INTEGER NOT NULL,
                timestamp INTEGER NOT NULL,
                sha256 TEXT NOT NULL,
                copied_at INTEGER NOT NULL,
                PRIMARY KEY (data_type, sub_type, file_number, size, timestamp)
            )""",
        )
    }

    // ---- files ----

    /** Inserts the row unless that hash is already known. Returns true if it was new. */
    fun insertFile(row: FileRow): Boolean {
        val values = ContentValues().apply {
            put("sha256", row.sha256)
            put("size", row.size)
            put("data_type", row.dataType)
            put("sub_type", row.subType)
            put("file_index", row.fileIndex)
            put("file_number", row.fileNumber)
            put("watch_timestamp", row.watchTimestamp)
            put("received_at", row.receivedAt)
        }
        return writableDatabase.insertWithOnConflict("files", null, values, SQLiteDatabase.CONFLICT_IGNORE) != -1L
    }

    /** The oldest file that is due for an upload attempt. */
    fun nextDue(nowMs: Long): FileRow? = readableDatabase.rawQuery(
        "SELECT * FROM files WHERE uploaded_at IS NULL AND next_attempt_at <= ? ORDER BY received_at, sha256 LIMIT 1",
        arrayOf(nowMs.toString()),
    ).use { c -> if (c.moveToFirst()) row(c) else null }

    fun markUploaded(sha256: String, nowMs: Long) {
        val v = ContentValues().apply {
            put("uploaded_at", nowMs)
            put("attempts", 0)
            put("next_attempt_at", 0)
            putNull("last_error")
        }
        writableDatabase.update("files", v, "sha256 = ?", arrayOf(sha256))
    }

    /** Records a failed attempt; [nextAttemptAt] = [UploadPolicy.PARKED] stops automatic retries. */
    fun markFailed(sha256: String, attempts: Int, nextAttemptAt: Long, error: String) {
        val v = ContentValues().apply {
            put("attempts", attempts)
            put("next_attempt_at", nextAttemptAt)
            put("last_error", error)
        }
        writableDatabase.update("files", v, "sha256 = ?", arrayOf(sha256))
    }

    /** Settings changed or "upload now": every pending file is due at once, parked ones too. */
    fun resetBackoff() {
        val v = ContentValues().apply {
            put("next_attempt_at", 0)
            put("attempts", 0)
        }
        writableDatabase.update("files", v, "uploaded_at IS NULL", null)
    }

    /** Earliest automatic retry among pending files, or null (nothing pending or all parked). */
    fun earliestRetryAt(): Long? = readableDatabase.rawQuery(
        "SELECT MIN(next_attempt_at) FROM files WHERE uploaded_at IS NULL AND next_attempt_at < ?",
        arrayOf(UploadPolicy.PARKED.toString()),
    ).use { c -> if (c.moveToFirst() && !c.isNull(0)) c.getLong(0) else null }

    data class Counts(val stored: Int, val uploaded: Int, val pending: Int)

    fun counts(): Counts = readableDatabase.rawQuery(
        "SELECT COUNT(*), COUNT(uploaded_at) FROM files",
        null,
    ).use { c ->
        c.moveToFirst()
        val stored = c.getInt(0)
        val uploaded = c.getInt(1)
        Counts(stored, uploaded, stored - uploaded)
    }

    fun lastUploadAt(): Long? = readableDatabase.rawQuery("SELECT MAX(uploaded_at) FROM files", null)
        .use { c -> if (c.moveToFirst() && !c.isNull(0)) c.getLong(0) else null }

    fun firstPendingError(): String? = readableDatabase.rawQuery(
        "SELECT last_error FROM files WHERE uploaded_at IS NULL AND last_error IS NOT NULL ORDER BY received_at LIMIT 1",
        null,
    ).use { c -> if (c.moveToFirst()) c.getString(0) else null }

    fun recent(limit: Int): List<FileRow> = readableDatabase.rawQuery(
        "SELECT * FROM files ORDER BY received_at DESC, sha256 LIMIT ?",
        arrayOf(limit.toString()),
    ).use { c ->
        buildList { while (c.moveToNext()) add(row(c)) }
    }

    private fun row(c: android.database.Cursor) = FileRow(
        sha256 = c.getString(c.getColumnIndexOrThrow("sha256")),
        size = c.getLong(c.getColumnIndexOrThrow("size")),
        dataType = c.getInt(c.getColumnIndexOrThrow("data_type")),
        subType = c.getInt(c.getColumnIndexOrThrow("sub_type")),
        fileIndex = c.getInt(c.getColumnIndexOrThrow("file_index")),
        fileNumber = c.getInt(c.getColumnIndexOrThrow("file_number")),
        watchTimestamp = c.getLong(c.getColumnIndexOrThrow("watch_timestamp")),
        receivedAt = c.getLong(c.getColumnIndexOrThrow("received_at")),
        uploadedAt = c.getColumnIndexOrThrow("uploaded_at").let { if (c.isNull(it)) null else c.getLong(it) },
        attempts = c.getInt(c.getColumnIndexOrThrow("attempts")),
        nextAttemptAt = c.getLong(c.getColumnIndexOrThrow("next_attempt_at")),
        lastError = c.getColumnIndexOrThrow("last_error").let { if (c.isNull(it)) null else c.getString(it) },
    )

    // ---- local server (personal flavor) ----

    /** The oldest stored file the local server has not ingested yet. */
    fun nextLocalPending(): FileRow? = readableDatabase.rawQuery(
        "SELECT * FROM files WHERE local_at IS NULL ORDER BY received_at, sha256 LIMIT 1",
        null,
    ).use { c -> if (c.moveToFirst()) row(c) else null }

    fun markLocal(sha256: String, nowMs: Long) {
        val v = ContentValues().apply { put("local_at", nowMs) }
        writableDatabase.update("files", v, "sha256 = ?", arrayOf(sha256))
    }

    // ---- copied (alongside) ----

    /** Records a copied file; the same key again only refreshes `copied_at` and the hash. */
    fun insertCopied(row: CopiedRow) {
        val values = ContentValues().apply {
            put("data_type", row.dataType)
            put("sub_type", row.subType)
            put("file_number", row.fileNumber)
            put("size", row.size)
            put("timestamp", row.timestamp)
            put("sha256", row.sha256)
            put("copied_at", row.copiedAt)
        }
        writableDatabase.insertWithOnConflict("copied", null, values, SQLiteDatabase.CONFLICT_REPLACE)
    }

    /** The files copied at or after [sinceMs]: what the next connection leaves alone. */
    fun copiedSince(sinceMs: Long): List<CopiedRow> = readableDatabase.rawQuery(
        "SELECT data_type, sub_type, file_number, size, timestamp, sha256, copied_at FROM copied WHERE copied_at >= ?",
        arrayOf(sinceMs.toString()),
    ).use { c ->
        buildList {
            while (c.moveToNext()) {
                add(CopiedRow(c.getInt(0), c.getInt(1), c.getInt(2), c.getLong(3), c.getLong(4), c.getString(5), c.getLong(6)))
            }
        }
    }

    fun copiedCountSince(sinceMs: Long): Int = readableDatabase.rawQuery(
        "SELECT COUNT(*) FROM copied WHERE copied_at >= ?",
        arrayOf(sinceMs.toString()),
    ).use { c -> if (c.moveToFirst()) c.getInt(0) else 0 }

    /** Drops rows older than [beforeMs]: they no longer go into the skip list. */
    fun pruneCopied(beforeMs: Long) {
        writableDatabase.delete("copied", "copied_at < ?", arrayOf(beforeMs.toString()))
    }

    /** The watch was forgotten: its keys mean nothing for another one. */
    fun clearCopied() {
        writableDatabase.delete("copied", null, null)
    }

    // ---- log ----

    fun insertLog(atMs: Long, level: Int, text: String) {
        val v = ContentValues().apply {
            put("at", atMs)
            put("level", level)
            put("text", text)
        }
        writableDatabase.insert("log", null, v)
    }

    /** Newest first. */
    fun loadLog(limit: Int): List<LogRow> = readableDatabase.rawQuery(
        "SELECT at, level, text FROM log ORDER BY id DESC LIMIT ?",
        arrayOf(limit.toString()),
    ).use { c ->
        buildList { while (c.moveToNext()) add(LogRow(c.getLong(0), c.getInt(1), c.getString(2))) }
    }

    fun trimLog(keep: Int) {
        writableDatabase.execSQL(
            "DELETE FROM log WHERE id <= (SELECT MAX(id) FROM log) - ?",
            arrayOf<Any>(keep),
        )
    }

    private companion object {
        const val VERSION = 3
    }
}
