package dev.nimrag.app.engine.auth.logic

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test
import java.io.IOException

/** The app API against a fake transport: what is sent, and what each answer becomes. */
class AuthApiTest {
    private val base = "https://nimrag.example.com"
    private val hex = "cd".repeat(32)
    private val cookie = "__Host-nimrag_session=$hex; Path=/; Secure; HttpOnly; SameSite=Strict; Max-Age=2592000"

    private class Fake(private val answer: (HttpRequest) -> HttpResponse) : HttpTransport {
        val sent = mutableListOf<HttpRequest>()

        override fun send(request: HttpRequest): HttpResponse {
            sent += request
            return answer(request)
        }
    }

    private fun reply(status: Int, body: String = "{}", headers: Map<String, List<String>> = emptyMap()) =
        HttpResponse(status, headers.mapKeys { it.key.lowercase() }, body)

    private val loginBody = """{"me":{"authenticated":true,"username":"anna","role":"user","timezone":"Europe/Copenhagen","sample":false},"device_id":7,"token":"nrg_abc123","device_secret":"nrd_def456"}"""

    @Test
    fun loginSendsTheFieldsWithTheCsrfHeaderAndReadsTheGrant() {
        val http = Fake { reply(201, loginBody, mapOf("Set-Cookie" to listOf(cookie))) }
        val result = AuthApi(http).login(base, "anna", "pa\"ss", "123456", "Pixel 8a")
        val grant = (result as LoginResult.Success).grant
        assertEquals("anna", grant.username)
        assertEquals(7L, grant.deviceId)
        assertEquals("nrg_abc123", grant.token)
        assertEquals("nrd_def456", grant.deviceSecret)
        assertEquals("__Host-nimrag_session", grant.cookie.name)

        val sent = http.sent.single()
        assertEquals("POST", sent.method)
        assertEquals("$base/api/app/login", sent.url)
        assertEquals("1", sent.headers["X-Nimrag"])
        assertEquals("application/json", sent.headers["Content-Type"])
        val body = MiniJson.parseObject(sent.body!!)!!
        assertEquals(mapOf("username" to "anna", "password" to "pa\"ss", "code" to "123456", "device_name" to "Pixel 8a"), body)
    }

    @Test
    fun theGrantAndTheRequestNeverShowSecretsInToString() {
        val http = Fake { reply(201, loginBody, mapOf("Set-Cookie" to listOf(cookie))) }
        val result = AuthApi(http).login(base, "anna", "hunter2", "123456", "Pixel 8a")
        for (text in listOf(result.toString(), (result as LoginResult.Success).grant.toString(), http.sent.single().toString())) {
            for (secret in listOf("nrg_abc123", "nrd_def456", hex, "hunter2", "123456")) assertFalse(text.contains(secret), text)
        }
    }

    @Test
    fun loginFailuresMapPerStatus() {
        fun fail(status: Int, body: String = "{}", headers: Map<String, List<String>> = emptyMap()) =
            (AuthApi(Fake { reply(status, body, headers) }).login(base, "a", "p", "123456", "n") as LoginResult.Failure).failure
        assertEquals(AuthFailure.WrongCredentials, fail(401, """{"error":"invalid credentials"}"""))
        assertEquals(AuthFailure.AdminAccount, fail(403))
        assertEquals(AuthFailure.TemporaryPassword, fail(409, """{"error":"temporary password: change your password first"}"""))
        assertEquals(AuthFailure.TokenCap, fail(409, """{"error":"20 device tokens are active, revoke one first"}"""))
        assertEquals(AuthFailure.RateLimited(30), fail(429, headers = mapOf("Retry-After" to listOf("30"))))
        assertEquals(AuthFailure.Server, fail(500, "<html>oops</html>"))
    }

    @Test
    fun loginWithoutANetworkIsANetworkFailure() {
        val http = HttpTransport { throw IOException("down") }
        assertEquals(LoginResult.Failure(AuthFailure.Network), AuthApi(http).login(base, "a", "p", "123456", "n"))
    }

    @Test
    fun aLoginAnswerMissingAnythingIsAServerFailure() {
        fun login(body: String, cookies: List<String> = listOf(cookie)) =
            AuthApi(Fake { reply(201, body, mapOf("Set-Cookie" to cookies)) }).login(base, "a", "p", "123456", "n")
        assertEquals(LoginResult.Failure(AuthFailure.Server), login(loginBody, cookies = emptyList()))
        assertEquals(LoginResult.Failure(AuthFailure.Server), login("""{"device_id":7,"token":"nrg_x"}"""))
        assertEquals(LoginResult.Failure(AuthFailure.Server), login("""{"device_id":7,"token":"has space","device_secret":"nrd_y"}"""))
        assertEquals(LoginResult.Failure(AuthFailure.Server), login("not json"))
    }

