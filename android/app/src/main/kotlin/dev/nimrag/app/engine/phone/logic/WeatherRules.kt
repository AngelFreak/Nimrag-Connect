package dev.nimrag.app.engine.phone.logic

import dev.nimrag.app.engine.auth.logic.MiniJson
import dev.nimrag.app.engine.workouts.logic.HttpReply
import java.util.Base64

/*
 * The pure part of the weather feature (stage 2f). Our own code: the layouts of the watch's
 * request (5014) and of the answer (FIT weather_conditions in 5011/5012) are handled by the
 * protocol client and the server; the phone only asks the server and carries the bytes. No
 * Android types, so the unit tests run on the JVM.
 *
 * Privacy: a position (even rounded to 0.1 degree) is private. It is held in memory for the
 * length of one request and for [WeatherRules.MAX_AGE_MS] in [WeatherMemory] (to answer when the
 * server is down); it is never written to settings, the database or the log, and its toString
 * says nothing.
 */

/** The watch's position rounded to 0.1 degree, in tenths of a degree, as the protocol client gives it. */
class WeatherCell(val latTenths: Int, val lonTenths: Int) {
    override fun equals(other: Any?) = other is WeatherCell && other.latTenths == latTenths && other.lonTenths == lonTenths

    override fun hashCode() = 31 * latTenths + lonTenths

    /** Says nothing on purpose: the cell is where the owner is. */
    override fun toString() = "WeatherCell"
}

/** The server's answer: the FIT definition record and data records for the watch, and when they were fetched. */
class WeatherAnswer(
    val definitions: ByteArray,
    val records: List<ByteArray>,
    /** Unix milliseconds when the forecast was fetched (the older of the server's time and our receipt, so a wrong server clock cannot make it look fresh). */
    val fetchedAtMs: Long,
) {
    /** The forecast is the owner's area: nothing of it is ever printed. */
    override fun toString() = "WeatherAnswer"
}

/** How asking the server ended. */
sealed interface WeatherFetch {
    class Ok(val answer: WeatherAnswer) : WeatherFetch

    /** The server answered with something else; [status] is the HTTP status, 0 for an answer that was not one. */
    data class Failed(val status: Int) : WeatherFetch
}

/** `POST` with the Bearer token and a reply cut at [maxBytes]; the real one is `UrlConnectionWeatherHttp`. */
interface WeatherHttp {
    fun post(url: String, token: String, json: String, maxBytes: Int): HttpReply
}

object WeatherRules {
    /** An earlier answer for the same cell is reused for this long when the server cannot be reached. */
    const val MAX_AGE_MS = 3L * 60 * 60 * 1000

    /** The answer is a few kilobytes of base64; anything near this is not one. */
    const val MAX_REPLY_BYTES = 96 * 1024

    /** Most records an answer can hold (1 current, 12 hourly, 5 daily); more is not our server. */
    const val MAX_RECORDS = 32

    /** A request: connect and read limits. The server itself gives Open-Meteo 10 s. */
    const val CONNECT_TIMEOUT_MS = 8_000
    const val READ_TIMEOUT_MS = 15_000

    /** "55.7", "-0.5", "0.0": a tenth of a degree as text, in integers so that no float error creeps in. */
    fun tenths(t: Int): String {
        val sign = if (t < 0) "-" else ""
        val a = Math.abs(t.toLong())
        return "$sign${a / 10}.${a % 10}"
    }

    /** Body of `POST /api/device/weather`. */
    fun requestBody(cell: WeatherCell, hours: Int, format: Int): String =
        "{\"lat\":${tenths(cell.latTenths)},\"lon\":${tenths(cell.lonTenths)},\"hours\":${hours.coerceIn(0, 255)},\"format\":${format.coerceIn(0, 255)}}"

    /**
     * The server's answer, or null when the text is not one: `{definitions, records, fetched_at}`
     * with base64 bytes. [receivedAtMs] is the phone's clock.
     */
    fun parseAnswer(text: String, receivedAtMs: Long): WeatherAnswer? {
        val root = MiniJson.parseObject(text) ?: return null
        val decoder = Base64.getDecoder()
        fun bytes(value: Any?): ByteArray? {
            val s = value as? String ?: return null
            return try {
                decoder.decode(s).takeIf { it.isNotEmpty() }
            } catch (_: IllegalArgumentException) {
                null
            }
        }
        val definitions = bytes(root["definitions"]) ?: return null
        val list = root["records"] as? List<*> ?: return null
        if (list.isEmpty() || list.size > MAX_RECORDS) return null
        val records = list.map { bytes(it) ?: return null }
        val fetchedMs = (root["fetched_at"] as? Double)?.takeIf { it > 0 && it < 1e12 }?.let { (it * 1000).toLong() }
        return WeatherAnswer(definitions, records, if (fetchedMs == null) receivedAtMs else minOf(fetchedMs, receivedAtMs))
    }

    /** A fixed phrase for the log and the page; it never holds the server's own text or a position. */
    fun describeFailure(status: Int): String = when (status) {
        0 -> "the server could not be reached"
        401, 403 -> "the server does not accept this phone"
        404 -> "this server has no weather feature yet"
        502 -> "the server could not get a forecast"
        503 -> "weather is off on the server or it cannot write it"
        else -> "the server answered HTTP $status"
    }

    /** Whether [answer] is young enough to be sent again at [nowMs]. A clock that went backwards says no. */
    fun usable(answer: WeatherAnswer, nowMs: Long): Boolean {
        val age = nowMs - answer.fetchedAtMs
        return age in 0 until MAX_AGE_MS
    }
}

/** The phone's side of `POST /api/device/weather` (Bearer token, no cookie). IO errors are thrown for the caller. */
class WeatherApi(private val http: WeatherHttp, private val base: String, private val token: String) {
    fun forecast(cell: WeatherCell, hours: Int, format: Int, nowMs: Long): WeatherFetch {
        val reply = http.post("$base/api/device/weather", token, WeatherRules.requestBody(cell, hours, format), WeatherRules.MAX_REPLY_BYTES)
        if (reply.status != 200) return WeatherFetch.Failed(reply.status)
        val answer = WeatherRules.parseAnswer(String(reply.body, Charsets.UTF_8), nowMs) ?: return WeatherFetch.Failed(0)
        return WeatherFetch.Ok(answer)
    }
}

/**
 * The last answer for one cell, in memory only. The owner's position is not stored anywhere else;
 * a restart of the app forgets it, and switching the feature off clears it.
 */
class WeatherMemory {
    private var cell: WeatherCell? = null
    private var answer: WeatherAnswer? = null

    @Synchronized
    fun remember(cell: WeatherCell, answer: WeatherAnswer) {
        this.cell = cell
        this.answer = answer
    }

    /** The remembered answer for exactly this [cell], if it is under [WeatherRules.MAX_AGE_MS] old. */
    @Synchronized
    fun reusable(cell: WeatherCell, nowMs: Long): WeatherAnswer? {
        val a = answer ?: return null
        return if (cell == this.cell && WeatherRules.usable(a, nowMs)) a else null
    }

    @Synchronized
    fun clear() {
        cell = null
        answer = null
    }
}
