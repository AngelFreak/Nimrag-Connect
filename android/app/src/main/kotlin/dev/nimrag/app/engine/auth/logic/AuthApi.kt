package dev.nimrag.app.engine.auth.logic

import dev.nimrag.app.engine.logic.UploadPolicy
import java.io.IOException

/** One HTTP exchange; implemented over `HttpURLConnection` ([HttpUrlTransport]) and by fakes in tests. */
fun interface HttpTransport {
    /** @throws IOException when no answer came. */
    fun send(request: HttpRequest): HttpResponse
}

class HttpRequest(
    val method: String,
    val url: String,
    val headers: Map<String, String> = emptyMap(),
    val body: String? = null,
) {
    /** Never shows the headers or the body (cookies, passwords, secrets). */
    override fun toString() = "HttpRequest($method)"
}

class HttpResponse(
    val status: Int,
    /** Header names lower-cased. */
    val headers: Map<String, List<String>>,
    val body: String,
) {
    fun header(name: String): String? = headers[name.lowercase()]?.firstOrNull()

    override fun toString() = "HttpResponse($status)"
}

/** What a successful first sign-in hands over. Shown once by the server; never printed. */
class LoginGrant(
    val username: String,
    val deviceId: Long,
    /** The upload credential. */
    val token: String,
    /** The trusted-device credential (`nrd_...`). */
    val deviceSecret: String,
    val cookie: SessionCookie,
) {
    override fun toString() = "LoginGrant(device=$deviceId)"
}

sealed interface LoginResult {
    class Success(val grant: LoginGrant) : LoginResult {
        override fun toString() = "LoginResult.Success"
    }

    data class Failure(val failure: AuthFailure) : LoginResult
}

sealed interface SessionResult {
    class Success(val cookie: SessionCookie) : SessionResult {
        override fun toString() = "SessionResult.Success"
    }

    data class Failure(val failure: AuthFailure) : SessionResult
}

/** The app API of the server (`server/README.md`, "The Android app"). Blocking: call off the main thread. */
class AuthApi(private val http: HttpTransport, private val probeHttp: HttpTransport = http) {
    /** `POST /api/app/login`: the first time on this phone. */
    fun login(base: String, username: String, password: String, code: String, deviceName: String): LoginResult {
        val body = MiniJson.obj("username" to username, "password" to password, "code" to code, "device_name" to deviceName)
        val reply = try {
            http.send(post("$base/api/app/login", body))
        } catch (_: IOException) {
            return LoginResult.Failure(AuthFailure.Network)
        }
        if (reply.status != 201) return LoginResult.Failure(failureOf(AuthCall.Login, reply))
        val json = MiniJson.parseObject(reply.body) ?: return LoginResult.Failure(AuthFailure.Server)
        val token = json["token"] as? String
        val secret = json["device_secret"] as? String
        val deviceId = (json["device_id"] as? Double)?.toLong()
        val name = ((json["me"] as? Map<*, *>)?.get("username") as? String) ?: username
        val cookie = SessionCookie.fromSetCookie(reply.headers["set-cookie"].orEmpty())
        if (token == null || !UploadPolicy.isValidToken(token) || secret == null || !UploadPolicy.isValidToken(secret) ||
            deviceId == null || cookie == null
        ) {
            return LoginResult.Failure(AuthFailure.Server)
        }
        return LoginResult.Success(LoginGrant(name, deviceId, token, secret, cookie))
    }

    /** `POST /api/app/unlock`: password plus this phone's secret. */
    fun unlock(base: String, username: String, password: String, deviceSecret: String): SessionResult =
        session(
            AuthCall.Unlock,
            post("$base/api/app/unlock", MiniJson.obj("username" to username, "password" to password, "device_secret" to deviceSecret)),
        )

    /** `POST /api/app/resume`: the fingerprint path (the secret alone). */
    fun resume(base: String, deviceSecret: String): SessionResult =
        session(AuthCall.Resume, post("$base/api/app/resume", MiniJson.obj("device_secret" to deviceSecret)))

    /** `GET /api/me` with the session in the cookie jar (or none): who is signed in. */
    fun probe(base: String, cookie: SessionCookie?): Probe {
        val headers = buildMap {
            put("Accept", "application/json")
            if (cookie != null) put("Cookie", cookie.header())
        }
        val reply = try {
            probeHttp.send(HttpRequest("GET", "$base/api/me", headers))
        } catch (_: IOException) {
            return Probe.Unreachable
        }
        return when (reply.status) {
            401 -> Probe.Unauthenticated
            200 -> {
                val json = MiniJson.parseObject(reply.body)
                // An admin session has no data and no place in the app (a leftover web login).
                if (json?.get("authenticated") == true && json["role"] == "user") Probe.Authenticated else Probe.Unauthenticated
            }
            else -> Probe.Unreachable
        }
    }

    /** `DELETE /api/tokens/{id}` then `POST /api/logout`, both best effort, with the live session. */
    fun signOutOnServer(base: String, cookie: SessionCookie, deviceId: Long?) {
        val headers = mapOf("X-Nimrag" to "1", "Cookie" to cookie.header())
        if (deviceId != null) {
            try {
                http.send(HttpRequest("DELETE", "$base/api/tokens/$deviceId", headers))
            } catch (_: IOException) {
                // Offline: the token stays valid on the server until it is revoked in its Settings.
            }
        }
        try {
            http.send(HttpRequest("POST", "$base/api/logout", headers))
        } catch (_: IOException) {
            // Same: the session ends by itself after its 30 days.
        }
    }

    private fun session(call: AuthCall, request: HttpRequest): SessionResult {
        val reply = try {
            http.send(request)
        } catch (_: IOException) {
            return SessionResult.Failure(AuthFailure.Network)
        }
        if (reply.status != 200) return SessionResult.Failure(failureOf(call, reply))
        val cookie = SessionCookie.fromSetCookie(reply.headers["set-cookie"].orEmpty())
            ?: return SessionResult.Failure(AuthFailure.Server)
        return SessionResult.Success(cookie)
    }

    private fun post(url: String, json: String) = HttpRequest(
        "POST",
        url,
        mapOf("Content-Type" to "application/json", "Accept" to "application/json", "X-Nimrag" to "1"),
        json,
    )

    private fun failureOf(call: AuthCall, reply: HttpResponse): AuthFailure {
        val error = MiniJson.parseObject(reply.body)?.get("error") as? String
        return AuthErrors.map(call, reply.status, reply.header("retry-after"), error)
    }
}
