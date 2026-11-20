package dev.nimrag.app.engine.logic

/** Who asks to start the watch service (see `StartSource` for the log wording). */
enum class StartRequest {
    /** `Application.onCreate`. */
    ProcessStart,

    /** Boot completed or the app was updated. */
    Boot,

    /** The system says the associated watch came into range. */
    Presence,

    /** The permission or Bluetooth state really changed. */
    PermissionsChanged,

    /** The app came to the foreground (every `ON_RESUME`) and nothing changed. */
    AppOpened,
}

enum class StartDecision {
    /** Start the service. */
    Start,

    /** Nothing to do (the service runs or is starting). */
    Skip,

    /** The service runs and the watch just appeared: it only needs a nudge to reconnect now. */
    Nudge,
}

/**
 * Decides whether a request to start the watch service does anything. One process start after
 * an app update used to produce a boot start, a process start and several "permissions changed"
 * starts (every time the app was opened) within seconds; each went through `startForegroundService`
 * and the link's start. Now: while the service runs or was asked to start a moment ago, a request
 * is skipped, and "permissions changed" only counts when they did change.
 *
 * Thread-safe (requests come from the main thread, binder threads and the engine thread).
 * `now` is a monotonic clock in milliseconds, passed in so tests need no real time.
 */
class ServiceStartGate(private val startingWindowMs: Long = 5_000L) {
    private var running = false
    private var requestedAtMs: Long? = null

    @Synchronized
    fun decide(request: StartRequest, now: Long): StartDecision {
        val starting = requestedAtMs?.let { now - it in 0 until startingWindowMs } == true
        val decision = when {
            // A change is a reason on its own, also while the service runs (the link retries now).
            request == StartRequest.PermissionsChanged -> StartDecision.Start
            running -> if (request == StartRequest.Presence) StartDecision.Nudge else StartDecision.Skip
            starting -> if (request == StartRequest.Presence) StartDecision.Nudge else StartDecision.Skip
            else -> StartDecision.Start
        }
        if (decision == StartDecision.Start) requestedAtMs = now
        return decision
    }

    /** `startForegroundService` was refused: the next request may try again at once. */
    @Synchronized
    fun refused() {
        requestedAtMs = null
    }

    /** `WatchService.onStartCommand` ran. */
    @Synchronized
    fun started() {
        running = true
    }

    /** `WatchService.onDestroy` ran. */
    @Synchronized
    fun stopped() {
        running = false
        requestedAtMs = null
    }

    @get:Synchronized
    val isRunning: Boolean get() = running
}
