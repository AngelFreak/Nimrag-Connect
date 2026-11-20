package dev.nimrag.app.engine.auth.logic

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

class AuthErrorsTest {
    private fun map(call: AuthCall, status: Int, retryAfter: String? = null, error: String? = null) =
        AuthErrors.map(call, status, retryAfter, error)

    @Test
    fun aWrongFirstSignInIsOneAnswerWhateverWasWrong() {
        // The server says 401 for a wrong username, password or code; the app must not guess which.
        assertEquals(AuthFailure.WrongCredentials, map(AuthCall.Login, 401, error = "wrong code"))
        assertEquals(AuthFailure.WrongCredentials, map(AuthCall.Login, 401, error = "unknown user"))
    }

    @Test
    fun theSame401MeansDifferentThingsPerCall() {
        assertEquals(AuthFailure.WrongPasswordOrRemoved, map(AuthCall.Unlock, 401))
        assertEquals(AuthFailure.SecretRevoked, map(AuthCall.Resume, 401))
    }

    @Test
    fun anAdminAccountIsRefusedOnlyAtSignIn() {
        assertEquals(AuthFailure.AdminAccount, map(AuthCall.Login, 403))
        assertEquals(AuthFailure.Server, map(AuthCall.Unlock, 403))
        assertEquals(AuthFailure.Server, map(AuthCall.Resume, 403))
    }

    @Test
    fun aConflictIsATemporaryPasswordOrTheTokenCap() {
        assertEquals(AuthFailure.TemporaryPassword, map(AuthCall.Login, 409, error = "You have a temporary password: change your password in the web app first"))
        assertEquals(AuthFailure.TokenCap, map(AuthCall.Login, 409, error = "20 device tokens are active; revoke one first"))
        assertEquals(AuthFailure.Server, map(AuthCall.Login, 409, error = "something else"))
        assertEquals(AuthFailure.Server, map(AuthCall.Login, 409))
    }

    @Test
    fun tooManyAttemptsCarriesTheWait() {
        assertEquals(AuthFailure.RateLimited(37), map(AuthCall.Login, 429, retryAfter = "37"))
        assertEquals(AuthFailure.RateLimited(null), map(AuthCall.Login, 429))
        assertEquals(AuthFailure.RateLimited(null), map(AuthCall.Unlock, 429, retryAfter = "soon"))
    }

    @Test
    fun retryAfterIsClamped() {
        assertEquals(1, AuthErrors.parseRetryAfter("0"))
        assertEquals(1, AuthErrors.parseRetryAfter("-5"))
        assertEquals(AuthErrors.MAX_RETRY_AFTER_SECONDS, AuthErrors.parseRetryAfter("999999999"))
        assertEquals(12, AuthErrors.parseRetryAfter(" 12 "))
        assertNull(AuthErrors.parseRetryAfter(null))
        assertNull(AuthErrors.parseRetryAfter("Wed, 21 Oct 2026 07:28:00 GMT"))
    }

    @Test
    fun everythingElseIsAServerProblem() {
        for (status in listOf(400, 404, 500, 502, 503)) assertEquals(AuthFailure.Server, map(AuthCall.Login, status))
    }
}

class AuthMachineTest {
    private fun next(phase: AuthPhase, event: AuthMachine.Event, credentials: Boolean = false, server: Boolean = true) =
        AuthMachine.next(phase, event, credentials, server)

    @Test
    fun aLiveSessionIsSignedIn() {
        assertEquals(AuthPhase.SignedIn, next(AuthPhase.Starting, AuthMachine.Event.Probed(Probe.Authenticated)))
    }

    @Test
    fun noSessionButATrustedPhoneIsLocked() {
        assertEquals(AuthPhase.Locked, next(AuthPhase.Starting, AuthMachine.Event.Probed(Probe.Unauthenticated), credentials = true))
        assertEquals(AuthPhase.SignedOut, next(AuthPhase.Starting, AuthMachine.Event.Probed(Probe.Unauthenticated), credentials = false))
    }

    @Test
    fun offlineAtLaunchKeepsWorkingWhenThereIsAServer() {
        assertEquals(AuthPhase.SignedIn, next(AuthPhase.Starting, AuthMachine.Event.Probed(Probe.Unreachable), server = true))
        assertEquals(AuthPhase.SignedOut, next(AuthPhase.Starting, AuthMachine.Event.Probed(Probe.Unreachable), server = false))
    }

