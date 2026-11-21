package dev.nimrag.app.engine.gps.logic

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

class GpsAssistRulesTest {
    private val h = 3600_000L

    @Test
    fun `a copy is fresh for under four hours and usable offline for under three days`() {
        val now = 100 * 24 * h
        assertTrue(GpsAssistRules.isFresh(now - 4 * h + 1, now))
        assertFalse(GpsAssistRules.isFresh(now - 4 * h, now))
        assertTrue(GpsAssistRules.isUsableOffline(now - 72 * h + 1, now))
        assertFalse(GpsAssistRules.isUsableOffline(now - 72 * h, now))
    }

    @Test
    fun `a clock set back makes a copy old, not fresh forever`() {
        val now = 100 * h
        assertFalse(GpsAssistRules.isFresh(now + h, now))
        assertFalse(GpsAssistRules.isUsableOffline(now + h, now))
    }

    @Test
    fun `the server's Age shortens how long the phone treats a copy as fresh`() {
        val now = 1_000_000_000L
        assertEquals(now, GpsAssistRules.fetchedAt(now, null))
        assertEquals(now - 3 * h, GpsAssistRules.fetchedAt(now, 3 * 3600))
        // Never more than the four hours of freshness, never negative.
        assertEquals(now - 4 * h, GpsAssistRules.fetchedAt(now, 99 * 3600))
        assertEquals(now, GpsAssistRules.fetchedAt(now, -5))
    }

    @Test
    fun `only ephemeris paths of the whitelist are asked for`() {
        val ok = listOf(
            "/ephemeris/cpe/sony/lle?coverage=WEEKS_1&constellations=GPS,GLONASS,GALILEO,QZSS",
            "/ephemeris/rxnetworks/v1/file.gz",
            "/ephemeris/a%20b",
            "/ephemeris/a+b",
        )
        for (p in ok) assertTrue(GpsAssistRules.isSafePath(p), p)
        val bad = listOf(
            "",
            "/",
            "/ephemeris",
            "ephemeris/x",
            "/api/health",
            "/ephemeris/../admin",
            "/ephemeris//x",
            "/ephemeris/%2e%2E/x",
            "/ephemeris/a%2Fb",
            "/ephemeris/a?b?c",
            "/ephemeris/a b",
            "/ephemeris/a#x",
            "/ephemeris/a\\b",
            "/ephemeris/ä",
            "/ephemeris/" + "a".repeat(600),
            "https://api.gcs.garmin.com/ephemeris/x",
        )
        for (p in bad) assertFalse(GpsAssistRules.isSafePath(p), p)
    }

    @Test
    fun `the request goes to the device endpoint of the given server with the path encoded`() {
        assertEquals(
            "https://nimrag.example.ts.net/api/device/gps-assist?path=%2Fephemeris%2Fcpe%2Fsony%2Flle%3Fcoverage%3DWEEKS_1%26constellations%3DGPS%2CGLONASS",
            GpsAssistRules.urlFor("https://nimrag.example.ts.net", "/ephemeris/cpe/sony/lle?coverage=WEEKS_1&constellations=GPS,GLONASS"),
        )
        // A plus is encoded, so it cannot turn into a space on the way.
        assertTrue(GpsAssistRules.urlFor("http://10.0.0.2:8080", "/ephemeris/a+b").endsWith("path=%2Fephemeris%2Fa%2Bb"))
    }

    @Test
    fun `ages read as a person would say them`() {
        val now = 10 * 24 * h
        assertEquals("just now", GpsAssistRules.ago(now - 20_000, now))
        assertEquals("5 min ago", GpsAssistRules.ago(now - 5 * 60_000, now))
        assertEquals("3 h ago", GpsAssistRules.ago(now - 3 * h, now))
        assertEquals("3 days ago", GpsAssistRules.ago(now - 72 * h, now))
        assertEquals("just now", GpsAssistRules.ago(now + h, now))
    }
}
