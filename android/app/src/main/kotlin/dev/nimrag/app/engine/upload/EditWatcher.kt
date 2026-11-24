package dev.nimrag.app.engine.upload

import dev.nimrag.app.engine.logic.EditCountRule
import dev.nimrag.app.engine.logic.EditOutbox
import java.util.concurrent.Executors
import java.util.concurrent.ScheduledFuture
import java.util.concurrent.TimeUnit

/**
 * Notices edits made on the phone's copy. The local server runs in this process and has no
 * callback into Kotlin, so while the app is on screen (the only time the page can be used) this
 * looks at the outbox's count every [PERIOD_MS]: a cheap local database count. A new edit wakes
 * the upload worker; any change of the count is published for the banner. One more look runs
 * when the app leaves the screen, so an edit made just before is not missed.
 */
class EditWatcher(
    private val outbox: () -> EditOutbox?,
    /** Also run on every look while on screen (the test inbox of debuggable builds). */
    private val alsoEachPoll: () -> Unit = {},
    /** The count changed ([wake]: a new edit is there to send). Called on the watcher's thread. */
    private val onChange: (count: Int, wake: Boolean) -> Unit,
) {
    private val executor = Executors.newSingleThreadScheduledExecutor { r ->
        Thread(r, "nimrag-edits").apply { isDaemon = true }
    }
    private var task: ScheduledFuture<*>? = null

    @Volatile private var last: Int? = null

    /** The app came to the screen (true) or left it (false). Any thread. */
    @Synchronized
    fun setForeground(on: Boolean) {
        task?.cancel(false)
        task = null
        if (on) {
            last = null
            task = executor.scheduleWithFixedDelay(::poll, 0, PERIOD_MS, TimeUnit.MILLISECONDS)
        } else {
            executor.execute(::poll)
        }
    }

    private fun poll() {
        runCatching { alsoEachPoll() }
        val count = try {
            outbox()?.pending()
        } catch (_: Exception) {
            null
        } ?: return
        val decision = EditCountRule.decide(last, count)
        last = count
        if (decision.publish || decision.wake) runCatching { onChange(count, decision.wake) }
    }

    private companion object {
        const val PERIOD_MS = 2_000L
    }
}
