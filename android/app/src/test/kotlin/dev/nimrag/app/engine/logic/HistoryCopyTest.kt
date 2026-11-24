package dev.nimrag.app.engine.logic

import dev.nimrag.app.model.HistoryRetention
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertInstanceOf
import org.junit.jupiter.api.Assertions.assertNotNull
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

private const val HOUR = 3_600_000L
private const val NOW = 1_800_000_000_000L

private fun sha(n: Int) = n.toString(16).padStart(64, '0')

private fun file(n: Int, size: Long = 1_000, lastTs: Long? = 1_700_000_000L + n) = HistoryFile(
    sha256 = sha(n),
    size = size,
    source = "connect",
    dataType = null,
    subType = null,
    fileIndex = null,
    fileNumber = null,
    watchTimestamp = null,
    lastTs = lastTs,
)

/** The phone's copy: the files it has, and what was put in or removed. */
private class FakeCopy(have: Set<String> = emptySet()) : HistoryCopy {
    val have = have.toMutableSet()
    val ingested = mutableListOf<String>()
    var ingestFails = false
    var prunedWith: List<Long?> = emptyList()

    override fun missing(sha256s: List<String>) = sha256s.filter { it !in have }

    override fun ingest(bytes: ByteArray, file: HistoryFile): Boolean {
        if (ingestFails) error("the phone could not take it")
        ingested += file.sha256
        return have.add(file.sha256)
    }

    override fun prune(keepSince: Long?): Int {
        prunedWith = prunedWith + keepSince
        return 0
    }
}

/** The real server: a list in pages of [pageSize], and the files it can give. */
private class FakeServer(val files: List<HistoryFile>, val pageSize: Int = 2) : HistoryServer {
    val listCalls = mutableListOf<Triple<String?, Int, Long?>>()
    val downloads = mutableListOf<String>()
    val gone = mutableSetOf<String>()
    var listReply: HistoryPage? = null
    var downloadReply: HistoryDownload? = null
    var failAfter = Int.MAX_VALUE

    override fun list(cursor: String?, limit: Int, since: Long?): HistoryPage {
        listCalls += Triple(cursor, limit, since)
        listReply?.let { return it }
        val from = cursor?.toInt() ?: 0
        val page = files.drop(from).take(pageSize)
        val next = (from + pageSize).takeIf { it < files.size }?.toString()
        return HistoryPage.Files(page, next)
    }

    override fun download(sha256: String): HistoryDownload {
        downloads += sha256
        if (downloads.size > failAfter) return HistoryDownload.Failed("Connection reset")
        downloadReply?.let { return it }
        if (sha256 in gone) return HistoryDownload.Gone
        val size = files.first { it.sha256 == sha256 }.size.toInt()
        return HistoryDownload.Bytes(ByteArray(minOf(size, 16)))
    }
}

class HistoryCopyTest {
    @Test
    fun `the choices map to windows by data date and unknown stored values mean off`() {
        assertEquals(null, HistoryRetention.Off.sinceSeconds(NOW))
        assertEquals(null, HistoryRetention.Everything.sinceSeconds(NOW))
        assertEquals(NOW / 1000 - 91 * 86_400L, HistoryRetention.Last3Months.sinceSeconds(NOW))
        assertEquals(NOW / 1000 - 365 * 86_400L, HistoryRetention.Last12Months.sinceSeconds(NOW))
        assertFalse(HistoryRetention.Off.copies)
        assertTrue(HistoryRetention.Last3Months.copies)
        assertEquals(HistoryRetention.Off, HistoryRetention.fromKey(null))
        assertEquals(HistoryRetention.Off, HistoryRetention.fromKey("weekly"))
        for (c in HistoryRetention.entries) assertEquals(c, HistoryRetention.fromKey(c.key))
    }

