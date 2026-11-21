package dev.nimrag.app.engine.workouts.logic

import dev.nimrag.app.engine.workouts.logic.WorkoutSendRules.Queued
import org.junit.jupiter.api.Assertions.assertArrayEquals
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertThrows
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test
import java.io.IOException

private class FakeHttp : WorkoutHttp {
    val calls = mutableListOf<String>()
    var reply: HttpReply = HttpReply(200, null, ByteArray(0))
    var fail = false
    var lastBody: String? = null
    var lastToken: String? = null

    override fun get(url: String, token: String, maxBytes: Int): HttpReply = answer("GET $url", token, null)

    override fun post(url: String, token: String, json: String): HttpReply = answer("POST $url", token, json)

    private fun answer(call: String, token: String, body: String?): HttpReply {
        calls += call
        lastToken = token
        lastBody = body
        if (fail) throw IOException("down")
        return reply
    }
}

class WorkoutApiTest {
    private val http = FakeHttp()
    private val api = WorkoutApi(http, "https://nimrag.example.ts.net", "tok")

    @Test
    fun `the queue comes from the device endpoint with the token`() {
        http.reply = HttpReply(200, null, """{"workouts":[{"id":3,"name":"Tempo","revision":2}]}""".toByteArray())
        val got = api.queue()
        assertEquals(listOf("GET https://nimrag.example.ts.net/api/device/workouts"), http.calls)
        assertEquals("tok", http.lastToken)
        assertEquals(QueueFetch.Ok(listOf(Queued(3, "Tempo", 2))), got)
    }

    @Test
    fun `an answer that is not a queue and an error status both say failed`() {
        http.reply = HttpReply(200, null, "<html>".toByteArray())
        assertEquals(QueueFetch.Failed(0), api.queue())
        http.reply = HttpReply(403, null, """{"error":"not assigned"}""".toByteArray())
        assertEquals(QueueFetch.Failed(403), api.queue())
        http.reply = HttpReply(404, null, ByteArray(0))
        assertEquals(QueueFetch.Failed(404), api.queue())
    }

    @Test
    fun `an IO error is thrown for the caller to describe`() {
        http.fail = true
        assertThrows(IOException::class.java) { api.queue() }
        assertThrows(IOException::class.java) { api.report(1, 1, true, null) }
    }

    @Test
    fun `the fit file arrives with the revision the server made it from`() {
        http.reply = HttpReply(200, "3", byteArrayOf(14, 16, 1, 2, 3))
        val got = api.fit(Queued(7, "Tempo", 2)) as FitFetch.Ok
        assertEquals("GET https://nimrag.example.ts.net/api/device/workouts/7/fit", http.calls.single())
        assertArrayEquals(byteArrayOf(14, 16, 1, 2, 3), got.bytes)
        assertEquals(3L, got.revision, "the header wins over the queue's number")
    }

    @Test
    fun `a missing or bad revision header falls back to the queue's`() {
        for (header in listOf(null, "", "abc", "0", "-4")) {
            http.reply = HttpReply(200, header, byteArrayOf(1))
            assertEquals(2L, (api.fit(Queued(7, "w", 2)) as FitFetch.Ok).revision, "header $header")
        }
    }

    @Test
    fun `a server without the profile says so and an empty file is a refusal`() {
        http.reply = HttpReply(503, null, """{"error":"this server was built without the FIT profile"}""".toByteArray())
        assertEquals(
            FitFetch.Refused(503, "this server was built without the FIT profile"),
            api.fit(Queued(7, "w", 1)),
        )
        http.reply = HttpReply(200, null, ByteArray(0))
        assertTrue(api.fit(Queued(7, "w", 1)) is FitFetch.Refused)
    }

    @Test
    fun `a workout that is no longer queued is gone`() {
        http.reply = HttpReply(404, null, """{"error":"this workout is not queued"}""".toByteArray())
        assertEquals(FitFetch.Gone, api.fit(Queued(7, "w", 1)))
    }

    @Test
    fun `a report goes to the result endpoint as json`() {
        http.reply = HttpReply(200, null, """{"applied":true}""".toByteArray())
        assertTrue(api.report(7, 3, false, "the watch refused"))
        assertEquals("POST https://nimrag.example.ts.net/api/device/workouts/7/result", http.calls.single())
        assertEquals("""{"revision":3,"ok":false,"error":"the watch refused"}""", http.lastBody)
        assertTrue(api.report(7, 3, true, null))
        assertEquals("""{"revision":3,"ok":true}""", http.lastBody)
    }

    @Test
    fun `a report is done when the server took it or the workout is gone, not on an error`() {
        http.reply = HttpReply(404, null, ByteArray(0))
        assertTrue(api.report(7, 1, true, null))
        for (status in listOf(400, 401, 403, 500, 503)) {
            http.reply = HttpReply(status, null, ByteArray(0))
            assertFalse(api.report(7, 1, true, null), "status $status")
        }
    }
}
