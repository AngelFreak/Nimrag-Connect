package dev.nimrag.app.engine.store

import java.io.File

/**
 * Testing without a watch, debuggable builds only: a FIT file put in `files/test-inbox/` is stored
 * exactly like a file the watch sent (the file store, then the upload worker and the phone's own
 * copy are told), so a sync can be played on a phone that has no watch.
 *
 * The folder is in the app's private storage: only `adb shell run-as dev.nimrag.app`, which already
 * has the whole app's data, can write there. Name a file `<dataType>-<subType>-<index>-<fileNumber>-<garminTimestamp>.fit`
 * (e.g. `128-32-1-1-1100000000.fit`); a file named otherwise is stored as a monitoring file (128/32)
 * made now. Each file is removed once stored.
 */
class TestInbox(
    filesDir: File,
    private val store: FileStore,
    private val log: EngineLog,
    /** A file was stored: wake the uploads and the phone's copy, as after a watch download. */
    private val onStored: () -> Unit,
    private val enabled: Boolean,
) {
    private val dir = File(filesDir, "test-inbox")

    /** Store what is waiting. Not on the main thread. */
    fun poll() {
        if (!enabled) return
        val files = dir.listFiles { f -> f.isFile && f.name.endsWith(".fit") }?.sortedBy { it.name } ?: return
        var stored = 0
        for (f in files) {
            val meta = TestInboxName.parse(f.name, System.currentTimeMillis())
            try {
                val saved = store.save(
                    dataType = meta.dataType,
                    subType = meta.subType,
                    fileIndex = meta.fileIndex,
                    fileNumber = meta.fileNumber,
                    garminTimestamp = meta.garminTimestamp,
                    data = f.readBytes(),
                    nowMs = System.currentTimeMillis(),
                )
                log.info("Test inbox: stored a ${meta.dataType}/${meta.subType} file${if (saved.alreadyStored) ", already on the phone" else ""}")
                stored++
            } catch (e: Exception) {
                log.error("Test inbox: could not store a file: ${e.javaClass.simpleName}")
            }
            f.delete()
        }
        if (stored > 0) onStored()
    }
}

/** The file name of a [TestInbox] file as its watch directory entry. Pure, for tests. */
object TestInboxName {
    data class Meta(val dataType: Int, val subType: Int, val fileIndex: Int, val fileNumber: Int, val garminTimestamp: Long)

    /** Seconds between 1970-01-01 and the Garmin epoch (1989-12-31). */
    private const val GARMIN_EPOCH_S = 631_065_600L

    fun parse(name: String, nowMs: Long): Meta {
        val parts = name.removeSuffix(".fit").split('-')
        val nums = parts.mapNotNull { it.toLongOrNull() }
        if (parts.size == 5 && nums.size == 5) {
            return Meta(nums[0].toInt(), nums[1].toInt(), nums[2].toInt(), nums[3].toInt(), nums[4])
        }
        return Meta(128, 32, 0, 0, nowMs / 1000 - GARMIN_EPOCH_S)
    }
}
