package dev.nimrag.app.engine.store

import dev.nimrag.app.model.LogLine
import java.util.concurrent.Executors

/**
 * The engine's log: the newest 500 lines in memory (published through [onChange]) and a
 * SQLite table trimmed to 1000 rows. Safe from any thread; database writes happen on their
 * own thread. Never pass it the watch's unit id, the token or other secrets.
 */
class EngineLog(
    private val db: EngineDb,
    private val clock: () -> Long = System::currentTimeMillis,
    private val onChange: (List<LogLine>) -> Unit,
) {
    private val writer = Executors.newSingleThreadExecutor { r -> Thread(r, "nimrag-log").apply { isDaemon = true } }
    private val lock = Any()
    private var lines: List<LogLine> = emptyList()
    private var sinceTrim = 0

    /** Reads the saved lines; call once from a background thread at start. */
    fun load() {
        val saved = try {
            db.loadLog(MEMORY_LINES).map { LogLine(it.atMs, levelOf(it.level), it.text) }
        } catch (_: Exception) {
            emptyList()
        }
        val snapshot = synchronized(lock) {
            lines = (lines + saved).take(MEMORY_LINES)
            lines
        }
        onChange(snapshot)
    }

    fun info(text: String) = add(LogLine.Level.Info, text)

    fun warn(text: String) = add(LogLine.Level.Warning, text)

    fun error(text: String) = add(LogLine.Level.Error, text)

    private fun add(level: LogLine.Level, text: String) {
        val line = LogLine(clock(), level, text)
        val snapshot = synchronized(lock) {
            lines = (listOf(line) + lines).take(MEMORY_LINES)
            lines
        }
        onChange(snapshot)
        writer.execute {
            try {
                db.insertLog(line.atMs, level.ordinal, line.text)
                if (++sinceTrim >= TRIM_EVERY) {
                    sinceTrim = 0
                    db.trimLog(DB_LINES)
                }
            } catch (_: Exception) {
                // The log must never take the engine down.
            }
        }
    }

    private fun levelOf(ordinal: Int): LogLine.Level =
        LogLine.Level.entries.getOrElse(ordinal) { LogLine.Level.Info }

    private companion object {
        const val MEMORY_LINES = 500
        const val DB_LINES = 1000
        const val TRIM_EVERY = 50
    }
}
