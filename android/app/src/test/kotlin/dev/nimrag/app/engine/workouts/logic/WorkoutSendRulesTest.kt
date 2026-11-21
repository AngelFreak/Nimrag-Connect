package dev.nimrag.app.engine.workouts.logic

import dev.nimrag.app.engine.workouts.logic.WorkoutSendRules.Queued
import dev.nimrag.app.engine.workouts.logic.WorkoutSendRules.Report
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test
import uniffi.nimrag_ffi.UploadResult
import uniffi.nimrag_ffi.UploadStage

class WorkoutSendRulesTest {
    @Test
    fun `a workout is file type 128 sub type 5`() {
        assertEquals(128, WorkoutSendRules.DATA_TYPE)
        assertEquals(5, WorkoutSendRules.SUB_TYPE)
    }

    @Test
    fun `the queue is read in the server's order`() {
        val text = """{"workouts":[{"id":3,"name":"Tempo","revision":2},{"id":9,"name":"4 x 400 m","revision":1}]}"""
        assertEquals(listOf(Queued(3, "Tempo", 2), Queued(9, "4 x 400 m", 1)), WorkoutSendRules.parseQueue(text))
        assertEquals(emptyList<Queued>(), WorkoutSendRules.parseQueue("""{"workouts":[]}"""))
    }

    @Test
    fun `a queue that is not a queue is refused and bad entries are dropped`() {
        assertNull(WorkoutSendRules.parseQueue("not json"))
        assertNull(WorkoutSendRules.parseQueue("""{"other":[]}"""))
        assertNull(WorkoutSendRules.parseQueue("[]"))
        val text = """{"workouts":[{"id":0,"name":"x","revision":1},{"id":4,"revision":1},{"id":5,"name":"ok","revision":3},7]}"""
        assertEquals(listOf(Queued(5, "ok", 3)), WorkoutSendRules.parseQueue(text))
    }

    @Test
    fun `workouts already on their way are not fetched twice and a look takes at most four`() {
        val queue = (1L..9L).map { Queued(it, "w$it", 1) }
        assertEquals(listOf(1L, 3L, 4L, 5L), WorkoutSendRules.pick(queue, setOf(2L)).map { it.id })
        assertEquals(emptyList<Queued>(), WorkoutSendRules.pick(queue.take(2), setOf(1L, 2L)))
    }

    @Test
    fun `the result body is exact`() {
        assertEquals("""{"revision":4,"ok":true}""", WorkoutSendRules.resultBody(4, true, null))
        assertEquals("""{"revision":4,"ok":true}""", WorkoutSendRules.resultBody(4, true, "ignored"))
        assertEquals(
            """{"revision":2,"ok":false,"error":"it \"failed\""}""",
            WorkoutSendRules.resultBody(2, false, "it \"failed\""),
        )
        assertEquals("""{"revision":2,"ok":false}""", WorkoutSendRules.resultBody(2, false, null))
        val long = WorkoutSendRules.resultBody(2, false, "x".repeat(500))
        assertEquals(200, long.count { it == 'x' })
    }

    @Test
    fun `the server's error text is shown and cut`() {
        val body = """{"error":"this server was built without the FIT profile"}""".toByteArray()
        assertEquals("this server was built without the FIT profile", WorkoutSendRules.errorText(body, "fallback"))
        assertEquals("fallback", WorkoutSendRules.errorText("<html>".toByteArray(), "fallback"))
        assertEquals("fallback", WorkoutSendRules.errorText(ByteArray(0), "fallback"))
        val long = """{"error":"${"e".repeat(600)}"}""".toByteArray()
        assertEquals(WorkoutSendRules.MAX_REASON_CHARS, WorkoutSendRules.errorText(long, "f").length)
    }

    @Test
    fun `sent is reported as sent and a closed link is not reported at all`() {
        assertEquals(Report.Sent, WorkoutSendRules.reportFor(UploadResult.Sent))
        assertEquals(Report.Keep, WorkoutSendRules.reportFor(UploadResult.Aborted))
    }

    @Test
    fun `every other result is a failure with its own sentence`() {
        val results = listOf(
            UploadResult.Refused(UploadStage.CREATE, 2u),
            UploadResult.Refused(UploadStage.REQUEST, 5u),
            UploadResult.Refused(UploadStage.DATA, 3u),
            UploadResult.Nak(UploadStage.CREATE, 1u),
            UploadResult.TimedOut(UploadStage.REQUEST),
            UploadResult.BadReply(UploadStage.DATA),
            UploadResult.Invalid,
        )
        val sentences = results.map {
            val report = WorkoutSendRules.reportFor(it)
            assertTrue(report is Report.Failed, "$it")
            (report as Report.Failed).reason
        }
        assertEquals(results.size, sentences.toSet().size, "each says something different")
        assertTrue(sentences.all { it.length in 10..200 && it.none(Char::isISOControl) })
    }

    @Test
    fun `refusals name the watch's status the way Gadgetbridge does`() {
        assertEquals(
            "The watch refused the workout while creating the file (status 2, NO_SPACE)",
            WorkoutSendRules.describe(UploadResult.Refused(UploadStage.CREATE, 2u)),
        )
        assertEquals(
            "The watch refused the workout while starting the transfer (status 5, NOT_READY)",
            WorkoutSendRules.describe(UploadResult.Refused(UploadStage.REQUEST, 5u)),
        )
        assertEquals(
            "The watch refused the workout while sending the data (status 3, CRC_MISMATCH)",
            WorkoutSendRules.describe(UploadResult.Refused(UploadStage.DATA, 3u)),
        )
        assertEquals("status 99", WorkoutSendRules.statusName(UploadStage.DATA, 99))
        assertFalse(WorkoutSendRules.describe(UploadResult.Refused(UploadStage.DATA, 99u)).contains("null"))
    }
}
