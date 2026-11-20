package dev.nimrag.app.engine.logic

/** Reconnect delays: 2 s, doubling, capped at 5 minutes; reset after a finished handshake. */
class Backoff(private val firstMs: Long = 2_000L, private val maxMs: Long = 300_000L) {
    private var nextMs = firstMs

    /** The delay to use now; the following call gives double (up to the cap). */
    fun next(): Long {
        val now = nextMs
        nextMs = minOf(nextMs * 2, maxMs)
        return now
    }

    fun reset() {
        nextMs = firstMs
    }
}

/** Small decisions of the link logic that need no Android types. */
object LinkRules {
    /** `first_connect` is set for the first connection after bonding and one more retry. */
    const val FIRST_CONNECT_ATTEMPTS = 2

    /** [triesSoFar] connections already used `first_connect` without PairingCompleted. */
    fun firstConnectFor(triesSoFar: Int): Boolean = triesSoFar < FIRST_CONNECT_ATTEMPTS

    /**
     * The state of the `first_connect` budget. A connection counts against it only once its
     * handshake finished (`HandshakeDone`): ChannelInUse retries and early drops never get
     * as far, so they cost nothing.
     */
    data class FirstConnect(val stillSet: Boolean, val tries: Int)

    /** Call on `HandshakeDone` of a connection that carried `first_connect`. */
    fun afterHandshake(triesSoFar: Int): FirstConnect {
        val tries = triesSoFar + 1
        return FirstConnect(stillSet = firstConnectFor(tries), tries = tries)
    }

    /**
     * After the handshake: request a sync when auto-sync is on and the last finished sync is
     * older than [autoSyncMinutes] (or there never was one, or the clock went backwards).
     */
    fun shouldStartSync(lastFinishedAtMs: Long?, nowMs: Long, autoSyncMinutes: Int): Boolean {
        if (autoSyncMinutes <= 0) return false
        if (lastFinishedAtMs == null || lastFinishedAtMs > nowMs) return true
        return nowMs - lastFinishedAtMs >= autoSyncMinutes * 60_000L
    }

    /**
     * The last finished sync must be at least this old before opening the app starts one (a
     * sync from a minute ago already has what the watch holds).
     */
    const val OPEN_SYNC_MIN_AGE_MS = 5 * 60_000L

    /**
     * A sync was asked for and the watch has not said it started yet: asking again within this
     * time would only queue a second cycle (the client queues a request that arrives while one runs).
     */
    const val SYNC_ASK_WINDOW_MS = 60_000L

    /** The link is ready, no sync runs or was just asked for, and [shouldStartSync] says it is due. */
    fun shouldStartAutoSync(
        linkReady: Boolean,
        syncBusy: Boolean,
        lastFinishedAtMs: Long?,
        nowMs: Long,
        autoSyncMinutes: Int,
    ): Boolean = linkReady && !syncBusy && shouldStartSync(lastFinishedAtMs, nowMs, autoSyncMinutes)

    /**
     * The app came to the foreground: sync when the link is ready, nothing runs and the last
     * finished sync is older than [OPEN_SYNC_MIN_AGE_MS] (or there never was one, or the clock
     * went backwards). Auto-sync Off stays off: then the phone only syncs on a tap or when the
     * watch asks.
     */
    fun shouldSyncOnOpen(
        linkReady: Boolean,
        syncBusy: Boolean,
        lastFinishedAtMs: Long?,
        nowMs: Long,
        autoSyncMinutes: Int,
    ): Boolean {
        if (!linkReady || syncBusy || autoSyncMinutes <= 0) return false
        if (lastFinishedAtMs == null || lastFinishedAtMs > nowMs) return true
        return nowMs - lastFinishedAtMs >= OPEN_SYNC_MIN_AGE_MS
    }

    /** A sync was asked for [sinceAskMs] ago (null: never) and the watch has not started it yet. */
    fun syncAskPending(sinceAskMs: Long?): Boolean = sinceAskMs != null && sinceAskMs in 0 until SYNC_ASK_WINDOW_MS

    /**
     * How long until the next wake-up of the auto-sync alarm: when the interval since the last
     * finished sync runs out, but never sooner than [MIN_ALARM_DELAY_MS], and a full interval
     * when there was no sync, the clock went backwards or the sync is already due (the check
     * that follows the alarm starts it, so the next one is a whole interval away).
     */
    fun nextAutoSyncDelayMs(lastFinishedAtMs: Long?, nowMs: Long, autoSyncMinutes: Int): Long {
        val interval = autoSyncMinutes.coerceAtLeast(1) * 60_000L
        if (lastFinishedAtMs == null || lastFinishedAtMs > nowMs) return interval
        val remaining = lastFinishedAtMs + interval - nowMs
        return if (remaining <= 0) interval else remaining.coerceAtLeast(MIN_ALARM_DELAY_MS)
    }

    const val MIN_ALARM_DELAY_MS = 30_000L

    /** The watch's software version number as shown to people: 1722 becomes "17.22". */
    fun formatSoftwareVersion(version: Int): String = "${version / 100}.${"%02d".format(version % 100)}"
}

/**
 * The write queue of the GATT link: values go out one at a time, the next one only after
 * the previous write completed (`onCharacteristicWrite`). The sender hands one value to the
 * stack and says what happened.
 */
class WriteQueue(private val send: (ByteArray) -> SendResult) {
    enum class SendResult { Started, Busy, Failed }

    enum class Pump { Idle, Sent, Busy, Failed }

    private val queue = ArrayDeque<ByteArray>()
    private var inFlight = false

    val size: Int get() = queue.size + if (inFlight) 1 else 0

    fun enqueue(values: List<ByteArray>) {
        queue.addAll(values)
    }

    /** Hands the head to the stack if nothing is in flight. [Pump.Busy] keeps it queued; retry later. */
    fun pump(): Pump {
        if (inFlight) return Pump.Idle
        val head = queue.firstOrNull() ?: return Pump.Idle
        return when (send(head)) {
            SendResult.Started -> {
                queue.removeFirst()
                inFlight = true
                Pump.Sent
            }
            SendResult.Busy -> Pump.Busy
            SendResult.Failed -> Pump.Failed
        }
    }

    /** `onCharacteristicWrite`: the stack is done with the value; sends the next one. */
    fun onWritten(success: Boolean): Pump {
        inFlight = false
        if (!success) return Pump.Failed
        return pump()
    }

    fun clear() {
        queue.clear()
        inFlight = false
    }
}
