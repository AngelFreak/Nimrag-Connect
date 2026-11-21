package dev.nimrag.app.engine.gps

import dev.nimrag.app.engine.Hub
import dev.nimrag.app.engine.ble.WatchLink
import dev.nimrag.app.engine.gps.logic.GpsAnswer
import dev.nimrag.app.engine.gps.logic.GpsAssistCache
import dev.nimrag.app.engine.gps.logic.GpsAssistHttp
import dev.nimrag.app.engine.gps.logic.GpsAssistRules
import dev.nimrag.app.engine.gps.logic.GpsAssistSource
import dev.nimrag.app.engine.gps.logic.Origin
import dev.nimrag.app.engine.gps.logic.ServerConn
import dev.nimrag.app.engine.logic.UploadPolicy
import uniffi.nimrag_ffi.Event
import uniffi.nimrag_ffi.GpsAssistOutcome
import uniffi.nimrag_ffi.Input
import java.io.File
import java.util.concurrent.Executors

/**
 * GPS assist (stage 2f): satellite data to the watch so it finds GPS faster. The watch asks for a
 * file under `/ephemeris/`; the protocol client turns that into [Event.GpsAssistRequest] and
 * waits for [Input.GpsAssistData] or [Input.GpsAssistUnavailable]. The bytes come from the phone's
 * own copy when it is under 4 hours old, else from the Nimrag server (about 10 s), else from a
 * copy under 3 days old, else there is no answer ([GpsAssistSource]). The server downloads the
 * file from Garmin; this phone never talks to Garmin, only to the server it already uploads to,
 * with the same device token and the same network rule as the uploads.
 *
 * Threading: events and answers are on the engine's HandlerThread ([Hub.handler]); every network
 * and file call runs on one worker thread and posts its result back, so the BLE thread never waits.
 *
 * Privacy: the path the watch asked for (it may carry a query) is never logged; the log says what
 * happened and where the bytes came from. The host and path of a refused web request are already
 * cleaned by the core.
 */
