package dev.nimrag.app.ui.shell.logic

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

class ShellChromeTest {
    private fun style(
        source: WebSource,
        hasTarget: Boolean = true,
        inWatch: Boolean = false,
        height: Int = 800,
        ime: Boolean = false,
    ) = ShellChrome.navStyle(source, hasTarget, inWatch, height, ime)

    @Test
    fun `signed in shows the bar in portrait`() {
        assertEquals(NavStyle.Bar, style(WebSource.Remote))
        assertEquals(NavStyle.Bar, style(WebSource.Local))
    }

    @Test
    fun `every account step hides the navigation`() {
        for (source in listOf(WebSource.SignIn, WebSource.Unlock, WebSource.Checking)) {
            assertEquals(NavStyle.None, style(source), "portrait $source")
            assertEquals(NavStyle.None, style(source, height = 360), "landscape $source")
        }
    }

    @Test
    fun `no server address means the sign-in screen and no navigation`() {
        assertTrue(ShellChrome.accountStepShown(WebSource.Remote, hasTarget = false))
        assertEquals(NavStyle.None, style(WebSource.Remote, hasTarget = false))
        assertFalse(ShellChrome.accountStepShown(WebSource.Remote, hasTarget = true))
    }

    @Test
    fun `the watch tab keeps the navigation even while signed out`() {
        assertEquals(NavStyle.Bar, style(WebSource.SignIn, inWatch = true))
        assertEquals(NavStyle.Rail, style(WebSource.Unlock, inWatch = true, height = 360))
    }

    @Test
    fun `short windows use the rail and tall ones the bar`() {
        assertEquals(NavStyle.Rail, style(WebSource.Remote, height = 360))
        assertEquals(NavStyle.Rail, style(WebSource.Remote, height = 479))
        assertEquals(NavStyle.Bar, style(WebSource.Remote, height = 480))
        assertEquals(NavStyle.Bar, style(WebSource.Remote, height = 915))
        assertTrue(ShellChrome.useRail(411))
        assertFalse(ShellChrome.useRail(ShellChrome.COMPACT_HEIGHT_DP))
    }

    @Test
    fun `the keyboard hides the bottom bar but not the rail`() {
        assertEquals(NavStyle.None, style(WebSource.Remote, ime = true))
        assertEquals(NavStyle.Rail, style(WebSource.Remote, height = 360, ime = true))
    }
}
