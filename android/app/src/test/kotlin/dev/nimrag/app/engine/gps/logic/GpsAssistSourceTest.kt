package dev.nimrag.app.engine.gps.logic

import org.junit.jupiter.api.Assertions.assertArrayEquals
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertNotNull
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test
import org.junit.jupiter.api.io.TempDir
import java.io.File
import java.io.IOException

private class Call(val url: String, val token: String, val ifNoneMatch: String?, val timeoutMs: Int)

private class FakeServer : GpsAssistHttp {
    val calls = mutableListOf<Call>()
    var reply: () -> GpsReply = { GpsReply(200, "\"t1\"", 0, "tar-bytes".toByteArray()) }

    override fun get(url: String, token: String, ifNoneMatch: String?, timeoutMs: Int, maxBytes: Int): GpsReply {
        calls += Call(url, token, ifNoneMatch, timeoutMs)
        return reply()
    }
}

class GpsAssistSourceTest {
    @TempDir
    lateinit var dir: File

    private val h = 3600_000L
    private var now = 1_000_000L * h
    private val http = FakeServer()
    private val conn = ServerConn("https://nimrag.example.ts.net", "secret-token")
    private val path = "/ephemeris/cpe/sony/lle?coverage=WEEKS_1&constellations=GPS,GLONASS,GALILEO,QZSS"

    private fun cache() = GpsAssistCache(File(dir, "gps-assist"))

    private fun source() = GpsAssistSource(cache(), http, { now })

    private fun data(a: GpsAnswer) = a as GpsAnswer.Data

    @Test
    fun `the first ask goes to the server, with the token and within ten seconds`() {
        val a = data(source().answer(conn, path))
        assertEquals(Origin.Server, a.origin)
        assertArrayEquals("tar-bytes".toByteArray(), a.body)
        assertEquals("\"t1\"", a.etag)
        val c = http.calls.single()
        assertEquals(GpsAssistRules.urlFor(conn.base, path), c.url)
        assertEquals("secret-token", c.token)
        assertNull(c.ifNoneMatch)
        assertEquals(10_000, c.timeoutMs)
        assertFalse(c.url.contains("secret-token"))
    }

    @Test
    fun `a copy under four hours old is answered without the server`() {
        source().answer(conn, path)
        now += 3 * h
        val a = data(source().answer(conn, path))
        assertEquals(Origin.PhoneCopy, a.origin)
        assertEquals(3 * h, a.ageMs)
        assertEquals(1, http.calls.size)
    }

    @Test
    fun `an older copy is asked again with its tag, and not modified keeps it and makes it fresh`() {
        source().answer(conn, path)
        now += 5 * h
        http.reply = { GpsReply(304, "\"t1\"", 0, ByteArray(0)) }
        val a = data(source().answer(conn, path))
        assertEquals(Origin.Server, a.origin)
        assertArrayEquals("tar-bytes".toByteArray(), a.body)
        assertEquals("\"t1\"", http.calls.last().ifNoneMatch)
        now += 1 * h
        assertEquals(Origin.PhoneCopy, data(source().answer(conn, path)).origin)
        assertEquals(2, http.calls.size)
    }

    @Test
    fun `a new file from the server replaces the copy`() {
        source().answer(conn, path)
        now += 5 * h
        http.reply = { GpsReply(200, "\"t2\"", 0, "new".toByteArray()) }
        val a = data(source().answer(conn, path))
        assertEquals("\"t2\"", a.etag)
        assertArrayEquals("new".toByteArray(), cache().read(path)!!.body)
    }

    @Test
    fun `the server's Age counts against the four hours`() {
        http.reply = { GpsReply(200, "\"t1\"", 3 * 3600L, "tar-bytes".toByteArray()) }
        source().answer(conn, path)
        now += 2 * h
        // Three hours old at the server plus two here: asked again.
        source().answer(conn, path)
        assertEquals(2, http.calls.size)
    }

