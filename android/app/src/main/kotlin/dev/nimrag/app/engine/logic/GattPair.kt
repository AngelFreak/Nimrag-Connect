package dev.nimrag.app.engine.logic

import java.util.UUID
import kotlin.random.Random

/**
 * The multi-link GATT service and its characteristic pairs (PROTOCOL.md §1.1): data
 * characteristics notify `6A4E2810`–`2819`; the write characteristic is the same UUID with
 * the first `1` replaced by `2` (`2810` becomes `2820`).
 */
object GattPair {
    private const val TAIL = "-667B-11E3-949A-0800200C9A66"

    val SERVICE: UUID = uuid(0x2800)

    /** Lowest and highest 16-bit id of a notify characteristic. */
    const val NOTIFY_FIRST = 0x2810
    const val NOTIFY_LAST = 0x2819

    private const val WRITE_OFFSET = 0x10

    /** `6A4E<id>-667B-11E3-949A-0800200C9A66`. */
    fun uuid(id: Int): UUID = UUID.fromString("6A4E%04X%s".format(id, TAIL))

    /** The 16-bit id (`2810`..`2819`) if [uuid] is one of our notify characteristics. */
    fun notifyId(uuid: UUID): Int? {
        val id = shortId(uuid) ?: return null
        return if (id in NOTIFY_FIRST..NOTIFY_LAST) id else null
    }

    /** The write characteristic that belongs to notify id [notifyId]. */
    fun writeUuidFor(notifyId: Int): UUID = uuid(notifyId + WRITE_OFFSET)

    /**
     * `CloseReason.ChannelInUse(retry_char)`: the 16-bit field names the characteristic to
     * retry on. Accepts the notify id (`2810`..`2819`) or, leniently, its write id
     * (`2820`..`2829`). Anything else gives null.
     */
    fun notifyIdFromRetryChar(retryChar: Int): Int? = when (retryChar) {
        in NOTIFY_FIRST..NOTIFY_LAST -> retryChar
        in (NOTIFY_FIRST + WRITE_OFFSET)..(NOTIFY_LAST + WRITE_OFFSET) -> retryChar - WRITE_OFFSET
        else -> null
    }

    /**
     * Choose the notify id to use among the characteristics the service exposes.
     * A pair needs both its notify and its write characteristic. [forced] (from a
     * `ChannelInUse` retry) wins when that pair is usable; otherwise a random usable pair,
     * avoiding [avoid] (a pair that just reported "in use") when another exists.
     * Null if no pair is usable.
     */
    fun choose(
        present: Collection<UUID>,
        forced: Int? = null,
        avoid: Int? = null,
        random: Random = Random.Default,
    ): Int? {
        val ids = present.mapNotNull { shortId(it) }.toSet()
        val usable = (NOTIFY_FIRST..NOTIFY_LAST).filter { it in ids && (it + WRITE_OFFSET) in ids }
        if (usable.isEmpty()) return null
        if (forced != null && forced in usable) return forced
        val preferred = usable.filter { it != avoid }
        return (preferred.ifEmpty { usable }).random(random)
    }

    /** The 16-bit id if [uuid] is `6A4Exxxx-667B-11E3-949A-0800200C9A66`. */
    private fun shortId(uuid: UUID): Int? {
        val text = uuid.toString().uppercase()
        if (!text.startsWith("6A4E") || !text.endsWith(TAIL)) return null
        return text.substring(4, 8).toIntOrNull(16)
    }
}
