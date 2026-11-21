package dev.nimrag.app.engine.gps.logic

import java.io.File
import java.io.FileOutputStream
import java.io.IOException
import java.security.MessageDigest

/** One file the phone keeps: the bytes, Garmin's `ETag` as the server gave it, and when it was fetched. */
class GpsFile(val path: String, val body: ByteArray, val etag: String, val fetchedAtMs: Long)

/**
 * The phone's copies of the satellite files, one pair of files per path in [dir] (private app
 * storage): `<sha256 of the path>.bin` and `.meta` (fetch time, tag, path). Only JDK types, so it
 * is unit-tested on the JVM. Nothing here is logged, and nothing leaves the phone except to the
 * watch.
 */
class GpsAssistCache(private val dir: File) {
    private fun name(path: String) = MessageDigest.getInstance("SHA-256")
        .digest(path.toByteArray(Charsets.UTF_8))
        .joinToString("") { "%02x".format(it.toInt() and 0xFF) }

    fun read(path: String): GpsFile? = readNamed(name(path))?.takeIf { it.path == path }

    /** Every copy there is (the paths the watch has asked for). */
    fun all(): List<GpsFile> =
        dir.listFiles { f -> f.name.endsWith(".meta") }.orEmpty().mapNotNull { readNamed(it.name.removeSuffix(".meta")) }

    fun write(file: GpsFile) {
        if (!dir.isDirectory && !dir.mkdirs() && !dir.isDirectory) throw IOException("cannot create the GPS assist folder")
        val n = name(file.path)
        // Body first: a reader never meets a meta file without its body.
        atomic(File(dir, "$n.bin"), file.body)
        atomic(File(dir, "$n.meta"), "${file.fetchedAtMs}\n${file.etag}\n${file.path}\n".toByteArray(Charsets.UTF_8))
        prune()
    }

    fun forget(path: String) {
        val n = name(path)
        File(dir, "$n.meta").delete()
        File(dir, "$n.bin").delete()
    }

    private fun readNamed(n: String): GpsFile? = try {
        val meta = File(dir, "$n.meta").takeIf { it.isFile }?.readText(Charsets.UTF_8)?.split('\n')
        val body = File(dir, "$n.bin").takeIf { it.isFile && it.length() in 1..GpsAssistRules.MAX_BYTES }?.readBytes()
        val at = meta?.getOrNull(0)?.toLongOrNull()
        val path = meta?.getOrNull(2)
        if (meta == null || body == null || at == null || path.isNullOrEmpty()) null else GpsFile(path, body, meta[1], at)
    } catch (_: IOException) {
        null
    }

    private fun atomic(target: File, bytes: ByteArray) {
        val tmp = File(dir, ".${target.name}.tmp")
        try {
            FileOutputStream(tmp).use {
                it.write(bytes)
                it.flush()
            }
            if (!tmp.renameTo(target)) throw IOException("cannot save the GPS assist file")
        } finally {
            if (tmp.exists()) tmp.delete()
        }
    }

    private fun prune() {
        val all = all().sortedByDescending { it.fetchedAtMs }
        for (old in all.drop(GpsAssistRules.MAX_FILES)) forget(old.path)
    }
}

/** Where an answer came from (for the log; never the path). */
enum class Origin { PhoneCopy, Server, OldPhoneCopy }

sealed interface GpsAnswer {
    class Data(val body: ByteArray, val etag: String, val origin: Origin, val ageMs: Long) : GpsAnswer

    data object Unavailable : GpsAnswer
}

/**
 * Where the bytes for the watch come from: the phone's copy when it is under 4 hours old, else
 * the server (about 10 s), else a copy under 3 days old, else nothing. The watch's own
 * `If-None-Match` is not needed here: the core compares the tag we hand back with the watch's.
 *
 * Blocking (network and files): call off the handler thread. The token goes only to
 * [ServerConn.base], never in a URL and never to a redirect (the client does not follow any).
 */
class GpsAssistSource(
    private val cache: GpsAssistCache,
    private val http: GpsAssistHttp,
    private val nowMs: () -> Long,
) {
    /** [conn] is null when the phone may not or cannot ask the server (not signed in, wrong network). */
    fun answer(conn: ServerConn?, path: String): GpsAnswer {
        if (!GpsAssistRules.isSafePath(path)) return GpsAnswer.Unavailable
        val now = nowMs()
        val have = cache.read(path)
        if (have != null && GpsAssistRules.isFresh(have.fetchedAtMs, now)) return data(have, Origin.PhoneCopy, now)
        if (conn != null) {
            val fetched = fetch(conn, path, have, GpsAssistRules.REQUEST_TIMEOUT_MS)
            if (fetched != null) return data(fetched, Origin.Server, now)
        }
        if (have != null && GpsAssistRules.isUsableOffline(have.fetchedAtMs, now)) return data(have, Origin.OldPhoneCopy, now)
        return GpsAnswer.Unavailable
    }

    /**
     * After a sync: every known path whose copy is 4 hours old or older is asked for again.
     * Returns how many were refreshed (a "not modified" counts).
     */
    fun refreshKnown(conn: ServerConn): Int {
        var n = 0
        for (file in cache.all()) {
            val now = nowMs()
            if (GpsAssistRules.isFresh(file.fetchedAtMs, now)) continue
            if (fetch(conn, file.path, file, GpsAssistRules.REFRESH_TIMEOUT_MS) != null) n += 1
        }
        return n
    }

    /** A copy the core refused is dropped, so it is not served again for 4 hours. */
    fun forget(path: String) = cache.forget(path)

    private fun data(f: GpsFile, origin: Origin, now: Long) = GpsAnswer.Data(f.body, f.etag, origin, now - f.fetchedAtMs)

    /** The server's file (stored), the stored copy touched when the server says "not modified", else null. */
    private fun fetch(conn: ServerConn, path: String, have: GpsFile?, timeoutMs: Int): GpsFile? {
        val reply = try {
            http.get(
                GpsAssistRules.urlFor(conn.base, path),
                conn.token,
                have?.etag?.takeIf { it.isNotEmpty() },
                timeoutMs,
                GpsAssistRules.MAX_BYTES,
            )
        } catch (_: IOException) {
            return null
        } catch (_: RuntimeException) {
            return null
        }
        val now = nowMs()
        val at = GpsAssistRules.fetchedAt(now, reply.ageSeconds)
        return when {
            reply.status == 304 && have != null -> GpsFile(path, have.body, have.etag, at).also { save(it) }
            reply.status == 200 && reply.body.isNotEmpty() && reply.body.size <= GpsAssistRules.MAX_BYTES ->
                GpsFile(path, reply.body, reply.etag.orEmpty(), at).also { save(it) }
            else -> null
        }
    }

    private fun save(file: GpsFile) {
        try {
            cache.write(file)
        } catch (_: IOException) {
            // Not being able to keep a copy does not stop this answer.
        }
    }
}