    @Test
    fun `without the server a copy under three days old is used, an older one is not`() {
        source().answer(conn, path)
        http.reply = { throw IOException("down") }
        now += 2 * 24 * h
        val a = data(source().answer(conn, path))
        assertEquals(Origin.OldPhoneCopy, a.origin)
        assertArrayEquals("tar-bytes".toByteArray(), a.body)
        now += 2 * 24 * h
        assertEquals(GpsAnswer.Unavailable, source().answer(conn, path))
    }

    @Test
    fun `a server error or an empty answer counts as down`() {
        for (r in listOf(GpsReply(502, null, null, ByteArray(0)), GpsReply(404, null, null, ByteArray(0)), GpsReply(200, "\"x\"", 0, ByteArray(0)))) {
            http.reply = { r }
            assertEquals(GpsAnswer.Unavailable, source().answer(conn, "/ephemeris/p${r.status}"))
        }
    }

    @Test
    fun `no connection to the server uses the local copy or says there is none, and makes no request`() {
        assertEquals(GpsAnswer.Unavailable, source().answer(null, path))
        source().answer(conn, path)
        val before = http.calls.size
        now += 6 * h
        assertEquals(Origin.OldPhoneCopy, data(source().answer(null, path)).origin)
        assertEquals(before, http.calls.size)
    }

    @Test
    fun `a path outside the whitelist is never sent anywhere`() {
        assertEquals(GpsAnswer.Unavailable, source().answer(conn, "/api/health"))
        assertEquals(GpsAnswer.Unavailable, source().answer(conn, "/ephemeris/../x"))
        assertTrue(http.calls.isEmpty())
    }

    @Test
    fun `refreshing after a sync asks only for copies that are four hours old or older`() {
        val other = "/ephemeris/rxnetworks/v1/a.gz"
        source().answer(conn, path)
        now += 3 * h
        source().answer(conn, other)
        now += 2 * h // path: 5 h old, other: 2 h old
        http.calls.clear()
        http.reply = { GpsReply(200, "\"t9\"", 0, "fresh".toByteArray()) }
        assertEquals(1, source().refreshKnown(conn))
        assertEquals(GpsAssistRules.urlFor(conn.base, path), http.calls.single().url)
        assertEquals(25_000, http.calls.single().timeoutMs)
        assertArrayEquals("fresh".toByteArray(), cache().read(path)!!.body)
        // Nothing left to refresh now.
        http.calls.clear()
        assertEquals(0, source().refreshKnown(conn))
        assertTrue(http.calls.isEmpty())
    }

    @Test
    fun `a refresh that fails changes nothing`() {
        source().answer(conn, path)
        now += 5 * h
        http.reply = { throw IOException("down") }
        assertEquals(0, source().refreshKnown(conn))
        assertEquals(5 * h, now - cache().read(path)!!.fetchedAtMs)
    }

    @Test
    fun `a copy the client refused is forgotten`() {
        source().answer(conn, path)
        source().forget(path)
        assertNull(cache().read(path))
        assertTrue(cache().all().isEmpty())
    }

    @Test
    fun `only the newest few paths are kept, and the cache names no path`() {
        for (i in 0 until 12) {
            now += 1_000
            source().answer(conn, "/ephemeris/p$i")
        }
        val all = cache().all()
        assertEquals(GpsAssistRules.MAX_FILES, all.size)
        assertNotNull(cache().read("/ephemeris/p11"))
        assertNull(cache().read("/ephemeris/p0"))
        for (f in File(dir, "gps-assist").listFiles()!!) assertFalse(f.name.contains("ephemeris"), f.name)
    }

    @Test
    fun `a damaged cache file is a miss, not a crash`() {
        source().answer(conn, path)
        File(dir, "gps-assist").listFiles { f -> f.name.endsWith(".meta") }!!.forEach { it.writeText("garbage") }
        assertNull(cache().read(path))
        http.calls.clear()
        assertEquals(Origin.Server, data(source().answer(conn, path)).origin)
    }
}
