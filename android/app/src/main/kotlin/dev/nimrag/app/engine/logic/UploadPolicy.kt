package dev.nimrag.app.engine.logic

import java.io.IOException
import java.net.ConnectException
import java.net.NoRouteToHostException
import java.net.SocketTimeoutException
import java.net.URI
import java.net.UnknownHostException
import javax.net.ssl.SSLException

/**
 * Upload decisions as pure functions (IMPLEMENTATION_PLAN.md stage 2d): what a server
 * answer means, how long to wait after a failure, and when the active network may be used.
 */
object UploadPolicy {
    const val BACKOFF_FIRST_MS = 30_000L
    const val BACKOFF_MAX_MS = 3_600_000L

    /** A file that is not retried by itself (413 or another 4xx): the next attempt time. */
    const val PARKED = Long.MAX_VALUE

    sealed interface Outcome {
        /** 200 or 201: the server has the bytes. */
        data object Uploaded : Outcome

        /** 401/403: the token is wrong or revoked. Stop all uploads until the settings change. */
        data class StopAll(val reason: String) : Outcome

        /** 413 or another 4xx: this file is marked failed; the others go on. */
        data class FailFile(val reason: String) : Outcome

        /** IO error, 5xx, 408, 429 and anything unexpected: try again later with backoff. */
        data class Retry(val reason: String) : Outcome
    }

    /** Server answer to `POST /api/files`. */
    fun outcome(httpStatus: Int): Outcome = when (httpStatus) {
        200, 201 -> Outcome.Uploaded
        401, 403 -> Outcome.StopAll("The server refused the token (HTTP $httpStatus)")
        408, 429 -> Outcome.Retry("HTTP $httpStatus, will retry")
        413 -> Outcome.FailFile("The server says the file is too large (HTTP 413)")
        in 400..499 -> Outcome.FailFile("The server rejected the file (HTTP $httpStatus)")
        in 500..599 -> Outcome.Retry("Server error (HTTP $httpStatus), will retry")
        else -> Outcome.Retry("Unexpected answer (HTTP $httpStatus), check the server URL")
    }

    /** No answer at all (connection refused, timeout, DNS): retry with backoff. */
    fun ioFailure(description: String): Outcome = Outcome.Retry(description)

    /**
     * Wait after the [attempts]-th failed attempt of one file (1 = first failure):
     * 30 s, 60 s, 2 min, ... capped at 1 hour.
     */
    fun backoffMs(attempts: Int): Long {
        val n = (attempts - 1).coerceIn(0, 20)
        return minOf(BACKOFF_FIRST_MS shl n, BACKOFF_MAX_MS)
    }

    fun nextAttemptAt(nowMs: Long, attempts: Int): Long = nowMs + backoffMs(attempts)

    /**
     * What the active network looks like to the rule. [validated] (Android has checked that
     * the Internet really answers) does not change the rule; it only takes part in telling
     * a real change from noise, see [NetworkChangeFilter].
     */
    data class NetworkInfo(
        val hasInternet: Boolean,
        val unmetered: Boolean,
        val vpn: Boolean,
        val validated: Boolean = true,
    )

    const val MAX_TOKEN_LENGTH = 512

    /**
     * Bearer tokens are printable ASCII without spaces (0x21 to 0x7E), at most 512 characters.
     * Anything else (a pasted newline in the middle, a smart quote, control characters) would
     * end up in an HTTP header, so it is refused.
     */
    fun isValidToken(token: String): Boolean =
        token.isNotEmpty() && token.length <= MAX_TOKEN_LENGTH && token.all { it.code in 0x21..0x7E }

    /**
     * A fixed, secret-free description of a network failure: only the kind, never the
     * exception's own text (which may name hosts, paths or worse).
     */
    fun describeIo(e: Throwable): String = when (e) {
        is UnknownHostException -> "Network error: the server name could not be resolved"
        is SocketTimeoutException -> "Network error: the server did not answer in time"
        is ConnectException, is NoRouteToHostException -> "Network error: the server cannot be reached"
        is SSLException -> "Network error: the secure connection failed (check https and the certificate)"
        is IOException -> "Network error: the connection failed"
        else -> "Unexpected error while talking to the server"
    }

    /**
     * Uploads need an active network with Internet that is unmetered (Wi-Fi, Ethernet) or a
     * VPN, unless [allowMobileData] is set. No active network means no upload.
     */
    fun networkAllows(network: NetworkInfo?, allowMobileData: Boolean): Boolean {
        if (network == null || !network.hasInternet) return false
        return allowMobileData || network.unmetered || network.vpn
    }

    /**
     * Trimmed, without trailing slashes, only http or https with a host; null otherwise.
     * Plain http is allowed on purpose: the server may sit on the LAN or the tailnet.
     */
    fun normalizeServerUrl(raw: String): String? {
        val trimmed = raw.trim().trimEnd('/')
        if (trimmed.isEmpty()) return null
        val uri = try {
            URI(trimmed)
        } catch (_: Exception) {
            return null
        }
        val scheme = uri.scheme?.lowercase() ?: return null
        if (scheme != "http" && scheme != "https") return null
        if (uri.host.isNullOrEmpty()) return null
        if (uri.rawQuery != null || uri.rawFragment != null) return null
        return trimmed
    }

    /** What the server stores about one file; null parameters are left out. */
    data class UploadMeta(
        val dataType: Int,
        val subType: Int,
        val fileIndex: Int,
        val fileNumber: Int,
        /** Raw Garmin-epoch seconds as the watch gave them; null or 0 = none. */
        val watchTimestamp: Long?,
        val sha256: String,
    )

    /** `{server}/api/files?...&source=watch&sha256=...` (server/README.md, API). */
    fun uploadUrl(baseUrl: String, meta: UploadMeta): String {
        val params = buildList {
            add("data_type=${meta.dataType}")
            add("sub_type=${meta.subType}")
            add("file_index=${meta.fileIndex}")
            add("file_number=${meta.fileNumber}")
            if (meta.watchTimestamp != null && meta.watchTimestamp != 0L) {
                add("watch_timestamp=${meta.watchTimestamp}")
            }
            add("source=watch")
            add("sha256=${meta.sha256}")
        }
        return "$baseUrl/api/files?${params.joinToString("&")}"
    }
}

/**
 * Decides whether a connectivity callback is worth waking the upload worker: only when the
 * default network appeared, vanished or changed in one of the things the rule cares about
 * (Internet, validated, unmetered, VPN). Android calls `onCapabilitiesChanged` for every
 * signal-strength change; those must not each cause a pass over the queue.
 */
class NetworkChangeFilter {
    private var last: UploadPolicy.NetworkInfo? = null
    private var seen = false

    /** [info] is null when the default network is gone. Thread-safe. */
    @Synchronized
    fun shouldWake(info: UploadPolicy.NetworkInfo?): Boolean {
        val changed = !seen || info != last
        seen = true
        last = info
        return changed
    }
}
