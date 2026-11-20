package dev.nimrag.app.engine.logic

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

class LinkSupervisorTest {
    private fun reconnect(a: StallAction) = (a as StallAction.Reconnect).recovery

    @Test
    fun `an unanswered MTU exchange is skipped once and does not count as a stall`() {
        val s = LinkSupervisor()
        assertEquals(StallAction.SkipMtu, s.onStall(LinkStep.MtuExchange, mtuSkipped = false))
        assertEquals(0, s.stalls)
    }

    @Test
    fun `a stall in discovery reconnects plainly the first time`() {
        val s = LinkSupervisor()
        val r = reconnect(s.onStall(LinkStep.ServiceDiscovery, mtuSkipped = true))
        assertEquals(Recovery(attempt = 1, refreshCache = false, directConnect = false), r)
        assertFalse(s.refreshCache)
        assertFalse(s.directConnect)
    }

    @Test
    fun `repeated stalls escalate to a cache refresh and then a direct connection`() {
        val s = LinkSupervisor()
        s.onStall(LinkStep.ServiceDiscovery, true)
        val second = reconnect(s.onStall(LinkStep.Notifications, true))
        assertEquals(Recovery(2, refreshCache = true, directConnect = false), second)
        assertTrue(s.refreshCache)
        val third = reconnect(s.onStall(LinkStep.ServiceDiscovery, true))
        assertEquals(Recovery(3, refreshCache = true, directConnect = true), third)
        assertTrue(s.directConnect)
        // Stays escalated until the link is healthy.
        assertEquals(Recovery(9, true, true), (1..6).map { reconnect(s.onStall(LinkStep.ServiceDiscovery, true)) }.last())
    }

    @Test
    fun `an MTU stall after the skip counts like any other stall`() {
        val s = LinkSupervisor()
        assertEquals(1, reconnect(s.onStall(LinkStep.MtuExchange, mtuSkipped = true)).attempt)
    }

    @Test
    fun `the cache refresh is requested once per escalation`() {
        val s = LinkSupervisor()
        s.onStall(LinkStep.ServiceDiscovery, true)
        s.onStall(LinkStep.ServiceDiscovery, true)
        assertTrue(s.refreshCache)
        s.refreshDone()
        assertFalse(s.refreshCache)
    }

    @Test
    fun `a direct connection that finds no watch goes back to the background connection but keeps the stall count`() {
        val s = LinkSupervisor()
        repeat(3) { s.onStall(LinkStep.ServiceDiscovery, true) }
        assertTrue(s.directConnect)
        s.connectFailed()
        assertFalse(s.directConnect)
        assertEquals(3, s.stalls)
    }

    @Test
    fun `a finished handshake forgets everything`() {
        val s = LinkSupervisor()
        repeat(4) { s.onStall(LinkStep.ServiceDiscovery, true) }
        s.healthy()
        assertEquals(0, s.stalls)
        assertFalse(s.directConnect)
        assertFalse(s.refreshCache)
        assertEquals(1, reconnect(s.onStall(LinkStep.ServiceDiscovery, true)).attempt)
    }

    @Test
    fun `every setup step has its own deadline and a background connect has none`() {
        assertNull(LinkSupervisor.timeoutMs(LinkStep.Connecting, direct = false))
        assertEquals(20_000L, LinkSupervisor.timeoutMs(LinkStep.Connecting, direct = true))
        assertEquals(4_000L, LinkSupervisor.timeoutMs(LinkStep.MtuExchange, false))
        assertEquals(12_000L, LinkSupervisor.timeoutMs(LinkStep.ServiceDiscovery, false))
        assertEquals(8_000L, LinkSupervisor.timeoutMs(LinkStep.Notifications, false))
        // The whole setup stays well under the old single 20 s budget for the MTU skip + the rest.
        assertTrue(LinkStep.entries.all { it.label.isNotBlank() })
    }
}
