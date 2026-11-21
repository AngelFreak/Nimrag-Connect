package dev.nimrag.app.engine.workouts

import dev.nimrag.app.engine.Hub
import dev.nimrag.app.engine.ble.WatchLink
import dev.nimrag.app.engine.logic.UploadPolicy
import dev.nimrag.app.engine.workouts.logic.FitFetch
import dev.nimrag.app.engine.workouts.logic.QueueFetch
import dev.nimrag.app.engine.workouts.logic.WorkoutApi
import dev.nimrag.app.engine.workouts.logic.WorkoutHttp
import dev.nimrag.app.engine.workouts.logic.WorkoutSendRules
import uniffi.nimrag_ffi.UploadResult
import java.io.IOException
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean

/**
 * Takes workouts from the server to the watch (stage 4a). The owner builds one on the web and
 * presses "Send to watch"; the server queues it. While the link to the watch is ready this
 * looks at the queue (right after the handshake, then every [WorkoutSendRules.POLL_INTERVAL_MS]),
 * fetches each queued workout as an encoded FIT file, hands it to the protocol client
 * (`Input.UploadFile`, file type 128/5) through [WatchLink.sendFile], and reports the outcome
 * back (`sent`, or `failed` with a reason the owner sees on the web page).
 *
 * Threading: the calls from [WatchLink] (`onLinkReady`, `onLinkDown`, `onUploadFinished`) arrive on
 * the engine's HandlerThread and only touch the handler-side state; every network call runs on
 * one worker thread, never on the handler (BLE) thread and never on the main thread. A file
 * goes back to the handler with `post`, so "handle, then write" stays on one thread.
 *
 * The server needs the FIT profile to write the file; the phone never does, so this works in
 * the public flavor as it is. The same network rule as the file upload applies (Wi-Fi, Ethernet
 * or VPN, unless "allow mobile data" is on). Workout names are private: they never go into the
 * log, only the workout's number.
 */