    @Test
    fun `a wider choice looks again, a narrower one removes, the same does nothing`() {
        val off = HistoryRetention.Off
        val m3 = HistoryRetention.Last3Months
        val m12 = HistoryRetention.Last12Months
        val all = HistoryRetention.Everything
        assertEquals(HistoryChange.Widened, HistoryChoice.change(off, m3))
        assertEquals(HistoryChange.Widened, HistoryChoice.change(m3, m12))
        assertEquals(HistoryChange.Widened, HistoryChoice.change(m12, all))
        assertEquals(HistoryChange.Narrowed, HistoryChoice.change(all, m12))
        assertEquals(HistoryChange.Narrowed, HistoryChoice.change(m3, off))
        assertEquals(HistoryChange.None, HistoryChoice.change(m12, m12))
        // Off removes every copy; a window removes what is older than it.
        assertNull(HistoryChoice.pruneSince(off, NOW))
        assertEquals(HistoryRetention.Last3Months.sinceSeconds(NOW), HistoryChoice.pruneSince(m3, NOW))
    }

    @Test
    fun `the list answer is read, and anything else is not a list`() {
        val ok = HistoryPolicy.parseList(
            """{"files":[{"sha256":"${sha(1)}","size":12,"source":"connect","data_type":null,"sub_type":null,
               "file_index":null,"file_number":null,"watch_timestamp":null,"received_at":"2026-11-20T10:00:00.000Z",
               "last_ts":1773000000},
               {"sha256":"${sha(2)}","size":5,"source":"watch","data_type":128,"sub_type":4,"file_index":1,
               "file_number":2,"watch_timestamp":1100000000,"received_at":"x","last_ts":null}],"next_cursor":"abc"}""",
        )
        assertNotNull(ok)
        assertEquals("abc", ok!!.nextCursor)
        assertEquals(2, ok.files.size)
        assertEquals(HistoryFile(sha(1), 12, "connect", null, null, null, null, null, 1_773_000_000L), ok.files[0])
        assertEquals(HistoryFile(sha(2), 5, "watch", 128, 4, 1, 2, 1_100_000_000L, null), ok.files[1])
        assertNull(HistoryPolicy.parseList("""{"files":[],"next_cursor":null}""")!!.nextCursor)
        assertNull(HistoryPolicy.parseList("""{"files":[{"sha256":"nope","size":1,"source":"x"}]}"""))
        assertNull(HistoryPolicy.parseList("""{"files":[{"sha256":"${sha(1)}","source":"x"}]}"""))
        assertNull(HistoryPolicy.parseList("""<html>hotspot login</html>"""))
        assertNull(HistoryPolicy.parseList("""{"nothing":1}"""))
        // A long page is longer than the small-answer limit of the JSON reader.
        val big = (0 until 800).joinToString(",") {
            """{"sha256":"${sha(it)}","size":1,"source":"connect","received_at":"2026-11-20T10:00:00.000Z","last_ts":1}"""
        }
        assertEquals(800, HistoryPolicy.parseList("""{"files":[$big],"next_cursor":null}""")!!.files.size)
    }

    @Test
    fun `urls carry the window and the cursor, and statuses decide what happens`() {
        assertEquals("https://s/api/device/files?limit=200", HistoryPolicy.listUrl("https://s", null, 200, null))
        assertEquals("https://s/api/device/files?limit=50&since=99&cursor=ab12", HistoryPolicy.listUrl("https://s", "ab12", 50, 99))
        assertEquals("https://s/api/device/files/${sha(1)}/content", HistoryPolicy.contentUrl("https://s", sha(1)))
        assertInstanceOf(HistoryPage.Rejected::class.java, HistoryPolicy.page(ServerReply(401)))
        assertInstanceOf(HistoryPage.Rejected::class.java, HistoryPolicy.page(ServerReply(403)))
        assertInstanceOf(HistoryPage.Failed::class.java, HistoryPolicy.page(ServerReply(404)))
        assertInstanceOf(HistoryPage.Failed::class.java, HistoryPolicy.page(ServerReply(503)))
        assertInstanceOf(HistoryPage.Failed::class.java, HistoryPolicy.page(ServerReply.noAnswer("offline")))
        assertInstanceOf(HistoryPage.Failed::class.java, HistoryPolicy.page(ServerReply(200, "not json")))
        assertInstanceOf(HistoryPage.Files::class.java, HistoryPolicy.page(ServerReply(200, """{"files":[],"next_cursor":null}""")))
        assertEquals(HistoryDownload.Gone, HistoryPolicy.downloadFailure(404))
        assertEquals(HistoryDownload.Rejected(403), HistoryPolicy.downloadFailure(403))
        assertInstanceOf(HistoryDownload.Failed::class.java, HistoryPolicy.downloadFailure(500))
        assertInstanceOf(HistoryDownload.Failed::class.java, HistoryPolicy.downloadFailure(0))
    }

