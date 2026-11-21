package dev.nimrag.app.engine.phone.logic

import dev.nimrag.app.engine.workouts.logic.HttpReply
import org.junit.jupiter.api.Assertions.assertArrayEquals
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertNotNull
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Assertions.assertThrows
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test
import java.io.IOException
import java.util.Base64

private class FakeWeatherHttp : WeatherHttp {
    var reply = HttpReply(200, null, ByteArray(0))
    var fail = false
    var url: String? = null
    var token: String? = null
    var body: String? = null
    var maxBytes = 0

    override fun post(url: String, token: String, json: String, maxBytes: Int): HttpReply {
        this.url = url
        this.token = token
        this.body = json
        this.maxBytes = maxBytes
        if (fail) throw IOException("down")
        return reply
    }
}

class WeatherRulesTest {
    private val hour = 60L * 60 * 1000
    private val b64 = Base64.getEncoder()
    private val cell = WeatherCell(557, 126)

    private fun answerJson(fetchedAt: String = "\"fetched_at\":1800000000") =
        """{"definitions":"${b64.encodeToString(byteArrayOf(0x40, 0, 0))}","records":["${b64.encodeToString(byteArrayOf(1, 2, 3))}","${b64.encodeToString(byteArrayOf(4))}"],$fetchedAt}"""

    // ---- the request ----

    @Test
    fun `tenths of a degree become exact decimal text`() {
        assertEquals("55.7", WeatherRules.tenths(557))
        assertEquals("12.6", WeatherRules.tenths(126))
        assertEquals("0.0", WeatherRules.tenths(0))
        assertEquals("0.5", WeatherRules.tenths(5))
        assertEquals("-0.5", WeatherRules.tenths(-5))
        assertEquals("-33.9", WeatherRules.tenths(-339))
        assertEquals("180.0", WeatherRules.tenths(1800))
        assertEquals("-180.0", WeatherRules.tenths(-1800))
    }

    @Test
    fun `the request body is the cell, the hours and the format`() {
        assertEquals("""{"lat":55.7,"lon":12.6,"hours":12,"format":1}""", WeatherRules.requestBody(cell, 12, 1))
        assertEquals("""{"lat":-33.9,"lon":-0.5,"hours":0,"format":255}""", WeatherRules.requestBody(WeatherCell(-339, -5), -3, 999))
    }

    // ---- the answer ----

    @Test
    fun `a good answer is decoded`() {
        val got = WeatherRules.parseAnswer(answerJson(), receivedAtMs = 1_800_000_500_000L)!!
        assertArrayEquals(byteArrayOf(0x40, 0, 0), got.definitions)
        assertEquals(2, got.records.size)
        assertArrayEquals(byteArrayOf(1, 2, 3), got.records[0])
        assertArrayEquals(byteArrayOf(4), got.records[1])
        assertEquals(1_800_000_000_000L, got.fetchedAtMs)
    }

    @Test
    fun `a server clock that runs ahead cannot make a forecast look fresh`() {
        val got = WeatherRules.parseAnswer(answerJson("\"fetched_at\":1900000000"), receivedAtMs = 1_800_000_000_000L)!!
        assertEquals(1_800_000_000_000L, got.fetchedAtMs)
    }

    @Test
    fun `a missing time is the time we received it`() {
        val got = WeatherRules.parseAnswer(answerJson("\"x\":1"), receivedAtMs = 42_000L)!!
        assertEquals(42_000L, got.fetchedAtMs)
    }

    @Test
    fun `anything that is not an answer is refused`() {
        val d = b64.encodeToString(byteArrayOf(1))
        val bad = listOf(
            "",
            "<html>",
            "[]",
            """{"records":["$d"]}""",
            """{"definitions":"$d"}""",
            """{"definitions":"$d","records":[]}""",
            """{"definitions":"","records":["$d"]}""",
            """{"definitions":"$d","records":[""]}""",
            """{"definitions":"$d","records":[1]}""",
            """{"definitions":"not base64!","records":["$d"]}""",
            """{"definitions":"$d","records":["%%%"]}""",
            """{"definitions":"$d","records":"$d"}""",
            """{"definitions":"$d","records":[${(1..WeatherRules.MAX_RECORDS + 1).joinToString(",") { "\"$d\"" }}]}""",
        )
        for (text in bad) assertNull(WeatherRules.parseAnswer(text, 1L), text)
    }

