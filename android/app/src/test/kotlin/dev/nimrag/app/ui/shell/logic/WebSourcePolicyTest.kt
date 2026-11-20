package dev.nimrag.app.ui.shell.logic

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
    ) = WebSourcePolicy.choose(mode, localReady, remoteConfigured, online, remoteFailed, pending)

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
    fun overrideWins() {
        assertEquals(WebSource.Local, choose(mode = WebMode.Local))
        assertEquals(WebSource.Remote, choose(mode = WebMode.Remote, online = false))
    }
}