    @Test
    fun `a pass copies at most 25 files, newest first, and says how many are left`() {
        val files = (1..60).map { file(it) } // the server lists newest first: 1 is the newest
        val server = FakeServer(files, pageSize = 25)
        val copy = FakeCopy()
        val copier = HistoryCopier(copy, server)

        val first = copier.pass(HistoryRetention.Everything, NOW)
        assertEquals(HistoryCopier.Result.More(copied = 25, skipped = 0, left = 35), first)
        assertEquals(files.take(25).map { it.sha256 }, copy.ingested)

        val second = copier.pass(HistoryRetention.Everything, NOW)
        assertEquals(HistoryCopier.Result.More(25, 0, 10), second)
        assertEquals(files.take(50).map { it.sha256 }, copy.ingested)

        val third = copier.pass(HistoryRetention.Everything, NOW)
        assertEquals(HistoryCopier.Result.Done(copied = 10, skipped = 0, total = 60), third)
        assertEquals(files.map { it.sha256 }, copy.ingested)
        // The list was read once for the whole walk, not once per pass.
        assertEquals(3, server.listCalls.size)
    }

    @Test
    fun `a pass stops before the byte limit, but always takes its first file`() {
        val mb = 1024L * 1024
        val server = FakeServer(listOf(file(1, 4 * mb), file(2, 4 * mb), file(3, 4 * mb), file(4, 4 * mb)))
        val copy = FakeCopy()
        val copier = HistoryCopier(copy, server)
        // 4 + 4 MB fit in 10 MB, a third would make 12.
        assertEquals(HistoryCopier.Result.More(2, 0, 2), copier.pass(HistoryRetention.Everything, NOW))

        val huge = FakeServer(listOf(file(1, 50 * mb), file(2, 1_000)))
        val c2 = HistoryCopier(FakeCopy(), huge)
        assertEquals(HistoryCopier.Result.More(1, 0, 1), c2.pass(HistoryRetention.Everything, NOW))
        assertEquals(HistoryCopier.Result.Done(1, 0, 2), c2.pass(HistoryRetention.Everything, NOW))
    }

    @Test
    fun `the whole list is walked in pages, only the missing files are kept and the window goes with each request`() {
        val files = (1..7).map { file(it) }
        val server = FakeServer(files, pageSize = 3)
        val copy = FakeCopy(have = setOf(sha(2), sha(5), sha(6)))
        val copier = HistoryCopier(copy, server)
        val result = copier.pass(HistoryRetention.Last3Months, NOW)

        assertEquals(HistoryCopier.Result.Done(4, 0, 4), result)
        assertEquals(listOf(sha(1), sha(3), sha(4), sha(7)), copy.ingested)
        val since = HistoryRetention.Last3Months.sinceSeconds(NOW)
        assertEquals(
            listOf(Triple(null, HistoryPolicy.PAGE, since), Triple("3", HistoryPolicy.PAGE, since), Triple("6", HistoryPolicy.PAGE, since)),
            server.listCalls,
        )
        // Everything has no window.
        val all = FakeServer(files)
        HistoryCopier(FakeCopy(), all).pass(HistoryRetention.Everything, NOW)
        assertTrue(all.listCalls.all { it.third == null })
    }