    @Test
    fun noAnswerLaterNeverChangesThePhase() {
        for (phase in listOf(AuthPhase.SignedIn, AuthPhase.Locked, AuthPhase.SignedOut)) {
            assertEquals(phase, next(phase, AuthMachine.Event.Probed(Probe.Unreachable), credentials = true))
        }
    }

    @Test
    fun anExpiredSessionLocksATrustedPhoneAndSignsOutAnyOther() {
        assertEquals(AuthPhase.Locked, next(AuthPhase.SignedIn, AuthMachine.Event.SessionExpired, credentials = true))
        assertEquals(AuthPhase.SignedOut, next(AuthPhase.SignedIn, AuthMachine.Event.SessionExpired, credentials = false))
        assertEquals(AuthPhase.Locked, next(AuthPhase.Locked, AuthMachine.Event.SessionExpired, credentials = true))
        assertEquals(AuthPhase.SignedOut, next(AuthPhase.SignedOut, AuthMachine.Event.SessionExpired, credentials = true))
    }

    @Test
    fun signingInOrUnlockingEndsSignedIn() {
        assertEquals(AuthPhase.SignedIn, next(AuthPhase.SignedOut, AuthMachine.Event.SignedIn))
        assertEquals(AuthPhase.SignedIn, next(AuthPhase.Locked, AuthMachine.Event.Unlocked))
    }

    @Test
    fun aRejectedSecretOrASignOutEndsSignedOut() {
        for (phase in AuthPhase.entries) {
            assertEquals(AuthPhase.SignedOut, next(phase, AuthMachine.Event.SecretRejected, credentials = true))
            assertEquals(AuthPhase.SignedOut, next(phase, AuthMachine.Event.SignedOut, credentials = true))
        }
    }
}

class SessionCookieTest {
    private val hex = "ab".repeat(32)

    @Test
    fun readsTheInsecureAndTheHostCookie() {
        val plain = SessionCookie.fromSetCookie(listOf("nimrag_session=$hex; Path=/; Max-Age=2592000; HttpOnly; SameSite=Strict"))!!
        assertEquals("nimrag_session", plain.name)
        assertEquals(2_592_000L, plain.maxAgeSeconds)
        val host = SessionCookie.fromSetCookie(listOf("__Host-nimrag_session=$hex; Path=/; Secure; HttpOnly; SameSite=Strict"))!!
        assertEquals("__Host-nimrag_session", host.name)
        assertEquals(SessionCookie.DEFAULT_MAX_AGE_SECONDS, host.maxAgeSeconds)
    }

    @Test
    fun picksTheSessionCookieAmongOthers() {
        val cookie = SessionCookie.fromSetCookie(listOf("theme=dark; Path=/", "nimrag_session=$hex; Max-Age=60"))!!
        assertEquals("$hex", cookie.value)
        assertEquals(60L, cookie.maxAgeSeconds)
    }

    @Test
    fun refusesAnythingThatIsNotTheServersCookie() {
        assertNull(SessionCookie.fromSetCookie(emptyList()))
        assertNull(SessionCookie.fromSetCookie(listOf("other=$hex")))
        assertNull(SessionCookie.fromSetCookie(listOf("nimrag_session=short")))
        assertNull(SessionCookie.fromSetCookie(listOf("nimrag_session=${"AB".repeat(32)}")))
        assertNull(SessionCookie.fromSetCookie(listOf("nimrag_session=${"zz".repeat(32)}")))
        // The server's logout cookie (empty value) is not a session.
        assertNull(SessionCookie.fromSetCookie(listOf("nimrag_session=; Max-Age=0")))
    }

    @Test
    fun aNonPositiveLifetimeFallsBackToTheDefault() {
        assertEquals(SessionCookie.DEFAULT_MAX_AGE_SECONDS, SessionCookie.fromSetCookie(listOf("nimrag_session=$hex; Max-Age=0"))!!.maxAgeSeconds)
        assertEquals(SessionCookie.DEFAULT_MAX_AGE_SECONDS, SessionCookie.fromSetCookie(listOf("nimrag_session=$hex; Max-Age=abc"))!!.maxAgeSeconds)
    }

