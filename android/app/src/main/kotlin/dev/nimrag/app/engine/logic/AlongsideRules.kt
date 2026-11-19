package dev.nimrag.app.engine.logic

import dev.nimrag.app.model.GarminConnectMode
import dev.nimrag.app.model.SyncResult

/** What the Garmin Connect mode means for the link (stage 2g). No Android types. */
object AlongsideRules {
    /** Copied files older than this are listed again by the watch and fetched anew; the store dedupes them. */
    const val SKIP_WINDOW_MS = 60L * 24 * 60 * 60 * 1000

    /** Off archives each stored file on the watch; Alongside never does (Garmin Connect does). */
    fun archive(mode: GarminConnectMode): Boolean = mode == GarminConnectMode.Off

    /**
     * Garmin Connect owns the pairing in Alongside, so no first-connection events are sent
     * then. [wanted] is the budget's answer; the saved flag is not touched either way.
     */
    fun firstConnect(mode: GarminConnectMode, wanted: Boolean): Boolean = wanted && mode == GarminConnectMode.Off

    /** The oldest `copied_at` (wall-clock ms) whose key still goes into the skip list. */
    fun skipSince(nowMs: Long): Long = nowMs - SKIP_WINDOW_MS

    /** The hint "Garmin Connect probably holds the watch" only makes sense when Nimrag wants it alone. */
    fun hintsOtherApp(mode: GarminConnectMode): Boolean = mode == GarminConnectMode.Off

    /** A stored value back to a mode; anything unknown or missing is the default, Off. */
    fun modeFromStored(name: String?): GarminConnectMode =
        GarminConnectMode.entries.firstOrNull { it.name == name } ?: GarminConnectMode.Off

    /** The one log line per finished cycle; skipped files are summed up here, never logged one by one. */
    fun syncSummary(result: SyncResult, mode: GarminConnectMode): String = when (mode) {
        GarminConnectMode.Off -> buildString {
            append("Sync finished: ${result.downloaded} downloaded, ${result.archived} archived, ${result.failed} failed")
            if (result.skipped > 0) append(", ${result.skipped} skipped")
        }
        GarminConnectMode.Alongside -> buildString {
            append("Sync finished: ${result.downloaded} copied, ${result.failed} failed")
            if (result.skipped > 0) append(", ${result.skipped} skipped (copied before)")
        }
    }
}