    @Test
    fun `off copies nothing and asks nothing`() {
        val server = FakeServer(listOf(file(1)))
        val copy = FakeCopy()
        assertEquals(HistoryCopier.Result.Done(0, 0, 0), HistoryCopier(copy, server).pass(HistoryRetention.Off, NOW))
        assertTrue(server.listCalls.isEmpty())
        assertTrue(server.downloads.isEmpty())
        assertTrue(copy.ingested.isEmpty())
    }

    @Test
    fun `a file that went away is skipped and a file that arrived meanwhile is not downloaded`() {
        val files = (1..40).map { file(it) }
        val server = FakeServer(files, pageSize = 40)
        val copy = FakeCopy()
        val copier = HistoryCopier(copy, server)
        server.gone += sha(3)
        val first = copier.pass(HistoryRetention.Everything, NOW)
        assertEquals(HistoryCopier.Result.More(copied = 24, skipped = 1, left = 15), first)
        assertFalse(sha(3) in copy.ingested)
        // The watch brought file 30 between two passes.
        copy.have += sha(30)
        val second = copier.pass(HistoryRetention.Everything, NOW)
        assertEquals(HistoryCopier.Result.Done(copied = 14, skipped = 0, total = 38), second)
        assertFalse(sha(30) in server.downloads)
    }

    @Test
    fun `a failing download keeps what was copied, keeps the rest queued and reports the failure`() {
        val server = FakeServer((1..10).map { file(it) })
        val copy = FakeCopy()
        val copier = HistoryCopier(copy, server)
        server.failAfter = 4
        val result = copier.pass(HistoryRetention.Everything, NOW)
        assertEquals(HistoryCopier.Result.Failed("Connection reset", copied = 4), result)
        assertEquals(6, copier.left())
        // Later, the server is back: the pass goes on where it stopped, without reading the list again.
        server.failAfter = Int.MAX_VALUE
        val calls = server.listCalls.size
        assertEquals(HistoryCopier.Result.Done(6, 0, 10), copier.pass(HistoryRetention.Everything, NOW))
        assertEquals(calls, server.listCalls.size)
        assertEquals((1..10).map { sha(it) }, copy.ingested)
    }

    @Test
    fun `a refused token stops the pass and a failing list reads again next time`() {
        val server = FakeServer((1..5).map { file(it) })
        val copier = HistoryCopier(FakeCopy(), server)
        server.listReply = HistoryPage.Rejected(401)
        assertEquals(HistoryCopier.Result.TokenRejected(401, 0), copier.pass(HistoryRetention.Everything, NOW))
        server.listReply = HistoryPage.Failed("HTTP 503")
        assertEquals(HistoryCopier.Result.Failed("HTTP 503", 0), copier.pass(HistoryRetention.Everything, NOW))
        server.listReply = null
        server.downloadReply = HistoryDownload.Rejected(403)
        assertEquals(HistoryCopier.Result.TokenRejected(403, 0), copier.pass(HistoryRetention.Everything, NOW))
        server.downloadReply = null
        assertEquals(HistoryCopier.Result.Done(5, 0, 5), copier.pass(HistoryRetention.Everything, NOW))
    }

    @Test
    fun `a file the phone cannot take fails the pass without losing its place`() {
        val server = FakeServer((1..3).map { file(it) })
        val copy = FakeCopy()
        val copier = HistoryCopier(copy, server)
        copy.ingestFails = true
        val failed = copier.pass(HistoryRetention.Everything, NOW)
        assertInstanceOf(HistoryCopier.Result.Failed::class.java, failed)
        assertEquals(0, (failed as HistoryCopier.Result.Failed).copied)
        // The reason names the kind of failure only.
        assertFalse(failed.reason.contains("could not take it"))
        assertEquals(3, copier.left())
        copy.ingestFails = false
        assertEquals(HistoryCopier.Result.Done(3, 0, 3), copier.pass(HistoryRetention.Everything, NOW))
    }