    @Test
    fun theCookieIsRebuiltWithOurOwnAttributes() {
        // Whatever Domain or Path the server wrote, only these attributes go into the jar.
        val cookie = SessionCookie.fromSetCookie(listOf("nimrag_session=$hex; Domain=evil.example; Path=/x; Max-Age=100"))!!
        assertEquals("nimrag_session=$hex; Path=/; Max-Age=100; HttpOnly; SameSite=Strict", cookie.toSetCookie())
        val host = SessionCookie.fromSetCookie(listOf("__Host-nimrag_session=$hex; Max-Age=100"))!!
        assertEquals("__Host-nimrag_session=$hex; Path=/; Max-Age=100; HttpOnly; SameSite=Strict; Secure", host.toSetCookie())
        assertEquals("nimrag_session=$hex", cookie.header())
    }

    @Test
    fun theJarLineYieldsTheSession() {
        assertEquals(hex, SessionCookie.fromJar("a=b; nimrag_session=$hex; c=d")!!.value)
        assertEquals(hex, SessionCookie.fromJar("__Host-nimrag_session=$hex")!!.value)
        assertNull(SessionCookie.fromJar(null))
        assertNull(SessionCookie.fromJar(""))
        assertNull(SessionCookie.fromJar("a=b"))
        assertNull(SessionCookie.fromJar("nimrag_session=nothex"))
    }

    @Test
    fun theValueNeverShowsInToString() {
        assertFalse(SessionCookie.fromJar("nimrag_session=$hex")!!.toString().contains(hex))
    }
}

class LoginFormTest {
    @Test
    fun aGoodFormIsNormalised() {
        val ok = LoginForm.check(" https://nimrag.example.com/ ", "  anna ", "pw", "123 456") as LoginForm.Checked.Ok
        assertEquals("https://nimrag.example.com", ok.base)
        assertEquals("anna", ok.username)
        assertEquals("123456", ok.code)
    }

    @Test
    fun aBadAddressIsNamed() {
        assertEquals(LoginForm.Checked.Bad(AuthFailure.BadAddress), LoginForm.check("", "a", "p", "123456"))
        assertEquals(LoginForm.Checked.Bad(AuthFailure.BadAddress), LoginForm.check("nimrag.example.com", "a", "p", "123456"))
        assertEquals(LoginForm.Checked.Bad(AuthFailure.BadAddress), LoginForm.check("ftp://x", "a", "p", "123456"))
    }

    @Test
    fun emptyFieldsAndAShortCodeAreIncomplete() {
        val s = "https://x.example"
        assertEquals(LoginForm.Checked.Bad(AuthFailure.Incomplete), LoginForm.check(s, " ", "p", "123456"))
        assertEquals(LoginForm.Checked.Bad(AuthFailure.Incomplete), LoginForm.check(s, "a", "", "123456"))
        assertEquals(LoginForm.Checked.Bad(AuthFailure.Incomplete), LoginForm.check(s, "a", "p", "12345"))
        assertEquals(LoginForm.Checked.Bad(AuthFailure.Incomplete), LoginForm.check(s, "a", "p", "1234567"))
        assertEquals(LoginForm.Checked.Bad(AuthFailure.Incomplete), LoginForm.check(s, "a", "p", "12a456"))
    }

    @Test
    fun aPasswordKeepsItsSpaces() {
        // Only the username is trimmed: a password may begin or end with a space.
        assertTrue(LoginForm.check("https://x.example", "a", " p ", "123456") is LoginForm.Checked.Ok)
    }

    @Test
    fun theCodeFieldKeepsSixDigits() {
        assertEquals("123456", LoginForm.filterCode("12 34-56789"))
        assertEquals("", LoginForm.filterCode("abc"))
    }

    @Test
    fun theFormNeverShowsAnythingSecretInToString() {
        val ok = LoginForm.check("https://x.example", "anna", "hunter2", "123456") as LoginForm.Checked.Ok
        assertFalse(ok.toString().contains("123456"))
        assertFalse(ok.toString().contains("anna"))
    }
}

class DeviceNameTest {
    @Test
    fun usesTheModel() {
        assertEquals("Pixel 8a", DeviceName.of("Pixel 8a"))
    }

    @Test
    fun neverBlankNeverTooLong() {
        assertEquals(DeviceName.FALLBACK, DeviceName.of(null))
        assertEquals(DeviceName.FALLBACK, DeviceName.of("   "))
        assertEquals(DeviceName.FALLBACK, DeviceName.of("\u0000\n"))
        assertEquals(64, DeviceName.of("x".repeat(200)).length)
    }

    @Test
    fun dropsControlCharacters() {
        assertEquals("Pixel 8a", DeviceName.of("Pixel 8a\u0007"))
    }
}
