package dev.nimrag.app.engine.logic

import dev.nimrag.app.model.GarminConnectMode
import dev.nimrag.app.model.SyncResult
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

class AlongsideRulesTest {
    private val off = GarminConnectMode.Off
    private val alongside = GarminConnectMode.Alongside

    @Test
    fun `only Off archives on the watch`() {
        assertTrue(AlongsideRules.archive(off))
        assertFalse(AlongsideRules.archive(alongside))
    }

    @Test
    fun `Alongside never sends first_connect, Off passes the budget's answer through`() {
        assertTrue(AlongsideRules.firstConnect(off, true))
        assertFalse(AlongsideRules.firstConnect(off, false))
        assertFalse(AlongsideRules.firstConnect(alongside, true))
        assertFalse(AlongsideRules.firstConnect(alongside, false))
    }

    @Test
    fun `the other-app hint is for Off only`() {
        assertTrue(AlongsideRules.hintsOtherApp(off))
        assertFalse(AlongsideRules.hintsOtherApp(alongside))
    }

    @Test
    fun `copied keys count for 60 days`() {
        val now = 100L * 24 * 3_600_000
        val since = AlongsideRules.skipSince(now)
        assertEquals(40L * 24 * 3_600_000, since)
        assertEquals(60L * 24 * 3_600_000, now - since)
        // A row is included when copied_at >= since: exactly 60 days old still counts.
        assertTrue(since >= 0)
    }

    @Test
    fun `a stored mode name maps back, anything else is Off`() {
        assertEquals(alongside, AlongsideRules.modeFromStored("Alongside"))
        assertEquals(off, AlongsideRules.modeFromStored("Off"))
        assertEquals(off, AlongsideRules.modeFromStored(null))
        assertEquals(off, AlongsideRules.modeFromStored(""))
        assertEquals(off, AlongsideRules.modeFromStored("Forward"))
        GarminConnectMode.entries.forEach { assertEquals(it, AlongsideRules.modeFromStored(it.name)) }
    }

    @Test
    fun `the sync summary is one line per mode and mentions skipped files only when there are some`() {
        val plain = SyncResult(finishedAtMs = 1, downloaded = 2, archived = 2, failed = 1)
        assertEquals("Sync finished: 2 downloaded, 2 archived, 1 failed", AlongsideRules.syncSummary(plain, off))
        val skipped = plain.copy(archived = 0, skipped = 7)
        assertEquals("Sync finished: 2 downloaded, 0 archived, 1 failed, 7 skipped", AlongsideRules.syncSummary(skipped, off))
        assertEquals("Sync finished: 2 copied, 1 failed", AlongsideRules.syncSummary(plain.copy(archived = 0), alongside))
        assertEquals(
            "Sync finished: 2 copied, 1 failed, 7 skipped (copied before)",
            AlongsideRules.syncSummary(skipped, alongside),
        )
        assertFalse(AlongsideRules.syncSummary(skipped, alongside).contains('\n'))
    }
}