    @Test
    fun `a server that repeats its cursor does not trap the walk`() {
        val server = object : HistoryServer {
            override fun list(cursor: String?, limit: Int, since: Long?) = HistoryPage.Files(listOf(file(1)), "same")

            override fun download(sha256: String): HistoryDownload = HistoryDownload.Gone
        }
        val result = HistoryCopier(FakeCopy(), server).pass(HistoryRetention.Everything, NOW)
        assertInstanceOf(HistoryCopier.Result.Failed::class.java, result)
    }

    @Test
    fun `changing the choice starts the walk over`() {
        val server = FakeServer((1..30).map { file(it) }, pageSize = 30)
        val copier = HistoryCopier(FakeCopy(), server)
        copier.pass(HistoryRetention.Last3Months, NOW)
        assertEquals(5, copier.left())
        copier.reset()
        assertEquals(0, copier.left())
        copier.pass(HistoryRetention.Everything, NOW)
        assertEquals(2, server.listCalls.size)
    }

    @Test
    fun `a failed step backs off 30 seconds doubling, on its own`() {
        val history = EditBackoff()
        val uploads = EditBackoff()
        assertEquals(0L, history.waitMs(NOW))
        assertEquals(30_000L, history.failed(NOW))
        assertEquals(60_000L, history.failed(NOW))
        assertEquals(30_000L, history.waitMs(NOW + 30_000L))
        assertEquals(0L, uploads.waitMs(NOW), "the uploads' back-off is not touched")
        history.succeeded()
        assertEquals(0L, history.waitMs(NOW))
    }

    @Test
    fun `the first look is due, a later one waits for a reason and for six hours`() {
        val r = HistoryRecheck()
        assertTrue(r.due(NOW))
        r.onChecked(NOW)
        // Nothing asks for another look: never.
        assertNull(r.waitMs(NOW + 100 * HOUR))
        assertFalse(r.due(NOW + 100 * HOUR))
        // New uploads reached the server: look again, but not within six hours of the last look.
        r.onUploadsReached()
        assertEquals(6 * HOUR - HOUR, r.waitMs(NOW + HOUR))
        assertFalse(r.due(NOW + 5 * HOUR))
        assertTrue(r.due(NOW + 6 * HOUR))
        // ...and once looked at, quiet again.
        r.onChecked(NOW + 6 * HOUR)
        assertNull(r.waitMs(NOW + 7 * HOUR))
    }

    @Test
    fun `app start asks for a look unless one just happened`() {
        // A new process remembers when the last walk ended.
        assertEquals(5 * HOUR, HistoryRecheck(NOW).waitMs(NOW + HOUR))
        assertTrue(HistoryRecheck(NOW).due(NOW + 7 * HOUR))
        assertTrue(HistoryRecheck(0L).due(NOW))
    }

    @Test
    fun `a wider choice and a walk under way do not wait`() {
        val r = HistoryRecheck()
        r.onChecked(NOW)
        r.onWidened()
        assertTrue(r.due(NOW + 1))
        r.onWorkLeft()
        r.onChecked(NOW + 2)
        assertFalse(r.due(NOW + 3))
        r.onWorkLeft()
        assertTrue(r.due(NOW + 3))
    }

    @Test
    fun `a clock that went back does not make the worker wait for hours`() {
        val r = HistoryRecheck(NOW)
        assertEquals(0L, r.waitMs(NOW - 10 * HOUR))
    }

    @Test
    fun `the log lines say what was copied and what is left`() {
        assertEquals("Copied 25 files of the server's history to the phone (310 left)", HistoryLog.pass(25, 310))
        assertEquals("The server's history is on the phone (800 files copied)", HistoryLog.done(800))
        assertEquals("The server's history is on the phone (1 file copied)", HistoryLog.done(1))
        assertEquals("Removed 12 files of the server's history from the phone (last 3 months)", HistoryLog.pruned(12, HistoryRetention.Last3Months))
        assertEquals("Server history on this phone: off", HistoryLog.choice(HistoryRetention.Off))
        assertEquals("Could not copy the server's history: Server error (HTTP 503), will retry", HistoryLog.failed("Server error (HTTP 503), will retry"))
    }
}
