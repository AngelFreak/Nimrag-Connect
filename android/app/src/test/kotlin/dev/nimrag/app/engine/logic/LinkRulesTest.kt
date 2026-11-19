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

    @Test
    fun `the watch software version is shown with a dot`() {
        assertEquals("17.22", LinkRules.formatSoftwareVersion(1722))
        assertEquals("9.05", LinkRules.formatSoftwareVersion(905))
        assertEquals("0.00", LinkRules.formatSoftwareVersion(0))
    }
}
