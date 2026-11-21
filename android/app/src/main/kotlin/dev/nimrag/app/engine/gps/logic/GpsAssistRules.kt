package dev.nimrag.app.engine.gps.logic

import java.net.URLEncoder

/*
 * GPS assist (stage 2f), the phone's side. Our own code; the 4 hour / 3 day freshness rules and the
 * 10 second wait are the owner's decisions. What the watch asks for and how it is answered
 * (Gadgetbridge's GarminHttpRequest behaviour, inferred) lives in the Rust core; this file only
 * decides where the bytes come from. See docs/PORTED.md.
 */

/** One answer from the Nimrag server: status, the `ETag` and `Age` headers if there were any, the body. */
class GpsReply(val status: Int, val etag: String?, val ageSeconds: Long?, val body: ByteArray)

/** The one call GPS assist makes; the real one is `UrlConnectionGpsHttp`. IO errors are thrown. */
interface GpsAssistHttp {
    /**
     * `GET url` with the Bearer [token] and, when given, `If-None-Match`; no redirects. A body
     * longer than [maxBytes] is an IO error, and so is waiting longer than [timeoutMs].
     */
    fun get(url: String, token: String, ifNoneMatch: String?, timeoutMs: Int, maxBytes: Int): GpsReply
}

/** The server address (already normalised, no trailing slash) and the device token: the only place GPS assist talks to. */
class ServerConn(val base: String, val token: String)

object GpsAssistRules {
    /** A phone copy younger than this is answered without asking the server (the cache-control Garmin sends). */
    const val FRESH_MS = 4L * 3600_000L

    /** When the server cannot be reached, a copy younger than this is still better than none. */
    const val STALE_OK_MS = 3L * 24 * 3600_000L

    /** How long a watch request waits for the server. */
    const val REQUEST_TIMEOUT_MS = 10_000

    /** The refresh after a sync is not in a hurry; the server may have to ask Garmin first. */
    const val REFRESH_TIMEOUT_MS = 25_000

    /** Garmin's file is about 66 KB; the server and the core stop at 1 MiB. */
    const val MAX_BYTES = 1024 * 1024

    /** Paths kept on the phone, oldest dropped first. */
    const val MAX_FILES = 8

    const val PREFIX = "/ephemeris/"
    private const val MAX_PATH = 512
    private val ALLOWED = Regex("^[A-Za-z0-9/._,=&?%+-]+$")

    /**
     * The same rule as the core's `is_safe_path_and_query` and the server's: under `/ephemeris/`,
     * letters, digits and `/ . _ - , = & ? % +`, at most 512 bytes, one `?`, no `..`, no `//` and
     * no encoded dot or slash. The core has checked it already; this is the second lock before a
     * path is put into a request to the server.
     */
    fun isSafePath(path: String): Boolean {
        if (!path.startsWith(PREFIX) || path.length > MAX_PATH) return false
        if (!ALLOWED.matches(path)) return false
        if (path.count { it == '?' } > 1) return false
        if (path.contains("..") || path.contains("//")) return false
        val lower = path.lowercase()
        return !lower.contains("%2e") && !lower.contains("%2f")
    }

    /** `GET <base>/api/device/gps-assist?path=<encoded path>`. */
    fun urlFor(base: String, path: String): String =
        "$base/api/device/gps-assist?path=" + URLEncoder.encode(path, "UTF-8")

    /** True when [fetchedAtMs] is recent enough to answer from the phone's copy. A clock set back counts as old. */
    fun isFresh(fetchedAtMs: Long, nowMs: Long): Boolean = nowMs >= fetchedAtMs && nowMs - fetchedAtMs < FRESH_MS

    /** True when [fetchedAtMs] may still be used because the server is not there. */
    fun isUsableOffline(fetchedAtMs: Long, nowMs: Long): Boolean = nowMs >= fetchedAtMs && nowMs - fetchedAtMs < STALE_OK_MS

    /**
     * When the server's copy was fetched, as the phone should remember it: the server says how old
     * the file already was (`Age`), so a file that was 3 hours old there is not "fresh" for 4 more here.
     */
    fun fetchedAt(nowMs: Long, ageSeconds: Long?): Long {
        val age = (ageSeconds ?: 0L).coerceIn(0L, FRESH_MS / 1000) * 1000
        return nowMs - age
    }

    /** "3 h ago", "20 min ago", "just now", "2 days ago": the Phone features page's wording. */
    fun ago(thenMs: Long, nowMs: Long): String {
        val s = ((nowMs - thenMs) / 1000).coerceAtLeast(0)
        return when {
            s < 60 -> "just now"
            s < 3600 -> "${s / 60} min ago"
            s < 48 * 3600 -> "${s / 3600} h ago"
            else -> "${s / 86400} days ago"
        }
    }
}
