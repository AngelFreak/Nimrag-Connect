package dev.nimrag.app.engine.logic

/** Settings that older versions stored and this one no longer has. No Android types. */
object RemovedSettings {
    /** The preference key of the Garmin Connect mode (Off or Alongside), gone in 0.8.0. */
    const val GARMIN_CONNECT_MODE_KEY = "garmin_connect_mode"

    /** The one log line for a person who had Alongside saved. */
    const val ALONGSIDE_REMOVED = "Alongside Garmin Connect was removed: Nimrag is now the only sync app"

    /** Only Alongside was a real choice; Off was the default and is what everyone has now. */
    fun alongsideWasStored(stored: String?): Boolean = stored == "Alongside"
}