class GpsAssist(
    private val hub: Hub,
    http: GpsAssistHttp = UrlConnectionGpsHttp(),
) {
    private val handler = hub.handler
    private val log = hub.log
    private val worker = Executors.newSingleThreadExecutor { r -> Thread(r, "nimrag-gps").apply { isDaemon = true } }
    private val source = GpsAssistSource(GpsAssistCache(File(hub.app.filesDir, "gps-assist")), http, System::currentTimeMillis)

    private var link: WatchLink? = null

    /** Handler thread: bumped when the link goes down, so an answer for an old connection is dropped. */
    private var epoch = 0

    /** Handler thread: request id to the path it was answered for, until the client says how it ended. */
    private val answered = LinkedHashMap<UInt, String>()

    /** Handler thread: refused web requests already logged, with the time, so a watch that asks every minute does not fill the log. */
    private val refusedLogged = HashMap<String, Long>()

    fun attach(link: WatchLink) {
        this.link = link
    }

    // ---- from the link (handler thread) ----

    fun onLinkDown() {
        epoch += 1
        answered.clear()
    }

    fun onWatchEvent(event: Event) {
        when (event) {
            is Event.GpsAssistRequest -> onRequest(event.id, event.pathAndQuery)
            is Event.GpsAssistDone -> onDone(event.id, event.outcome)
            is Event.HttpRequestRefused -> onRefused(event.host, event.path)
            else -> Unit
        }
    }

    /** A sync finished: bring the known files up to date while the phone has the network, so the watch's next ask is quick. */
    fun onSyncFinished() {
        if (link?.connectionDuties?.gpsAssist != true) return
        worker.execute {
            try {
                val conn = connection() ?: return@execute
                val n = source.refreshKnown(conn)
                if (n > 0) log.info("GPS assist: refreshed $n satellite data file(s) from the server")
            } catch (_: Exception) {
                log.warn("GPS assist: could not refresh the satellite data")
            }
        }
    }

    // ---- requests ----

    private fun onRequest(id: UInt, path: String) {
        if (link?.connectionDuties?.gpsAssist != true) {
            link?.sendDuty(Input.GpsAssistUnavailable(id))
            return
        }
        val mine = epoch
        worker.execute {
            val answer = try {
                source.answer(connection(), path)
            } catch (_: Exception) {
                GpsAnswer.Unavailable
            }
            handler.post { deliver(mine, id, path, answer) }
        }
    }

    private fun deliver(forEpoch: Int, id: UInt, path: String, answer: GpsAnswer) {
        if (forEpoch != epoch) return
        when (answer) {
            is GpsAnswer.Data -> {
                answered[id] = path
                while (answered.size > MAX_TRACKED) answered.remove(answered.keys.first())
                val now = System.currentTimeMillis()
                log.info("GPS assist: the watch asked for satellite data, answered from ${describe(answer.origin)} (fetched ${GpsAssistRules.ago(now - answer.ageMs, now)})")
                link?.sendDuty(Input.GpsAssistData(id, answer.body, answer.etag))
            }
            GpsAnswer.Unavailable -> {
                log.warn("GPS assist: the watch asked for satellite data and there is none to give (the server could not be reached and the phone has no recent copy)")
                link?.sendDuty(Input.GpsAssistUnavailable(id))
            }
        }
    }

    private fun onDone(id: UInt, outcome: GpsAssistOutcome) {
        val path = answered.remove(id)
        when (outcome) {
            GpsAssistOutcome.SENT, GpsAssistOutcome.NOT_MODIFIED -> {
                hub.settings.gpsAssistLastSentMs = System.currentTimeMillis()
                hub.update { it.copy(settings = hub.settings.toState()) }
                log.info(if (outcome == GpsAssistOutcome.SENT) "GPS assist: the satellite data reached the watch" else "GPS assist: the watch already had the current satellite data")
            }
            GpsAssistOutcome.REFUSED -> {
                // The client did not like the file; do not serve it again for four hours.
                if (path != null) worker.execute { source.forget(path) }
                log.warn("GPS assist: the satellite data was refused (not a file the watch can use); the phone's copy was dropped")
            }
            GpsAssistOutcome.UNAVAILABLE -> Unit
            GpsAssistOutcome.FAILED -> log.warn("GPS assist: the watch did not take the satellite data")
            GpsAssistOutcome.NEEDS_FILE_ACCESS ->
                log.warn("GPS assist: the watch asked for the file in a way this version does not do yet (file access)")
        }
    }

    private fun onRefused(host: String, path: String) {
        val key = "$host$path"
        val now = System.currentTimeMillis()
        val last = refusedLogged[key]
        if (last != null && now - last < REFUSED_LOG_EVERY_MS) return
        if (refusedLogged.size > 32) refusedLogged.clear()
        refusedLogged[key] = now
        val what = if (host.isEmpty() && path.isEmpty()) "a request it sent encrypted" else "$host$path"
        log.info("Refused a web request from the watch: $what (Nimrag only serves GPS satellite data)")
    }

    // ---- helpers ----

    /** The server and token to use now, or null when the phone may not or cannot ask (not signed in, network rule). Worker thread. */
    private fun connection(): ServerConn? {
        val base = UploadPolicy.normalizeServerUrl(hub.settings.serverUrl) ?: return null
        val token = hub.settings.token ?: return null
        if (!hub.uploader.networkAllowsUploads()) return null
        return ServerConn(base, token)
    }

    private fun describe(origin: Origin) = when (origin) {
        Origin.PhoneCopy -> "the phone's copy"
        Origin.Server -> "the server"
        Origin.OldPhoneCopy -> "the phone's older copy (the server could not be reached)"
    }

    private companion object {
        const val MAX_TRACKED = 8
        const val REFUSED_LOG_EVERY_MS = 10 * 60_000L
    }
}
