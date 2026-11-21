package dev.nimrag.app.engine.phone

import dev.nimrag.app.engine.Hub
import dev.nimrag.app.engine.ble.WatchLink
import dev.nimrag.app.engine.logic.UploadPolicy
import dev.nimrag.app.engine.phone.logic.DutyMask
import dev.nimrag.app.engine.phone.logic.WeatherAnswer
import dev.nimrag.app.engine.phone.logic.WeatherApi
import dev.nimrag.app.engine.phone.logic.WeatherCell
import dev.nimrag.app.engine.phone.logic.WeatherFetch
import dev.nimrag.app.engine.phone.logic.WeatherHttp
import dev.nimrag.app.engine.phone.logic.WeatherMemory
import dev.nimrag.app.engine.phone.logic.WeatherRules
import dev.nimrag.app.model.WeatherLast
import dev.nimrag.app.model.WeatherStatus
import uniffi.nimrag_ffi.Event
import uniffi.nimrag_ffi.Input
import uniffi.nimrag_ffi.WeatherCell as FfiWeatherCell
import uniffi.nimrag_ffi.WeatherResult
import java.util.concurrent.Executors

/**
 * Weather for the watch (stage 2f). The watch's weather widget sends a request (5014) with its
 * position; the protocol client has already acknowledged it, rounded the position to 0.1 degree
 * (about 10 km) and raised [Event.WeatherRequest]. This asks the owner's Nimrag server
 * (`POST /api/device/weather`), which gets the forecast from Open-Meteo and writes it as the FIT
 * records the watch wants, and hands the bytes to the client ([Input.SendWeather]); the client
 * sends them as 5011 and 5012 and reports [Event.WeatherSent].
 *
 * The phone only ever talks to the Nimrag server. When that fails (no server set, unreachable, an
 * error), the last answer for the same cell is sent again if it is under three hours old;
 * otherwise nothing more is sent (the acknowledgement already went out), which the watch shows
 * as "no weather".
 *
 * Threading: events and results run on the engine's HandlerThread; the network call runs on its
 * own worker thread and posts back. A result that arrives after the connection changed is
 * dropped ([epoch]).
 *
 * Privacy: the position lives in memory for one request and, in [WeatherMemory], for three hours
 * at most; it is never written to settings, the database or the log. Log lines say what happened
 * (fixed phrases), never where.
 *
 * The same network rule as the file upload is NOT applied: a request is a few hundred bytes out
 * and a few kilobytes back, and weather is mostly wanted when the phone is out and about.
 */