    @Test
    fun resumeSendsOnlyTheSecret() {
        val http = Fake { reply(200, "{}", mapOf("Set-Cookie" to listOf(cookie))) }
        assertTrue(AuthApi(http).resume(base, "nrd_def456") is SessionResult.Success)
        val sent = http.sent.single()
        assertEquals("$base/api/app/resume", sent.url)
        assertEquals("1", sent.headers["X-Nimrag"])
        assertEquals(mapOf("device_secret" to "nrd_def456"), MiniJson.parseObject(sent.body!!))
    }

    @Test
    fun aRevokedSecretOnResumeIsReportedAsSuch() {
        val result = AuthApi(Fake { reply(401, """{"error":"unauthorized"}""") }).resume(base, "nrd_x")
        assertEquals(SessionResult.Failure(AuthFailure.SecretRevoked), result)
    }

    @Test
    fun unlockSendsPasswordAndSecretAndAWrongOneIsAmbiguous() {
        val http = Fake { reply(200, "{}", mapOf("Set-Cookie" to listOf(cookie))) }
        assertTrue(AuthApi(http).unlock(base, "anna", "pw", "nrd_x") is SessionResult.Success)
        val sent = http.sent.single()
        assertEquals("$base/api/app/unlock", sent.url)
        assertEquals(mapOf("username" to "anna", "password" to "pw", "device_secret" to "nrd_x"), MiniJson.parseObject(sent.body!!))

        val wrong = AuthApi(Fake { reply(401) }).unlock(base, "anna", "bad", "nrd_x")
        assertEquals(SessionResult.Failure(AuthFailure.WrongPasswordOrRemoved), wrong)
    }

    @Test
    fun anAnswerWithoutASessionCookieIsAServerFailure() {
        assertEquals(SessionResult.Failure(AuthFailure.Server), AuthApi(Fake { reply(200) }).resume(base, "nrd_x"))
    }

    @Test
    fun probeTellsAuthenticatedFromNot() {
        fun probe(status: Int, body: String = "{}", cookie: SessionCookie? = null) =
            AuthApi(Fake { reply(status, body) }).probe(base, cookie)
        assertEquals(Probe.Authenticated, probe(200, """{"authenticated":true,"username":"anna","role":"user"}"""))
        assertEquals(Probe.Unauthenticated, probe(401))
        // An admin session (a leftover web login) is not a way into the app.
        assertEquals(Probe.Unauthenticated, probe(200, """{"authenticated":true,"username":"root","role":"admin"}"""))
        assertEquals(Probe.Unauthenticated, probe(200, "garbage"))
        assertEquals(Probe.Unreachable, probe(502))
        assertEquals(Probe.Unreachable, probe(503))
    }

    @Test
    fun probeCarriesTheSessionCookieAndUsesItsOwnTransport() {
        val main = Fake { reply(500) }
        val probeOnly = Fake { reply(200, """{"authenticated":true,"role":"user"}""") }
        val api = AuthApi(main, probeOnly)
        val session = SessionCookie.fromJar("nimrag_session=$hex")
        assertEquals(Probe.Authenticated, api.probe(base, session))
        assertTrue(main.sent.isEmpty())
        val sent = probeOnly.sent.single()
        assertEquals("GET", sent.method)
        assertEquals("$base/api/me", sent.url)
        assertEquals("nimrag_session=$hex", sent.headers["Cookie"])
    }

    @Test
    fun probeWithNoNetworkIsUnreachable() {
        assertEquals(Probe.Unreachable, AuthApi(HttpTransport { throw IOException() }).probe(base, null))
    }

    @Test
    fun signOutRevokesThePhoneThenLogsOutWithTheSession() {
        val http = Fake { reply(204) }
        val session = SessionCookie.fromJar("nimrag_session=$hex")!!
        AuthApi(http).signOutOnServer(base, session, deviceId = 7)
        assertEquals(listOf("DELETE $base/api/tokens/7", "POST $base/api/logout"), http.sent.map { "${it.method} ${it.url}" })
        for (sent in http.sent) {
            assertEquals("1", sent.headers["X-Nimrag"])
            assertEquals("nimrag_session=$hex", sent.headers["Cookie"])
        }
    }

    @Test
    fun signOutWithoutADeviceIdOnlyLogsOutAndSurvivesNoNetwork() {
        val http = Fake { throw IOException("offline") }
        AuthApi(http).signOutOnServer(base, SessionCookie.fromJar("nimrag_session=$hex")!!, deviceId = null)
        assertEquals(listOf("POST $base/api/logout"), http.sent.map { "${it.method} ${it.url}" })
        // And with a device id the failed revoke does not stop the logout attempt.
        val both = Fake { throw IOException("offline") }
        AuthApi(both).signOutOnServer(base, SessionCookie.fromJar("nimrag_session=$hex")!!, deviceId = 3)
        assertEquals(2, both.sent.size)
    }
}
