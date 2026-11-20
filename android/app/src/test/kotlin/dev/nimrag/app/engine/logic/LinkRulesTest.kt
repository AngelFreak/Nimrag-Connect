package dev.nimrag.app.engine.logic

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

class LinkRulesTest {
    @Test
    fun `reconnect backoff doubles from 2 seconds to 5 minutes and resets`() {
        val b = Backoff()
        assertEquals(listOf(2_000L, 4_000L, 8_000L, 16_000L, 32_000L, 64_000L, 128_000L, 256_000L), List(8) { b.next() })
        assertEquals(300_000L, b.next())
        assertEquals(300_000L, b.next())
        b.reset()
        assertEquals(2_000L, b.next())
    }

    @Test
    fun `first_connect is used for the first connection and one retry`() {
        assertTrue(LinkRules.firstConnectFor(0))
        assertTrue(LinkRules.firstConnectFor(1))
        assertFalse(LinkRules.firstConnectFor(2))
        assertFalse(LinkRules.firstConnectFor(9))
    }

    private val hour = 3_600_000L

    @Test
    fun `a sync is requested after the handshake when the last one is old enough`() {
        val now = 100 * hour
        assertTrue(LinkRules.shouldStartSync(null, now, 60))
        assertTrue(LinkRules.shouldStartSync(now - hour, now, 60), "exactly one interval ago")
        assertFalse(LinkRules.shouldStartSync(now - hour + 1, now, 60))
        assertTrue(LinkRules.shouldStartSync(now - 2 * hour, now, 60))
    }

    @Test
    fun `auto-sync 0 never requests, a clock set back counts as old`() {
        assertFalse(LinkRules.shouldStartSync(null, 1_000, 0))
        assertFalse(LinkRules.shouldStartSync(0, 10 * hour, -5))
        assertTrue(LinkRules.shouldStartSync(5_000, 1_000, 60), "last sync in the future")
    }

    private val min = 60_000L

    @Test
    fun `the auto-sync check needs a ready link and no sync running or just asked for`() {
        val now = 100 * hour
        val old = now - 2 * hour
        assertTrue(LinkRules.shouldStartAutoSync(true, false, old, now, 60))
        assertFalse(LinkRules.shouldStartAutoSync(false, false, old, now, 60), "link not ready")
        assertFalse(LinkRules.shouldStartAutoSync(true, true, old, now, 60), "a sync runs")
        assertFalse(LinkRules.shouldStartAutoSync(true, false, now - 10 * min, now, 15), "not due yet")
        assertTrue(LinkRules.shouldStartAutoSync(true, false, now - 15 * min, now, 15), "due")
        assertFalse(LinkRules.shouldStartAutoSync(true, false, old, now, 0), "auto-sync off")
    }

    @Test
    fun `opening the app syncs when the last sync is at least 5 minutes old`() {
        val now = 100 * hour
        assertTrue(LinkRules.shouldSyncOnOpen(true, false, now - 5 * min, now, 15), "exactly 5 minutes")
        assertFalse(LinkRules.shouldSyncOnOpen(true, false, now - 5 * min + 1, now, 15))
        assertFalse(LinkRules.shouldSyncOnOpen(true, false, now - 1, now, 15))
        assertTrue(LinkRules.shouldSyncOnOpen(true, false, now - 3 * hour, now, 240), "older than 5 minutes, interval not reached")
        assertTrue(LinkRules.shouldSyncOnOpen(true, false, null, now, 15), "never synced")
        assertTrue(LinkRules.shouldSyncOnOpen(true, false, now + 1_000, now, 15), "clock set back")
    }

    @Test
    fun `opening the app does not sync without a ready link, during a sync, or with auto-sync off`() {
        val now = 100 * hour
        assertFalse(LinkRules.shouldSyncOnOpen(false, false, null, now, 15), "link not ready")
        assertFalse(LinkRules.shouldSyncOnOpen(true, true, null, now, 15), "a sync runs")
        assertFalse(LinkRules.shouldSyncOnOpen(true, false, null, now, 0), "auto-sync off")
    }

    @Test
    fun `a sync asked for a moment ago counts as pending`() {
        assertFalse(LinkRules.syncAskPending(null))
        assertTrue(LinkRules.syncAskPending(0))
        assertTrue(LinkRules.syncAskPending(LinkRules.SYNC_ASK_WINDOW_MS - 1))
        assertFalse(LinkRules.syncAskPending(LinkRules.SYNC_ASK_WINDOW_MS))
        assertFalse(LinkRules.syncAskPending(-5), "elapsed time never runs backwards; a bad value is not pending")
    }

    @Test
    fun `the alarm is set for when the interval runs out`() {
        val now = 100 * hour
        assertEquals(5 * min, LinkRules.nextAutoSyncDelayMs(now - 10 * min, now, 15), "5 minutes left")
        assertEquals(LinkRules.MIN_ALARM_DELAY_MS, LinkRules.nextAutoSyncDelayMs(now - 15 * min + 1_000, now, 15), "never sooner than the minimum")
        assertEquals(15 * min, LinkRules.nextAutoSyncDelayMs(now - 15 * min, now, 15), "due: the check starts it, the next one is a whole interval away")
        assertEquals(15 * min, LinkRules.nextAutoSyncDelayMs(now - 2 * hour, now, 15), "long overdue")
        assertEquals(15 * min, LinkRules.nextAutoSyncDelayMs(null, now, 15), "never synced")
        assertEquals(15 * min, LinkRules.nextAutoSyncDelayMs(now + 1_000, now, 15), "clock set back")
    }

    @Test
    fun `the watch software version is shown with a dot`() {
        assertEquals("17.22", LinkRules.formatSoftwareVersion(1722))
        assertEquals("9.05", LinkRules.formatSoftwareVersion(905))
        assertEquals("0.00", LinkRules.formatSoftwareVersion(0))
    }
}