class WeatherBridge(
    private val hub: Hub,
    private val http: WeatherHttp = UrlConnectionWeatherHttp(),
    private val clock: () -> Long = System::currentTimeMillis,
) {
    private val handler = hub.handler
    private val log = hub.log
    private val worker = Executors.newSingleThreadExecutor { r -> Thread(r, "nimrag-weather").apply { isDaemon = true } }
    private val memory = WeatherMemory()

    private var link: WatchLink? = null

    /** Handler thread: changes with every connection, so a late result for an older one is dropped. */
    private var epoch = 0

    /** Handler thread: a request is with the worker or with the client. */
    private var busy = false

    /** Handler thread: the answer on its way to the watch is an earlier one. */
    private var sendingEarlier = false

    /** What the worker found: an answer to send (maybe an earlier one) or why there is none. */
    private class Outcome(val answer: WeatherAnswer?, val earlier: Boolean, val failure: String?)

    fun attach(link: WatchLink) {
        this.link = link
    }

    // ---- link life cycle (from WatchLink, on the handler) ----

    fun onLinkReady() {
        epoch += 1
        busy = false
    }

    fun onLinkDown() {
        epoch += 1
        busy = false
    }

    /** The switches changed; with the feature off nothing of the owner's area stays in memory. */
    fun onDutiesChanged(duties: DutyMask) {
        if (!duties.weather) memory.clear()
    }

    // ---- events from the watch (WatchLink.handleEvent, on the handler) ----

    fun onWatchEvent(event: Event) {
        when (event) {
            is Event.WeatherRequest -> onRequest(event.format.toInt(), event.cell, event.hours.toInt())
            is Event.WeatherSent -> onSent(event.result)
            else -> Unit
        }
    }

    private fun onRequest(format: Int, cell: FfiWeatherCell?, hours: Int) {
        val link = link ?: return
        if (!link.connectionDuties.weather) return
        if (busy) {
            log.info("Weather: the watch asked again before the last request was done")
            return
        }
        if (cell == null) {
            log.info("Weather: the watch asked without a usable position, nothing sent")
            setStatus(WeatherLast.NoPosition, sentNow = false)
            return
        }
        busy = true
        val mine = epoch
        val where = WeatherCell(cell.latTenths.toInt(), cell.lonTenths.toInt())
        worker.execute {
            val outcome = try {
                fetch(where, hours, format)
            } catch (_: Exception) {
                Outcome(null, false, WeatherRules.describeFailure(0))
            }
            handler.post { deliver(mine, outcome) }
        }
    }

    /** Worker thread. */
    private fun fetch(cell: WeatherCell, hours: Int, format: Int): Outcome {
        val base = UploadPolicy.normalizeServerUrl(hub.settings.serverUrl)
        val token = hub.settings.token
        val now = clock()
        val why: String = if (base == null || token == null) {
            "no server is set up on this phone"
        } else {
            try {
                when (val got = WeatherApi(http, base, token).forecast(cell, hours, format, now)) {
                    is WeatherFetch.Ok -> {
                        memory.remember(cell, got.answer)
                        return Outcome(got.answer, earlier = false, failure = null)
                    }
                    is WeatherFetch.Failed -> WeatherRules.describeFailure(got.status)
                }
            } catch (_: Exception) {
                WeatherRules.describeFailure(0)
            }
        }
        val earlier = memory.reusable(cell, now)
        return if (earlier != null) {
            Outcome(earlier, earlier = true, failure = why)
        } else {
            Outcome(null, earlier = false, failure = why)
        }
    }

    /** Handler thread: the worker is done. */
    private fun deliver(mine: Int, outcome: Outcome) {
        if (mine != epoch) return
        val link = link
        val answer = outcome.answer
        if (answer == null) {
            busy = false
            log.warn("Weather: no forecast for the watch (${outcome.failure}), nothing sent")
            setStatus(WeatherLast.NoForecast, sentNow = false)
            return
        }
        if (link == null || !link.isReady()) {
            busy = false
            return
        }
        if (outcome.earlier) log.info("Weather: ${outcome.failure}, sending the earlier forecast for this area")
        sendingEarlier = outcome.earlier
        link.sendDuty(Input.SendWeather(answer.definitions, answer.records))
    }

    private fun onSent(result: WeatherResult) {
        busy = false
        if (result == WeatherResult.SENT) {
            log.info(if (sendingEarlier) "Weather: the watch took an earlier forecast" else "Weather: the watch took the forecast")
            setStatus(if (sendingEarlier) WeatherLast.Reused else WeatherLast.Sent, sentNow = true)
        } else {
            log.warn("Weather: the watch did not take the forecast (${describe(result)})")
            setStatus(WeatherLast.NotTaken, sentNow = false)
        }
        sendingEarlier = false
    }

    private fun setStatus(last: WeatherLast, sentNow: Boolean) {
        val at = clock()
        hub.update { state ->
            val old = state.phone.weather
            state.copy(phone = state.phone.copy(weather = WeatherStatus(if (sentNow) at else old.sentAtMs, last)))
        }
    }

    private fun describe(result: WeatherResult) = when (result) {
        WeatherResult.SENT -> "sent"
        WeatherResult.NOT_SUPPORTED -> "the watch does not offer weather"
        WeatherResult.BUSY -> "another answer was still going"
        WeatherResult.INVALID -> "the answer did not fit the watch's message size"
        WeatherResult.REFUSED -> "the watch refused it"
        WeatherResult.TIMED_OUT -> "the watch did not answer in time"
    }
}
