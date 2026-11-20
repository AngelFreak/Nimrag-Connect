package dev.nimrag.app.engine.logic

/** The steps of bringing a GATT link up, in order. Each one has its own deadline and its own log name. */
enum class LinkStep(val label: String) {
    /** `connectGatt` until `onConnectionStateChange(CONNECTED)`. */
    Connecting("connecting"),

    /** `requestMtu` until `onMtuChanged`. */
    MtuExchange("MTU exchange"),

    /** `discoverServices` until `onServicesDiscovered`. */
    ServiceDiscovery("service discovery"),

    /** CCCD write until `onDescriptorWrite`. */
    Notifications("enabling notifications"),
}

/** What to do about a step that did not finish in time. */
sealed interface StallAction {
    /** Only the MTU exchange can be skipped: the link works with the default of 23 bytes. */
    data object SkipMtu : StallAction

    /** Drop the connection and reconnect; [recovery] says how. */
    data class Reconnect(val recovery: Recovery) : StallAction
}

/**
 * How the next connection differs after [attempt] stalls in a row: [refreshCache] asks the stack to
 * forget its cached service list of the watch (right after the connection came up), [directConnect]
 * connects without `autoConnect` (a fresh, immediate connection instead of the background one).
 */
data class Recovery(val attempt: Int, val refreshCache: Boolean, val directConnect: Boolean)

/**
 * Per-step deadlines and the escalation ladder for a link that connects but never finishes
 * setting up (the owner's 0.4.1 log: "Connected" and then nothing for 20 s, over and over).
 * Pure Kotlin: the Android side only asks "what now?" and does it.
 *
 * Ladder (consecutive stalls, reset by a finished handshake): 1 plain retry with a fresh client,
 * 2 also refresh the GATT cache, 3 and later also connect directly (no `autoConnect`).
 */
class LinkSupervisor {
    var stalls = 0
        private set

    /** The next connection should be a direct one (set by the ladder, cleared when it fails to connect). */
    var directConnect = false
        private set

    /** The next connection should refresh the GATT cache once it is up. */
    var refreshCache = false
        private set

    /** A step stalled. [mtuSkipped]: the MTU exchange was already skipped on this connection. */
    fun onStall(step: LinkStep, mtuSkipped: Boolean): StallAction {
        if (step == LinkStep.MtuExchange && !mtuSkipped) return StallAction.SkipMtu
        stalls += 1
        val recovery = Recovery(
            attempt = stalls,
            refreshCache = stalls >= REFRESH_FROM,
            directConnect = stalls >= DIRECT_FROM,
        )
        refreshCache = recovery.refreshCache
        directConnect = recovery.directConnect
        return StallAction.Reconnect(recovery)
    }

    /** The cache refresh was done on a connection. */
    fun refreshDone() {
        refreshCache = false
    }

    /** A direct connection did not even connect (the watch is away): back to the background connection. */
    fun connectFailed() {
        directConnect = false
    }

    /** The handshake finished: the link is healthy, forget the stalls. */
    fun healthy() {
        stalls = 0
        directConnect = false
        refreshCache = false
    }

    companion object {
        const val REFRESH_FROM = 2
        const val DIRECT_FROM = 3

        const val MTU_TIMEOUT_MS = 4_000L
        const val DISCOVERY_TIMEOUT_MS = 12_000L
        const val NOTIFICATIONS_TIMEOUT_MS = 8_000L

        /** A direct connect to a watch that is away ends by itself after about 30 s; do not wait that long. */
        const val DIRECT_CONNECT_TIMEOUT_MS = 20_000L

        /**
         * The deadline of [step], or null when it has none: a background (`autoConnect`) connection
         * waits as long as it takes for the watch to come into range.
         */
        fun timeoutMs(step: LinkStep, direct: Boolean): Long? = when (step) {
            LinkStep.Connecting -> if (direct) DIRECT_CONNECT_TIMEOUT_MS else null
            LinkStep.MtuExchange -> MTU_TIMEOUT_MS
            LinkStep.ServiceDiscovery -> DISCOVERY_TIMEOUT_MS
            LinkStep.Notifications -> NOTIFICATIONS_TIMEOUT_MS
        }
    }
}
