package dev.nimrag.app.engine.phone.logic

/** Find my phone: how long the phone rings, and the state of one ringing. */
object FindPhoneRules {
    const val DEFAULT_SECONDS = 30
    const val MIN_SECONDS = 10
    const val MAX_SECONDS = 120

    /** The watch names a duration (0 when it names none); the phone never rings less than 10 s or more than 2 min. */
    fun ringSeconds(fromWatch: Int): Int =
        if (fromWatch <= 0) DEFAULT_SECONDS else fromWatch.coerceIn(MIN_SECONDS, MAX_SECONDS)

    /** Wait 0, buzz 0.7 s, rest 0.5 s, repeating from the second entry. */
    val VIBRATION_PATTERN = longArrayOf(0, 700, 500)
    const val VIBRATION_REPEAT_INDEX = 1
}

/**
 * One ringing at a time: a second request while ringing only moves the end, and a stop that
 * finds nothing ringing is a no-op. Time is passed in (elapsed-realtime ms), never read.
 */
class FindPhoneSession {
    var ringing = false
        private set
    private var endsAtMs = 0L

    /** True when this call started the ringing (false: it was already going; only the end moved). */
    fun start(nowMs: Long, seconds: Int): Boolean {
        val wasRinging = ringing
        ringing = true
        endsAtMs = nowMs + seconds * 1000L
        return !wasRinging
    }

    /** True when something was ringing. */
    fun stop(): Boolean {
        val wasRinging = ringing
        ringing = false
        return wasRinging
    }

    /** Milliseconds until the ringing ends by itself; 0 when it is over or nothing rings. */
    fun remainingMs(nowMs: Long): Long = if (ringing) (endsAtMs - nowMs).coerceAtLeast(0) else 0
}
