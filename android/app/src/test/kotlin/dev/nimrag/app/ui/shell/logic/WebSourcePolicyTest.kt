package dev.nimrag.app.ui.shell.logic

import dev.nimrag.app.engine.auth.logic.AuthPhase
import dev.nimrag.app.model.WebMode
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Test

class WebSourcePolicyTest {
    private fun choose(
        mode: WebMode = WebMode.Auto,
        localReady: Boolean = true,
        remoteConfigured: Boolean = true,
        online: Boolean = true,
        remoteFailed: Boolean = false,
        pending: Int = 0,
        auth: AuthPhase = AuthPhase.SignedIn,
    ) = WebSourcePolicy.choose(mode, localReady, remoteConfigured, online, remoteFailed, pending, auth)

    @Test
    fun publicBuildAlwaysUsesTheServer() {
        assertEquals(WebSource.Remote, choose(localReady = false, online = false))
        assertEquals(WebSource.Remote, choose(localReady = false, mode = WebMode.Local))
    }

    @Test
    fun noServerAddressUsesThePhone() {
        assertEquals(WebSource.Local, choose(remoteConfigured = false))
    }

    @Test
    fun autoPrefersTheServerWhenEverythingIsFine() {
        assertEquals(WebSource.Remote, choose())
    }

    @Test
    fun autoFallsBackToThePhone() {
        assertEquals(WebSource.Local, choose(online = false))
        assertEquals(WebSource.Local, choose(remoteFailed = true))
        assertEquals(WebSource.Local, choose(pending = 3))
    }

    @Test
    fun theServerNeedsASession() {
        assertEquals(WebSource.SignIn, choose(auth = AuthPhase.SignedOut))
        assertEquals(WebSource.Unlock, choose(auth = AuthPhase.Locked))
        assertEquals(WebSource.Checking, choose(auth = AuthPhase.Starting))
        // Also in a public build and with no server address saved: the sign-in asks for it.
        assertEquals(WebSource.SignIn, choose(localReady = false, remoteConfigured = false, auth = AuthPhase.SignedOut))
    }

    @Test
    fun thePhonesOwnCopyNeedsNoAccount() {
        for (phase in AuthPhase.entries) {
            assertEquals(WebSource.Local, choose(online = false, auth = phase), phase.name)
            assertEquals(WebSource.Local, choose(remoteConfigured = false, auth = phase), phase.name)
            assertEquals(WebSource.Local, choose(mode = WebMode.Local, auth = phase), phase.name)
        }
    }

    @Test
    fun aChosenServerStillNeedsASession() {
        assertEquals(WebSource.SignIn, choose(mode = WebMode.Remote, online = false, auth = AuthPhase.SignedOut))
        assertEquals(WebSource.Unlock, choose(mode = WebMode.Remote, auth = AuthPhase.Locked))
    }

    @Test
    fun remoteForMapsEachPhase() {
        assertEquals(WebSource.Remote, WebSourcePolicy.remoteFor(AuthPhase.SignedIn))
        assertEquals(WebSource.SignIn, WebSourcePolicy.remoteFor(AuthPhase.SignedOut))
        assertEquals(WebSource.Unlock, WebSourcePolicy.remoteFor(AuthPhase.Locked))
        assertEquals(WebSource.Checking, WebSourcePolicy.remoteFor(AuthPhase.Starting))
    }

    @Test
    fun overrideWins() {
        assertEquals(WebSource.Local, choose(mode = WebMode.Local))
        assertEquals(WebSource.Remote, choose(mode = WebMode.Remote, online = false))
    }
}
