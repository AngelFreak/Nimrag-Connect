package dev.nimrag.app.model

import android.app.Activity
import dev.nimrag.app.engine.auth.logic.AuthFailure
import dev.nimrag.app.engine.auth.logic.AuthPhase
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow

/*
 * The app's sign-in with the server's accounts (IMPLEMENTATION_PLAN.md, "Multi-user", stage 3p).
 * The UI reads [AccountState] and calls [AccountController]; secrets (the password, the code, the
 * token, the device secret) never enter the state.
 */

data class AccountState(
    val phase: AuthPhase = AuthPhase.Starting,
    /** The signed-in (or locked) account's username, for display; null when signed out. */
    val username: String? = null,
    /** A call to the server is running. */
    val busy: Boolean = false,
    /** Why the last sign-in or unlock did not work; cleared by the next attempt. */
    val failure: AuthFailure? = null,
    /** A fingerprint unlock is set up and its key is still valid. */
    val biometricReady: Boolean = false,
    /** Bumped each time a new session cookie went into the web view's cookie jar. */
    val sessionEpoch: Int = 0,
)

interface AccountController {
    val state: StateFlow<AccountState>

    /** First sign-in: server address, username, password and the 6-digit code. */
    fun signIn(server: String, username: String, password: String, code: String)

    /** Locked: the password plus this phone's secret (no code). */
    fun unlockWithPassword(password: String)

    /** Locked: the fingerprint releases the secret, which starts the session. [activity] hosts the system prompt. */
    fun unlockWithFingerprint(activity: Activity)

    /** Ends the session on the server, then clears the cookie and every credential on this phone. */
    fun signOut()

    /** The web app (or a check) found the session gone. */
    fun sessionExpired()

    /** Asks the server whether the session is still good (launch, coming back to the app). */
    fun checkSession()

    /** Forgets the last failure. */
    fun clearFailure()
}

/** For previews and a UI-only build: a fixed state, no server. */
class FixedAccountController(initial: AccountState = AccountState(phase = AuthPhase.SignedIn, username = "anna")) : AccountController {
    override val state: StateFlow<AccountState> = MutableStateFlow(initial)

    override fun signIn(server: String, username: String, password: String, code: String) = Unit

    override fun unlockWithPassword(password: String) = Unit

    override fun unlockWithFingerprint(activity: Activity) = Unit

    override fun signOut() = Unit

    override fun sessionExpired() = Unit

    override fun checkSession() = Unit

    override fun clearFailure() = Unit
}
