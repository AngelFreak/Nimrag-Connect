package dev.nimrag.app.engine.logic

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertNotNull
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test
import java.time.Instant
import java.time.ZoneId

class DstRuleTest {
    private fun rules(zone: String) = ZoneId.of(zone).rules

    private fun unix(iso: String) = Instant.parse(iso).epochSecond

    @Test
    fun `winter time in Copenhagen sends the next start first and the end after it`() {
        // 2026-11-19: next transition is into DST (2027-03-28 01:00Z), the one after is out of it.
        val pair = DstRule.compute(rules("Europe/Copenhagen"), Instant.parse("2026-11-19T12:00:00Z"))
        assertNotNull(pair)
        assertEquals(unix("2027-03-28T01:00:00Z"), pair!!.startUnixS)
        assertEquals(unix("2027-10-31T01:00:00Z"), pair.endUnixS)
        assertTrue(pair.startUnixS < pair.endUnixS)
    }

    @Test
    fun `matches the 5052 reply seen in capture CAP-1 (Garmin epoch)`() {
        val pair = DstRule.compute(rules("Europe/Copenhagen"), Instant.parse("2026-11-19T12:00:00Z"))!!
        val garminEpoch = 631_065_600L
        assertEquals(1_175_130_000L, pair.startUnixS - garminEpoch)
        assertEquals(1_193_878_800L, pair.endUnixS - garminEpoch)
    }

    @Test
    fun `summer time in Copenhagen sends the start as the later of the two`() {
        // 2026-07-01: next transition is out of DST (2026-10-25 01:00Z); the start is the one after it.
        val pair = DstRule.compute(rules("Europe/Copenhagen"), Instant.parse("2026-07-01T12:00:00Z"))!!
        assertEquals(unix("2026-10-25T01:00:00Z"), pair.endUnixS)
        assertEquals(unix("2027-03-28T01:00:00Z"), pair.startUnixS)
        assertTrue(pair.startUnixS > pair.endUnixS, "in summer time the start is the later one")
    }

    @Test
    fun `southern hemisphere summer follows the same rule`() {
        // Sydney in January is on DST; the next transition (April) is out of it.
        val summer = DstRule.compute(rules("Australia/Sydney"), Instant.parse("2027-01-15T00:00:00Z"))!!
        assertTrue(summer.startUnixS > summer.endUnixS)
        // Sydney in July is on standard time; the next transition (October) is into DST.
        val winter = DstRule.compute(rules("Australia/Sydney"), Instant.parse("2027-07-15T00:00:00Z"))!!
        assertTrue(winter.startUnixS < winter.endUnixS)
    }

    @Test
    fun `just before a transition the pair is already the next one`() {
        val transition = unix("2027-03-28T01:00:00Z")
        val before = DstRule.compute(rules("Europe/Copenhagen"), Instant.ofEpochSecond(transition - 1))!!
        assertEquals(transition, before.startUnixS)
        // At the transition instant itself that transition is over: the next one is out of DST.
        val at = DstRule.compute(rules("Europe/Copenhagen"), Instant.ofEpochSecond(transition))!!
        assertEquals(unix("2027-10-31T01:00:00Z"), at.endUnixS)
        assertTrue(at.startUnixS > at.endUnixS)
    }

    @Test
    fun `zones without transitions give no pair`() {
        assertNull(DstRule.compute(rules("UTC"), Instant.parse("2026-11-19T12:00:00Z")))
        assertNull(DstRule.compute(rules("Asia/Tokyo"), Instant.parse("2026-11-19T12:00:00Z")))
        assertNull(DstRule.compute(rules("Asia/Kolkata"), Instant.parse("2026-07-01T00:00:00Z")))
    }
}
