package dev.nimrag.app.engine.auth.logic

import dev.nimrag.app.engine.logic.UploadPolicy

/*
 * The pure rules of signing in to the app (IMPLEMENTATION_PLAN.md, "Multi-user", stage 3p): the
 * state machine signed out / signed in / locked, what the server's answers mean, the session
 * cookie, and the sign-in form. No Android types: the unit tests run on the JVM. Nothing in
 * here ever puts a password, code, token or device secret into a message or `toString`.
 */

/** Where the app stands with the server. */
enum class AuthPhase {
    /** Launch: the first check of the session has not answered yet. */
    Starting,

    /** No usable account on this phone: the sign-in screen (server, username, password, code). */
    SignedOut,

    /** A live session: the web app is shown. */
    SignedIn,

    /** The session ended but this phone is trusted: unlock with the fingerprint or the password. */
    Locked,
}

/** Why a sign-in or unlock did not work; the screens turn each into one fixed sentence. */
sealed interface AuthFailure {
    /** 401 on the first sign-in: the same answer for a wrong username, password or code. */
    data object WrongCredentials : AuthFailure

    /** 401 on a password unlock: wrong password, or the server no longer knows this phone. */
    data object WrongPasswordOrRemoved : AuthFailure

    /** 401 on the fingerprint path: the server no longer knows this phone's secret. Credentials are wiped. */
    data object SecretRevoked : AuthFailure

    /** 403: an admin account has no data and cannot use the app. */
    data object AdminAccount : AuthFailure

    /** 409: the account still has a temporary password; finish setting it up in a browser. */
    data object TemporaryPassword : AuthFailure

    /** 409: the account already has its maximum of device tokens. */
    data object TokenCap : AuthFailure

    /** 429: wait [retryAfterSeconds] (null when the server did not say). */
    data class RateLimited(val retryAfterSeconds: Int?) : AuthFailure

    /** The server address is blank or not an http(s) address. */
    data object BadAddress : AuthFailure

    /** A field is empty, or the code is not six digits. */
    data object Incomplete : AuthFailure

    /** No answer: offline, DNS, TLS, timeout. */
    data object Network : AuthFailure

    /** A 5xx or an answer the app does not understand. */
    data object Server : AuthFailure

    /** The fingerprint check failed or was refused (a cancel is not a failure). */
    data object BiometricFailed : AuthFailure

    /** No secret is stored on this phone any more: sign in again. */
    data object NoCredentials : AuthFailure

    /** An upload came back 401 or 403: the phone's token was revoked or never valid. Credentials are wiped. */
    data object TokenRejected : AuthFailure
}

/** Which app call an answer belongs to (the same 401 means different things). */
enum class AuthCall { Login, Unlock, Resume }

object AuthErrors {
    const val MAX_RETRY_AFTER_SECONDS = 86_400

    /**
     * Status code, `Retry-After` header and the server's error text to a failure. Only the status
     * decides, except for 409 (two reasons) where the text picks between the two documented
     * ones. The server's text is never passed on, and a 401 is the same for every wrong input.
     */
    fun map(call: AuthCall, status: Int, retryAfter: String?, serverError: String?): AuthFailure = when (status) {
        401 -> when (call) {
            AuthCall.Login -> AuthFailure.WrongCredentials
            AuthCall.Unlock -> AuthFailure.WrongPasswordOrRemoved
            AuthCall.Resume -> AuthFailure.SecretRevoked
        }
        403 -> if (call == AuthCall.Login) AuthFailure.AdminAccount else AuthFailure.Server
        409 -> {
            val text = serverError.orEmpty().lowercase()
            when {
                "temporary password" in text || "change your" in text -> AuthFailure.TemporaryPassword
                "device token" in text || "revoke one" in text -> AuthFailure.TokenCap
                else -> AuthFailure.Server
            }
        }
        429 -> AuthFailure.RateLimited(parseRetryAfter(retryAfter))
        else -> AuthFailure.Server
    }

    /** `Retry-After` in whole seconds (the server sends seconds); null when absent or not a number. */
    fun parseRetryAfter(value: String?): Int? {
        val n = value?.trim()?.toLongOrNull() ?: return null
        return n.coerceIn(1, MAX_RETRY_AFTER_SECONDS.toLong()).toInt()
    }
}

/** What the first check of the session found. */
enum class Probe { Authenticated, Unauthenticated, Unreachable }

object AuthMachine {
    sealed interface Event {
        /** `GET /api/me` answered (or did not) at launch or when the app came back. */
        data class Probed(val result: Probe) : Event

        /** The web app got a 401 (or the cheap check said so). */
        data object SessionExpired : Event

        /** First sign-in succeeded. */
        data object SignedIn : Event

        /** Unlock by fingerprint or password succeeded. */
        data object Unlocked : Event

        /** The server said it does not know this phone's secret. */
        data object SecretRejected : Event

        /** Sign out, or the local credentials were wiped. */
        data object SignedOut : Event
    }

