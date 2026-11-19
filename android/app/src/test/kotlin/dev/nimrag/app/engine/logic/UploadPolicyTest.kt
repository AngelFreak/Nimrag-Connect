package dev.nimrag.app.engine.logic

import dev.nimrag.app.engine.logic.UploadPolicy.NetworkInfo
import dev.nimrag.app.engine.logic.UploadPolicy.Outcome
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

class UploadPolicyTest {
    @Test
    fun `200 and 201 mean uploaded`() {
        assertEquals(Outcome.Uploaded, UploadPolicy.outcome(200))
        assertEquals(Outcome.Uploaded, UploadPolicy.outcome(201))
    }

    @Test
    fun `401 and 403 stop every upload`() {
        assertTrue(UploadPolicy.outcome(401) is Outcome.StopAll)
        assertTrue(UploadPolicy.outcome(403) is Outcome.StopAll)
    }

    @Test
    fun `413 and other client errors fail only that file`() {
        for (code in listOf(400, 404, 413, 422, 499)) {
            assertTrue(UploadPolicy.outcome(code) is Outcome.FailFile, "status $code")
        }
    }

    @Test
    fun `server errors, 408 and 429 are retried`() {
        for (code in listOf(408, 429, 500, 502, 503, 599)) {
            assertTrue(UploadPolicy.outcome(code) is Outcome.Retry, "status $code")
        }
    }

    @Test
    fun `unexpected codes such as redirects are retried, not parked`() {
        for (code in listOf(204, 301, 302, 100)) {
            assertTrue(UploadPolicy.outcome(code) is Outcome.Retry, "status $code")
        }
        assertTrue(UploadPolicy.ioFailure("timeout") is Outcome.Retry)
    }

    @Test
    fun `backoff starts at 30 seconds and doubles`() {
        assertEquals(30_000L, UploadPolicy.backoffMs(1))
        assertEquals(60_000L, UploadPolicy.backoffMs(2))
        assertEquals(120_000L, UploadPolicy.backoffMs(3))
        assertEquals(240_000L, UploadPolicy.backoffMs(4))
        assertEquals(480_000L, UploadPolicy.backoffMs(5))
    }

    @Test
    fun `backoff is capped at one hour and never overflows`() {
        assertEquals(1_920_000L, UploadPolicy.backoffMs(7)) // 32 min, still below the cap
        assertEquals(3_600_000L, UploadPolicy.backoffMs(8)) // 30 s * 128 = 64 min, capped
        for (attempts in listOf(9, 20, 64, 1_000, Int.MAX_VALUE)) {
            assertEquals(3_600_000L, UploadPolicy.backoffMs(attempts), "attempts $attempts")
        }
        var previous = 0L
        for (attempts in 1..40) {
            val now = UploadPolicy.backoffMs(attempts)
            assertTrue(now >= previous)
            previous = now
        }
    }

    @Test
    fun `zero or negative attempts count as the first`() {
        assertEquals(30_000L, UploadPolicy.backoffMs(0))
        assertEquals(30_000L, UploadPolicy.backoffMs(-3))
        assertEquals(1_000L + 30_000L, UploadPolicy.nextAttemptAt(1_000L, 1))
    }

    private fun net(internet: Boolean = true, unmetered: Boolean = false, vpn: Boolean = false) =
        NetworkInfo(hasInternet = internet, unmetered = unmetered, vpn = vpn)

    @Test
    fun `unmetered networks and VPNs are allowed by default`() {
        assertTrue(UploadPolicy.networkAllows(net(unmetered = true), allowMobileData = false))
        assertTrue(UploadPolicy.networkAllows(net(vpn = true), allowMobileData = false))
    }

    @Test
    fun `mobile data needs the setting`() {
        assertFalse(UploadPolicy.networkAllows(net(), allowMobileData = false))
        assertTrue(UploadPolicy.networkAllows(net(), allowMobileData = true))
    }

    @Test
    fun `no network or no Internet never uploads`() {
        assertFalse(UploadPolicy.networkAllows(null, allowMobileData = true))
        assertFalse(UploadPolicy.networkAllows(net(internet = false, unmetered = true), allowMobileData = true))
        assertFalse(UploadPolicy.networkAllows(net(internet = false, vpn = true), allowMobileData = false))
    }

    @Test
    fun `server addresses are normalised and validated`() {
        assertEquals("https://nimrag.example.ts.net", UploadPolicy.normalizeServerUrl(" https://nimrag.example.ts.net/ "))
        assertEquals("http://192.168.1.5:8080", UploadPolicy.normalizeServerUrl("http://192.168.1.5:8080"))
        assertEquals("http://host/sub", UploadPolicy.normalizeServerUrl("http://host/sub//"))
        assertNull(UploadPolicy.normalizeServerUrl(""))
        assertNull(UploadPolicy.normalizeServerUrl("   "))
        assertNull(UploadPolicy.normalizeServerUrl("nimrag.example.com"))
        assertNull(UploadPolicy.normalizeServerUrl("ftp://host"))
        assertNull(UploadPolicy.normalizeServerUrl("https://"))
        assertNull(UploadPolicy.normalizeServerUrl("https://host?x=1"))
        assertNull(UploadPolicy.normalizeServerUrl("https://exa mple.com"))
    }

    @Test
    fun `the upload url carries the metadata and the hash`() {
        val meta = UploadPolicy.UploadMeta(128, 4, 71, 1071, 1_100_000_000L, "ab".repeat(32))
        assertEquals(
            "https://h/api/files?data_type=128&sub_type=4&file_index=71&file_number=1071" +
                "&watch_timestamp=1100000000&source=watch&sha256=${"ab".repeat(32)}",
            UploadPolicy.uploadUrl("https://h", meta),
        )
        val none = UploadPolicy.uploadUrl("http://h", meta.copy(watchTimestamp = 0))
        assertFalse(none.contains("watch_timestamp"))
        assertTrue(none.contains("source=watch"))
    }
}
