package dev.nimrag.app.engine.logic

import dev.nimrag.app.engine.auth.logic.MiniJson
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

/** The outbox as the Rust side keeps it (same verdict table as `docs/design/offline-edits.md`, "FFI"). */
private class FakeOutbox(edits: List<OutboxEdit> = emptyList()) : EditOutbox {
    val queue = ArrayDeque(edits)
    val reported = mutableListOf<Pair<String, Int>>()
    val dropped = mutableListOf<DroppedChange>()
    var applied: String? = null
    var applyThrows = false

    override fun next(): OutboxEdit? = queue.firstOrNull()

    override fun done(opId: String, status: Int, responseBody: String): EditVerdict {
        reported += opId to status
        val edit = queue.firstOrNull { it.opId == opId } ?: return EditVerdict.Applied
        return when {
            status in 200..299 -> {
                queue.remove(edit)
                EditVerdict.Applied
            }
            status in 400..499 && status !in listOf(401, 403, 408, 429) -> {
                queue.remove(edit)
                dropped += DroppedChange(edit.method, edit.path, "refused with $status")
                EditVerdict.Dropped
            }
            else -> EditVerdict.Kept
        }
    }

    override fun pending(): Int = queue.size

    override fun takeDropped(): List<DroppedChange> = dropped.toList().also { dropped.clear() }

    override fun applyState(json: String): Boolean {
        if (applyThrows) throw IllegalStateException("not the state")
        if (queue.isNotEmpty()) return false
        applied = json
        return true
    }
}

private fun edit(n: Int, method: String = "PATCH", path: String = "/api/settings") =
    OutboxEdit(opId = "op-0000000$n", method = method, path = path, body = "{\"step_goal\":$n}", madeAt = 1_800_000_000L + n)

class EditDrainTest {
    private fun drain(outbox: FakeOutbox, answer: (OutboxEdit) -> ServerReply): Pair<EditDrain.Report, List<String>> {
        val sent = mutableListOf<String>()
        val report = EditDrain(outbox).run { e ->
            sent += e.opId
            answer(e)
        }
        return report to sent
    }

    @Test
    fun `sends oldest first, one at a time, until the outbox is empty`() {
        val outbox = FakeOutbox(listOf(edit(1), edit(2), edit(3)))
        val (report, sent) = drain(outbox) { ServerReply(200, "{}") }
        assertEquals(listOf("op-00000001", "op-00000002", "op-00000003"), sent)
        assertEquals(EditDrain.Report(applied = 3, dropped = 0, end = EditDrain.End.Empty), report)
        assertEquals(0, outbox.pending())
    }

    @Test
    fun `an empty outbox sends nothing`() {
        val (report, sent) = drain(FakeOutbox()) { error("nothing to send") }
        assertTrue(sent.isEmpty())
        assertEquals(EditDrain.Report(0, 0, EditDrain.End.Empty), report)
    }

    @Test
    fun `a refused change is dropped and the next one goes on`() {
        val outbox = FakeOutbox(listOf(edit(1), edit(2, "PUT", "/api/workouts/7"), edit(3)))
        val (report, sent) = drain(outbox) { e -> if (e.opId.endsWith("2")) ServerReply(404, "{}") else ServerReply(200, "{}") }
        assertEquals(3, sent.size)
        assertEquals(EditDrain.Report(applied = 2, dropped = 1, end = EditDrain.End.Empty), report)
        assertEquals(listOf("/api/workouts/7"), outbox.dropped.map { it.path })
    }

    @Test
    fun `server trouble keeps the change and stops the pass`() {
        for (status in listOf(500, 502, 503, 408, 429)) {
            val outbox = FakeOutbox(listOf(edit(1), edit(2)))
            val (report, sent) = drain(outbox) { ServerReply(status, "{}") }
            assertEquals(1, sent.size, "status $status")
            assertEquals(0, report.applied)
            assertTrue(report.end is EditDrain.End.Kept, "status $status")
            assertEquals(2, outbox.pending(), "status $status")
        }
    }

    @Test
    fun `no answer reports status 0, keeps the change and says why`() {
        val outbox = FakeOutbox(listOf(edit(1)))
        val (report, _) = drain(outbox) { ServerReply.noAnswer("Network error: the server cannot be reached") }
        assertEquals(listOf("op-00000001" to 0), outbox.reported)
        assertEquals(EditDrain.End.Kept("Network error: the server cannot be reached"), report.end)
        assertEquals(1, outbox.pending())
    }

    @Test
    fun `401 and 403 from the endpoint stop the pass as a rejected token`() {
        for (status in listOf(401, 403)) {
            val outbox = FakeOutbox(listOf(edit(1), edit(2)))
            val (report, sent) = drain(outbox) { ServerReply(status, "{\"error\":\"no\"}") }
            assertEquals(1, sent.size)
            assertEquals(EditDrain.End.TokenRejected(status), report.end)
            assertEquals(2, outbox.pending(), "the change stays")
        }
    }