    /**
     * The next phase. [hasCredentials]: a trusted-device secret is stored on this phone.
     * [hasServer]: a usable server address is saved.
     */
    fun next(phase: AuthPhase, event: Event, hasCredentials: Boolean, hasServer: Boolean): AuthPhase = when (event) {
        is Event.Probed -> when (event.result) {
            Probe.Authenticated -> AuthPhase.SignedIn
            Probe.Unauthenticated -> if (hasCredentials) AuthPhase.Locked else AuthPhase.SignedOut
            // Nothing could be asked. At launch keep working offline (cached pages, the phone's own
            // copy) when there is a server to come back to; later the phase simply stays.
            Probe.Unreachable -> if (phase == AuthPhase.Starting) {
                if (hasServer) AuthPhase.SignedIn else AuthPhase.SignedOut
            } else {
                phase
            }
        }
        Event.SessionExpired -> when (phase) {
            AuthPhase.SignedIn, AuthPhase.Starting -> if (hasCredentials) AuthPhase.Locked else AuthPhase.SignedOut
            AuthPhase.Locked, AuthPhase.SignedOut -> phase
        }
        Event.SignedIn, Event.Unlocked -> AuthPhase.SignedIn
        Event.SecretRejected, Event.SignedOut -> AuthPhase.SignedOut
    }
}

/**
 * The session cookie of the server, rebuilt from the `Set-Cookie` the server sent. Only the two
 * known names and a 64-character hex value are accepted, and the attributes are our own (the
 * server's `Domain`, `Path` or anything else is never copied).
 */
class SessionCookie(val name: String, val value: String, val maxAgeSeconds: Long) {
    /** What `CookieManager.setCookie(url, ...)` takes. `__Host-` cookies are `Secure` by definition. */
    fun toSetCookie(): String {
        val secure = name.startsWith("__Host-")
        return "$name=$value; Path=/; Max-Age=$maxAgeSeconds; HttpOnly; SameSite=Strict" + if (secure) "; Secure" else ""
    }

    /** The `Cookie` request header for a call that carries this session. */
    fun header(): String = "$name=$value"

    override fun toString() = "SessionCookie($name)"

    companion object {
        const val NAME_SECURE = "__Host-nimrag_session"
        const val NAME_INSECURE = "nimrag_session"
        const val DEFAULT_MAX_AGE_SECONDS = 30L * 24 * 3600
        private val HEX64 = Regex("^[0-9a-f]{64}$")

        /** The session cookie among the `Set-Cookie` values of one answer, or null. */
        fun fromSetCookie(headers: List<String>): SessionCookie? {
            for (raw in headers) {
                val parts = raw.split(';').map { it.trim() }
                val (name, value) = parts.firstOrNull()?.split('=', limit = 2)?.takeIf { it.size == 2 } ?: continue
                if (name != NAME_SECURE && name != NAME_INSECURE) continue
                if (!HEX64.matches(value)) continue
                val maxAge = parts.drop(1)
                    .firstOrNull { it.startsWith("Max-Age=", ignoreCase = true) }
                    ?.substringAfter('=')?.toLongOrNull()
                    ?.takeIf { it > 0 }
                return SessionCookie(name, value, maxAge ?: DEFAULT_MAX_AGE_SECONDS)
            }
            return null
        }

        /** The session cookie in a cookie jar line (`a=b; c=d`, as `CookieManager.getCookie` gives it), or null. */
        fun fromJar(jar: String?): SessionCookie? {
            if (jar.isNullOrBlank()) return null
            for (pair in jar.split(';')) {
                val (name, value) = pair.trim().split('=', limit = 2).takeIf { it.size == 2 } ?: continue
                if ((name == NAME_SECURE || name == NAME_INSECURE) && HEX64.matches(value)) {
                    return SessionCookie(name, value, DEFAULT_MAX_AGE_SECONDS)
                }
            }
            return null
        }
    }
}

/** The sign-in form's checks, before anything is sent. */
object LoginForm {
    const val CODE_LENGTH = 6

    sealed interface Checked {
        /** [base] is the normalised server address without a trailing slash. */
        class Ok(val base: String, val username: String, val code: String) : Checked {
            override fun toString() = "LoginForm.Ok($base)"
        }

        data class Bad(val failure: AuthFailure) : Checked
    }

    fun check(server: String, username: String, password: String, code: String): Checked {
        val base = UploadPolicy.normalizeServerUrl(server) ?: return Checked.Bad(AuthFailure.BadAddress)
        val name = username.trim()
        val digits = code.filter { !it.isWhitespace() }
        if (name.isEmpty() || password.isEmpty()) return Checked.Bad(AuthFailure.Incomplete)
        if (digits.length != CODE_LENGTH || !digits.all { it in '0'..'9' }) return Checked.Bad(AuthFailure.Incomplete)
        return Checked.Ok(base, name, digits)
    }

    /** What the code field keeps while the user types: digits only, at most six. */
    fun filterCode(input: String): String = input.filter { it in '0'..'9' }.take(CODE_LENGTH)
}

object DeviceName {
    const val MAX = 64
    const val FALLBACK = "Android phone"

    /** "Pixel 8a" from the model; control characters out, 1 to 64 characters, never blank. */
    fun of(model: String?): String {
        val clean = model.orEmpty().filter { !it.isISOControl() }.trim()
        if (clean.isEmpty()) return FALLBACK
        return if (clean.length <= MAX) clean else clean.take(MAX).trimEnd()
    }
}
