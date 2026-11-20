package dev.nimrag.app.ui.shell.logic

import dev.nimrag.app.engine.auth.logic.AuthPhase
import dev.nimrag.app.model.WebMode

/** What the web area of the shell shows. */
enum class WebSource {
    /** The real server's web app (needs a live session). */
    Remote,

    /** The phone's own copy (the local server, personal flavor): signed in by the app itself, no account screens. */
    Local,

    /** The native sign-in screen: server address, username, password, code. */
    SignIn,

    /** The native unlock screen: fingerprint or password. */
    Unlock,

    /** The first check of the session is still running: nothing to show yet. */
    Checking,
}

/**
 * Pure rule for the web view's source (no Android types).
 *
 * The real server is the source of truth (history, edits, desktop parity), so it wins whenever
 * it is reachable and has everything. The phone's own copy (the local server) is used when it is
 * the better or the only choice: no network, the server failed to answer this session, or
 * uploads are still pending (the phone holds data the server does not have yet). The choice is
 * made when the web view is (re)created, never while a page is open.
 *
 * Showing the server also needs an account: with no live session the answer is the native
 * sign-in screen (signed out) or unlock screen (locked), never the web app's own login page. The
 * phone's own copy needs none of that.
 */
object WebSourcePolicy {
    fun choose(
        mode: WebMode,
        localReady: Boolean,
        remoteConfigured: Boolean,
        online: Boolean,
        remoteFailed: Boolean,
        pendingUploads: Int,
        auth: AuthPhase = AuthPhase.SignedIn,
    ): WebSource {
        val wanted = wanted(mode, localReady, remoteConfigured, online, remoteFailed, pendingUploads)
        if (wanted == WebSource.Local) return WebSource.Local
        return remoteFor(auth)
    }

    /** The server's web app, or the account screen that has to come first. */
    fun remoteFor(auth: AuthPhase): WebSource = when (auth) {
        AuthPhase.SignedIn -> WebSource.Remote
        AuthPhase.SignedOut -> WebSource.SignIn
        AuthPhase.Locked -> WebSource.Unlock
        AuthPhase.Starting -> WebSource.Checking
    }

    private fun wanted(
        mode: WebMode,
        localReady: Boolean,
        remoteConfigured: Boolean,
        online: Boolean,
        remoteFailed: Boolean,
        pendingUploads: Int,
    ): WebSource {
        if (!localReady) return WebSource.Remote
        if (!remoteConfigured) return WebSource.Local
        return when (mode) {
            WebMode.Local -> WebSource.Local
            WebMode.Remote -> WebSource.Remote
            WebMode.Auto -> if (!online || remoteFailed || pendingUploads > 0) WebSource.Local else WebSource.Remote
        }
    }
}