    @Test
    fun `changes sent before a failure stay done`() {
        val outbox = FakeOutbox(listOf(edit(1), edit(2), edit(3)))
        val (report, sent) = drain(outbox) { e -> if (e.opId.endsWith("3")) ServerReply(503) else ServerReply(201, "{}") }
        assertEquals(3, sent.size)
        assertEquals(2, report.applied)
        assertEquals(1, outbox.pending())
    }

    @Test
    fun `a change reported done that comes back again does not loop`() {
        val stuck = object : EditOutbox by FakeOutbox() {
            override fun next() = edit(1)
            override fun done(opId: String, status: Int, responseBody: String) = EditVerdict.Applied
        }
        var calls = 0
        val report = EditDrain(stuck).run {
            calls++
            ServerReply(200, "{}")
        }
        assertEquals(1, calls)
        assertTrue(report.end is EditDrain.End.Kept)
    }
}

class StatePullTest {
    private val state = "{\"version\":1}"

    @Test
    fun `pulls only with an empty outbox`() {
        var fetched = 0
        val waiting = FakeOutbox(listOf(edit(1)))
        assertEquals(StatePull.Result.Skipped, StatePull(waiting).run { fetched++; ServerReply(200, state) })
        assertEquals(0, fetched, "no request at all while changes wait")
        assertNull(waiting.applied)

        val empty = FakeOutbox()
        assertEquals(StatePull.Result.Applied, StatePull(empty).run { fetched++; ServerReply(200, state) })
        assertEquals(1, fetched)
        assertEquals(state, empty.applied)
    }

    @Test
    fun `a change made during the pull is not undone and is no failure`() {
        val outbox = FakeOutbox()
        val result = StatePull(outbox).run {
            outbox.queue += edit(1) // the person edits while the answer travels
            ServerReply(200, state)
        }
        assertEquals(StatePull.Result.Later, result)
        assertNull(outbox.applied)
    }

    @Test
    fun `a refused token stops, other failures back off`() {
        val outbox = FakeOutbox()
        assertEquals(StatePull.Result.TokenRejected(401), StatePull(outbox).run { ServerReply(401) })
        assertEquals(StatePull.Result.TokenRejected(403), StatePull(outbox).run { ServerReply(403) })
        assertTrue(StatePull(outbox).run { ServerReply(503) } is StatePull.Result.Failed)
        assertTrue(StatePull(outbox).run { ServerReply(404, "{}") } is StatePull.Result.Failed)
        assertTrue(StatePull(outbox).run { ServerReply.noAnswer("Network error: the connection failed") } is StatePull.Result.Failed)
        assertNull(outbox.applied)
    }

    @Test
    fun `an answer that is not a state is a failure with a fixed sentence`() {
        val outbox = FakeOutbox().also { it.applyThrows = true }
        val result = StatePull(outbox).run { ServerReply(200, "<html>secret-looking page</html>") }
        assertEquals(StatePull.Result.Failed("The server's state could not be read"), result)
    }

    @Test
    fun `resume pulls again only after the gap`() {
        assertTrue(EditPolicy.resumePullDue(lastPullAtMs = 0, nowMs = 1_000))
        assertFalse(EditPolicy.resumePullDue(lastPullAtMs = 100_000, nowMs = 110_000))
        assertTrue(EditPolicy.resumePullDue(lastPullAtMs = 100_000, nowMs = 100_000 + EditPolicy.RESUME_PULL_GAP_MS))
        assertTrue(EditPolicy.resumePullDue(lastPullAtMs = 100_000, nowMs = 50_000), "a clock set back")
    }
}

class EditPolicyTest {
    @Test
    fun `only a JSON answer counts for a change`() {
        assertEquals(ServerReply(200, "{}"), EditPolicy.accept(200, "application/json", "{}"))
        assertEquals(ServerReply(404, "{\"error\":\"x\"}"), EditPolicy.accept(404, "application/json; charset=utf-8", "{\"error\":\"x\"}"))
        // A hotspot's login page, a proxy's error page, an old server without the route.
        for (status in listOf(200, 204, 400, 404, 405)) {
            val reply = EditPolicy.accept(status, "text/html", "<html></html>")
            assertEquals(0, reply.status, "status $status")
            assertTrue(reply.problem!!.contains("HTTP $status"))
        }
        assertEquals(0, EditPolicy.accept(200, null, "{}").status)
    }

    @Test
    fun `401, 403 and server errors are kept as they are`() {
        for (status in listOf(401, 403, 500, 502, 503)) {
            assertEquals(ServerReply(status, "x"), EditPolicy.accept(status, "text/plain", "x"))
        }
    }

    @Test
    fun `status 0 is no answer`() {
        assertEquals(0, EditPolicy.accept(0, "application/json", "").status)
    }

