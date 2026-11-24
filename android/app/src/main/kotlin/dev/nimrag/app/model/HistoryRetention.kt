package dev.nimrag.app.model

/**
 * "Server history on this phone" (Data retention, docs/design/history-on-phone.md): how much of
 * what only the server has is copied onto the phone so it shows offline. The watch's own data is
 * always kept, whatever the choice. [days] is the window by when the data happened; null for
 * [Off] (nothing is copied) and [Everything] (no window).
 */
enum class HistoryRetention(val key: String, val days: Int?) {
    Off("off", null),
    Last3Months("3m", 91),
    Last12Months("12m", 365),
    Everything("all", null),
    ;

    /** True when anything is copied at all. */
    val copies: Boolean get() = this != Off

    /** How far back the choice reaches, for comparing two of them: Off 0, up to Everything 3. */
    val reach: Int get() = ordinal

    /** The oldest data time (unix seconds) a file may have to be copied; null: no limit. */
    fun sinceSeconds(nowMs: Long): Long? = days?.let { nowMs / 1000 - it * 86_400L }

    companion object {
        /** An unknown or missing stored value is Off: nothing is copied by accident. */
        fun fromKey(key: String?): HistoryRetention = entries.firstOrNull { it.key == key } ?: Off
    }
}