    @Test
    fun `the answer and the cell say nothing when printed`() {
        val answer = WeatherRules.parseAnswer(answerJson(), 1L)!!
        assertEquals("WeatherAnswer", answer.toString())
        assertEquals("WeatherCell", cell.toString())
        assertFalse(cell.toString().contains("557"))
    }

    @Test
    fun `failures are described in fixed words`() {
        assertEquals("the server could not be reached", WeatherRules.describeFailure(0))
        assertEquals("the server does not accept this phone", WeatherRules.describeFailure(401))
        assertEquals("the server does not accept this phone", WeatherRules.describeFailure(403))
        assertEquals("this server has no weather feature yet", WeatherRules.describeFailure(404))
        assertEquals("the server could not get a forecast", WeatherRules.describeFailure(502))
        assertEquals("weather is off on the server or it cannot write it", WeatherRules.describeFailure(503))
        assertEquals("the server answered HTTP 418", WeatherRules.describeFailure(418))
    }

    // ---- the API ----

    @Test
    fun `the forecast comes from the device endpoint with the token and the cell`() {
        val http = FakeWeatherHttp()
        http.reply = HttpReply(200, null, answerJson().toByteArray())
        val api = WeatherApi(http, "https://nimrag.example.ts.net", "tok")
        val got = api.forecast(cell, 12, 1, nowMs = 1_800_000_100_000L)
        assertTrue(got is WeatherFetch.Ok)
        assertEquals("https://nimrag.example.ts.net/api/device/weather", http.url)
        assertEquals("tok", http.token)
        assertEquals("""{"lat":55.7,"lon":12.6,"hours":12,"format":1}""", http.body)
        assertEquals(WeatherRules.MAX_REPLY_BYTES, http.maxBytes)
    }

    @Test
    fun `an error status and a reply that is not an answer both say failed`() {
        val http = FakeWeatherHttp()
        val api = WeatherApi(http, "https://s", "t")
        http.reply = HttpReply(503, null, """{"error":"weather is switched off"}""".toByteArray())
        assertEquals(WeatherFetch.Failed(503), api.forecast(cell, 12, 1, 0))
        http.reply = HttpReply(200, null, "<html>".toByteArray())
        assertEquals(WeatherFetch.Failed(0), api.forecast(cell, 12, 1, 0))
    }

    @Test
    fun `an IO error is thrown for the caller to describe`() {
        val http = FakeWeatherHttp()
        http.fail = true
        assertThrows(IOException::class.java) { WeatherApi(http, "https://s", "t").forecast(cell, 12, 1, 0) }
    }

    // ---- the memory ----

    private fun answer(fetchedAtMs: Long) = WeatherAnswer(byteArrayOf(1), listOf(byteArrayOf(2)), fetchedAtMs)

    @Test
    fun `an answer for the same cell is reused for under three hours`() {
        val memory = WeatherMemory()
        val a = answer(1_000_000L)
        memory.remember(cell, a)
        assertEquals(a, memory.reusable(WeatherCell(557, 126), 1_000_000L))
        assertEquals(a, memory.reusable(cell, 1_000_000L + 3 * hour - 1))
        assertNull(memory.reusable(cell, 1_000_000L + 3 * hour))
        assertNull(memory.reusable(cell, 1_000_000L + 4 * hour))
    }

    @Test
    fun `another cell, even the next one over, gets no old answer`() {
        val memory = WeatherMemory()
        memory.remember(cell, answer(1_000_000L))
        assertNull(memory.reusable(WeatherCell(558, 126), 1_000_000L))
        assertNull(memory.reusable(WeatherCell(557, 127), 1_000_000L))
    }

    @Test
    fun `the age counts from when the forecast was fetched`() {
        val memory = WeatherMemory()
        memory.remember(cell, answer(fetchedAtMs = 0L))
        assertNotNull(memory.reusable(cell, 3 * hour - 1))
        assertNull(memory.reusable(cell, 3 * hour))
    }

    @Test
    fun `a clock that went backwards reuses nothing`() {
        val memory = WeatherMemory()
        memory.remember(cell, answer(5_000_000L))
        assertNull(memory.reusable(cell, 4_999_999L))
    }

    @Test
    fun `only the latest answer is kept and clearing forgets it`() {
        val memory = WeatherMemory()
        memory.remember(cell, answer(1_000L))
        memory.remember(WeatherCell(1, 2), answer(2_000L))
        assertNull(memory.reusable(cell, 3_000L))
        assertNotNull(memory.reusable(WeatherCell(1, 2), 3_000L))
        memory.clear()
        assertNull(memory.reusable(WeatherCell(1, 2), 3_000L))
    }
}