    @Test
    fun `the request body carries the change's JSON text as a string`() {
        val e = OutboxEdit("op-00000001", "PATCH", "/api/settings", "{\"timezone\":\"Europe/Copenhagen\"}", 1_800_000_000L)
        val sent = MiniJson.parseObject(EditPolicy.requestJson(e))!!
        assertEquals("op-00000001", sent["op_id"])
        assertEquals("PATCH", sent["method"])
        assertEquals("/api/settings", sent["path"])
        assertEquals("{\"timezone\":\"Europe/Copenhagen\"}", sent["body"])
        assertEquals(1_800_000_000.0, sent["made_at"])
        // No body: an empty string, not null.
        assertEquals("", MiniJson.parseObject(EditPolicy.requestJson(e.copy(body = "")))!!["body"])
    }

    @Test
    fun `reasons for a change that stays`() {
        assertEquals("Server error (HTTP 503), will retry", EditPolicy.keptReason(ServerReply(503)))
        assertEquals("HTTP 429, will retry", EditPolicy.keptReason(ServerReply(429)))
        assertEquals("No answer from the server", EditPolicy.keptReason(ServerReply(0)))
        assertEquals("Network error: x", EditPolicy.keptReason(ServerReply.noAnswer("Network error: x")))
        assertTrue(EditPolicy.keptReason(ServerReply(403)).contains("no longer accepts"))
    }
}

class EditBackoffTest {
    @Test
    fun `waits 30 seconds doubling to an hour, like a file`() {
        val b = EditBackoff()
        assertEquals(0L, b.waitMs(1_000))
        assertEquals(30_000L, b.failed(1_000))
        assertEquals(29_000L, b.waitMs(2_000))
        assertEquals(0L, b.waitMs(31_000))
        assertEquals(60_000L, b.failed(31_000))
        assertEquals(120_000L, b.failed(100_000))
        repeat(20) { b.failed(1_000_000) }
        assertEquals(UploadPolicy.BACKOFF_MAX_MS, b.failed(1_000_000))
    }

    @Test
    fun `success and upload now clear it`() {
        val b = EditBackoff()
        b.failed(0)
        b.failed(0)
        b.succeeded()
        assertEquals(0L, b.waitMs(1))
        assertEquals(30_000L, b.failed(0))
    }
}

class EditCountRuleTest {
    @Test
    fun `a new change wakes the worker and updates the page`() {
        assertEquals(EditCountRule.Decision(publish = true, wake = true), EditCountRule.decide(0, 1))
        assertEquals(EditCountRule.Decision(publish = true, wake = true), EditCountRule.decide(2, 3))
    }

    @Test
    fun `fewer changes only update the page`() {
        assertEquals(EditCountRule.Decision(publish = true, wake = false), EditCountRule.decide(3, 1))
        assertEquals(EditCountRule.Decision(publish = true, wake = false), EditCountRule.decide(1, 0))
    }

    @Test
    fun `no change does nothing`() {
        assertEquals(EditCountRule.Decision(publish = false, wake = false), EditCountRule.decide(2, 2))
        assertEquals(EditCountRule.Decision(publish = false, wake = false), EditCountRule.decide(0, 0))
    }

    @Test
    fun `the first look wakes only when something waits`() {
        assertEquals(EditCountRule.Decision(publish = true, wake = true), EditCountRule.decide(null, 4))
        assertEquals(EditCountRule.Decision(publish = true, wake = false), EditCountRule.decide(null, 0))
    }
}

class DroppedEditsTest {
    @Test
    fun `each allowed request has plain words`() {
        assertEquals("a change to your settings", DroppedEdits.what("PATCH", "/api/settings"))
        assertEquals("a new weight or body entry", DroppedEdits.what("POST", "/api/body"))
        assertEquals("deleting a weight or body entry", DroppedEdits.what("DELETE", "/api/body/12"))
        assertEquals("a change to a strength session", DroppedEdits.what("PATCH", "/api/activities/ab12@1700000000/strength"))
        assertEquals("a new workout", DroppedEdits.what("POST", "/api/workouts"))
        assertEquals("a change to a workout", DroppedEdits.what("PUT", "/api/workouts/7"))
        assertEquals("deleting a workout", DroppedEdits.what("DELETE", "/api/workouts/7"))
        assertEquals("sending a workout to the watch", DroppedEdits.what("POST", "/api/workouts/7/send"))
        assertEquals("taking a workout off the watch", DroppedEdits.what("DELETE", "/api/workouts/7/send"))
        assertEquals("a change", DroppedEdits.what("GET", "/api/else"))
    }

    @Test
    fun `the message names the change and the server's reason`() {
        val change = DroppedChange("PUT", "/api/workouts/7", "no such workout.")
        assertEquals(
            "A change made offline could not be saved: a change to a workout (no such workout)",
            DroppedEdits.message(change),
        )
        assertEquals(
            "A change made offline could not be saved: a new workout",
            DroppedEdits.message(DroppedChange("POST", "/api/workouts", "  ")),
        )
    }

    @Test
    fun `the log line has the kind and reason but not the path`() {
        val line = DroppedEdits.logLine(DroppedChange("DELETE", "/api/workouts/987654", "no such workout"))
        assertEquals("Dropped an offline change (deleting a workout): no such workout", line)
        assertFalse(line.contains("987654"))
    }
}