class WorkoutSender(
    private val hub: Hub,
    private val http: WorkoutHttp = UrlConnectionHttp(),
) {
    private val handler = hub.handler
    private val log = hub.log
    private val worker = Executors.newSingleThreadExecutor { r -> Thread(r, "nimrag-workouts").apply { isDaemon = true } }

    /** Handler thread: the link that carries the uploads. */
    private var link: WatchLink? = null

    /** Handler thread: the link is up and the handshake is done. */
    private var linkUp = false

    /** Workout id to the revision that is on its way to the watch. */
    private val inFlight = ConcurrentHashMap<Long, Long>()

    /** A look at the queue is running on the worker. */
    private val checking = AtomicBoolean(false)

    /** Reports the server did not take yet (worker thread only); retried at the next look. */
    private val unreported = ArrayDeque<Report>()

    private class Report(val id: Long, val revision: Long, val ok: Boolean, val error: String?)

    private companion object {
        const val NEXT_LOOK_MS = 2_000L
    }

    /** Looks at the queue and sets the next look. */
    private val poll = object : Runnable {
        override fun run() {
            if (!linkUp) return
            look()
            handler.postDelayed(this, WorkoutSendRules.POLL_INTERVAL_MS)
        }
    }

    fun attach(link: WatchLink) {
        this.link = link
    }

    // ---- from the link (handler thread) ----

    /** The handshake is done: look at the queue now, then on a timer while the link stays up. */
    fun onLinkReady() {
        linkUp = true
        handler.removeCallbacks(poll)
        handler.post(poll)
    }

    /** The client closed or the link was torn down. Uploads that were on their way were reported `Aborted` already. */
    fun onLinkDown() {
        linkUp = false
        handler.removeCallbacks(poll)
        inFlight.clear()
    }

    /** `Event.UploadFinished` for an upload this sender started (others are ignored). */
    fun onUploadFinished(uploadId: ULong, result: UploadResult) {
        val id = uploadId.toLong()
        val revision = inFlight.remove(id) ?: return
        when (val report = WorkoutSendRules.reportFor(result)) {
            is WorkoutSendRules.Report.Sent -> {
                log.info("Workout $id sent to the watch")
                send(Report(id, revision, ok = true, error = null))
            }
            is WorkoutSendRules.Report.Failed -> {
                log.warn("Workout $id could not be sent: ${report.reason}")
                send(Report(id, revision, ok = false, error = report.reason))
            }
            is WorkoutSendRules.Report.Keep -> log.info("Workout $id stays queued: the connection closed before it was sent")
        }
        // Others may be waiting: look again soon instead of at the end of the interval.
        if (linkUp) {
            handler.removeCallbacks(poll)
            handler.postDelayed(poll, NEXT_LOOK_MS)
        }
    }

    // ---- the worker ----

    /** Ask the worker to look at the queue (the handler thread never waits for it). */
    private fun look() {
        if (!checking.compareAndSet(false, true)) return
        worker.execute {
            try {
                lookOnWorker()
            } catch (e: IOException) {
                log.warn("Workout queue: ${UploadPolicy.describeIo(e)}")
            } catch (_: Exception) {
                log.warn("Workout queue: ${UploadPolicy.describeIo(IllegalStateException())}")
            } finally {
                checking.set(false)
            }
        }
    }

    private fun lookOnWorker() {
        val base = UploadPolicy.normalizeServerUrl(hub.settings.serverUrl) ?: return
        val token = hub.settings.token ?: return
        if (!hub.uploader.networkAllowsUploads()) return
        val api = WorkoutApi(http, base, token)
        retryReports(api)
        val queued = when (val q = api.queue()) {
            is QueueFetch.Ok -> q.workouts
            is QueueFetch.Failed -> {
                // 404 is an older server without workouts: nothing to do and nothing to say.
                if (q.status != 404) log.warn("Workout queue: the server answered ${if (q.status > 0) "HTTP ${q.status}" else "something unexpected"}")
                return
            }
        }
        for (workout in WorkoutSendRules.pick(queued, inFlight.keys)) {
            when (val fit = api.fit(workout)) {
                is FitFetch.Ok -> {
                    inFlight[workout.id] = fit.revision
                    handler.post { startUpload(workout.id, fit.bytes) }
                }
                is FitFetch.Refused -> {
                    log.warn("Workout ${workout.id}: the server could not make the file (HTTP ${fit.status})")
                    send(Report(workout.id, workout.revision, ok = false, error = fit.message))
                }
                is FitFetch.Gone -> Unit
            }
        }
    }

    /** Handler thread: give the file to the client. A link that is gone by now puts the workout back. */
    private fun startUpload(id: Long, data: ByteArray) {
        val accepted = linkUp && link?.sendFile(
            uploadId = id.toULong(),
            dataType = WorkoutSendRules.DATA_TYPE.toUByte(),
            subType = WorkoutSendRules.SUB_TYPE.toUByte(),
            data = data,
        ) == true
        if (!accepted) inFlight.remove(id)
    }

    private fun send(report: Report) {
        worker.execute {
            val base = UploadPolicy.normalizeServerUrl(hub.settings.serverUrl)
            val token = hub.settings.token
            if (base == null || token == null) {
                unreported.addLast(report)
                return@execute
            }
            if (!deliver(WorkoutApi(http, base, token), report)) unreported.addLast(report)
        }
    }

    private fun retryReports(api: WorkoutApi) {
        var left = unreported.size
        while (left-- > 0) {
            val report = unreported.removeFirstOrNull() ?: return
            if (!deliver(api, report)) {
                unreported.addLast(report)
                return
            }
        }
    }

    /** False when the server could not be reached (the report is kept for the next look). */
    private fun deliver(api: WorkoutApi, report: Report): Boolean = try {
        api.report(report.id, report.revision, report.ok, report.error)
    } catch (_: IOException) {
        false
    } catch (_: Exception) {
        false
    }
}
