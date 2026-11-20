package dev.nimrag.app.engine.auth.logic

import com.sun.net.httpserver.HttpServer
import org.junit.jupiter.api.AfterEach
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertThrows
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.BeforeEach
import org.junit.jupiter.api.Test
import java.io.IOException
import java.net.InetAddress
import java.net.InetSocketAddress

/** [HttpUrlTransport] against a real loopback server, and the whole app API through it. */
class HttpUrlTransportTest {
    private lateinit var server: HttpServer
    private var seenMethod = ""
    private var seenPath = ""
    private var seenBody = ""
    private val seenHeaders = HashMap<String, String>()
    private val hex = "ef".repeat(32)

    private val base get() = "http://127.0.0.1:${server.address.port}"

    @BeforeEach
    fun start() {
        server = HttpServer.create(InetSocketAddress(InetAddress.getLoopbackAddress(), 0), 0)
        server.createContext("/") { ex ->
            seenMethod = ex.requestMethod
            seenPath = ex.requestURI.path
            seenBody = ex.requestBody.readBytes().toString(Charsets.UTF_8)
            for ((k, v) in ex.requestHeaders) seenHeaders[k.lowercase()] = v.first()
            when (ex.requestURI.path) {
                "/api/app/login" -> {
                    ex.responseHeaders.add("Set-Cookie", "nimrag_session=$hex; Path=/; Max-Age=2592000; HttpOnly; SameSite=Strict")
                    val body = """{"me":{"authenticated":true,"username":"anna","role":"user","timezone":"UTC","sample":false},"device_id":3,"token":"nrg_t","device_secret":"nrd_s"}"""
                    respond(ex, 201, body)
                }
                "/api/app/unlock" -> respond(ex, 401, """{"error":"unauthorized"}""")
                "/api/app/resume" -> {
                    ex.responseHeaders.add("Retry-After", "42")
                    respond(ex, 429, """{"error":"too many"}""")
                }
                "/big" -> respond(ex, 200, "x".repeat(HttpUrlTransport.MAX_BODY + 10))
                "/redirect" -> {
                    ex.responseHeaders.add("Location", "/api/me")
                    respond(ex, 302, "")
                }
                else -> respond(ex, 404, "{}")
            }
        }
        server.start()
    }

    @AfterEach
    fun stop() = server.stop(0)

    private fun respond(ex: com.sun.net.httpserver.HttpExchange, status: Int, body: String) {
        val bytes = body.toByteArray()
        ex.sendResponseHeaders(status, if (bytes.isEmpty()) -1 else bytes.size.toLong())
        if (bytes.isNotEmpty()) ex.responseBody.use { it.write(bytes) }
        ex.close()
    }

    @Test
    fun sendsMethodHeadersAndBodyAndReadsStatusHeadersAndBody() {
        val reply = HttpUrlTransport().send(HttpRequest("POST", "$base/api/app/login", mapOf("X-Nimrag" to "1", "Content-Type" to "application/json"), """{"a":"å"}"""))
        assertEquals(201, reply.status)
        assertEquals("POST", seenMethod)
        assertEquals("1", seenHeaders["x-nimrag"])
        assertEquals("""{"a":"å"}""", seenBody)
        assertTrue(reply.header("set-cookie")!!.startsWith("nimrag_session="))
        assertTrue(reply.body.contains("nrg_t"))
    }

    @Test
    fun errorStatusesStillCarryTheirBodyAndHeaders() {
        val reply = HttpUrlTransport().send(HttpRequest("POST", "$base/api/app/resume", emptyMap(), "{}"))
        assertEquals(429, reply.status)
        assertEquals("42", reply.header("Retry-After"))
        assertTrue(reply.body.contains("too many"))
    }

    @Test
    fun aRedirectIsNotFollowed() {
        val reply = HttpUrlTransport().send(HttpRequest("GET", "$base/redirect"))
        assertEquals(302, reply.status)
        assertEquals("/api/me", reply.header("location"))
    }

    @Test
    fun anOverlongAnswerIsAnIoError() {
        assertThrows(IOException::class.java) { HttpUrlTransport().send(HttpRequest("GET", "$base/big")) }
    }

    @Test
    fun nothingListeningIsAnIoError() {
        val closed = "http://127.0.0.1:${server.address.port}"
        server.stop(0)
        assertThrows(IOException::class.java) { HttpUrlTransport(500).send(HttpRequest("GET", "$closed/api/me")) }
        // Restart so @AfterEach has something to stop.
        start()
    }

    @Test
    fun theWholeLoginFlowWorksOverRealHttp() {
        val api = AuthApi(HttpUrlTransport())
        val grant = (api.login(base, "anna", "pw", "123456", "Pixel 8a") as LoginResult.Success).grant
        assertEquals("nrg_t", grant.token)
        assertEquals("nimrag_session", grant.cookie.name)
        assertEquals(SessionResult.Failure(AuthFailure.WrongPasswordOrRemoved), api.unlock(base, "anna", "x", "nrd_s"))
        assertEquals(SessionResult.Failure(AuthFailure.RateLimited(42)), api.resume(base, "nrd_s"))
    }
}
