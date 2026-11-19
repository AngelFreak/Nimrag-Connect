package dev.nimrag.app.engine.logic

import dev.nimrag.app.engine.logic.UploadPolicy.NetworkInfo
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test
import java.io.IOException
import java.net.ConnectException
import java.net.SocketTimeoutException
import java.net.UnknownHostException
import javax.net.ssl.SSLHandshakeException

class FirstConnectBudgetTest {
    /** Simulates connections: each is (reachedHandshake, pairingCompleted). */
    private fun run(connections: List<Pair<Boolean, Boolean>>): List<Boolean> {
        var set = true
        var tries = 0
        val used = mutableListOf<Boolean>()
        for ((handshake, completed) in connections) {
            val first = set && LinkRules.firstConnectFor(tries)
            used += first
            if (handshake && first && set) {
                val budget = LinkRules.afterHandshake(tries)
                tries = budget.tries
                if (!budget.stillSet) set = false
            }
            if (completed) {
                set = false
                tries = 0
            }
        }
        return used
    }

    @Test
    fun `early drops and channel retries do not use the budget`() {
        // Five connections that never reach the handshake, then one that does.
        val used = run(List(5) { false to false } + listOf(true to false))
        assertEquals(List(6) { true }, used)
    }

    @Test
    fun `two finished handshakes without PairingCompleted use it up`() {
        val used = run(listOf(true to false, true to false, true to false, true to false))
        assertEquals(listOf(true, true, false, false), used)
    }

    @Test
    fun `PairingCompleted ends it at once`() {
        assertEquals(listOf(true, false, false), run(listOf(true to true, true to false, true to false)))
    }

    @Test
    fun `afterHandshake counts and reports the remaining state`() {
        assertEquals(LinkRules.FirstConnect(stillSet = true, tries = 1), LinkRules.afterHandshake(0))
        assertEquals(LinkRules.FirstConnect(stillSet = false, tries = 2), LinkRules.afterHandshake(1))
        assertFalse(LinkRules.afterHandshake(5).stillSet)
    }
}

class TokenValidationTest {
    @Test
    fun `printable ascii tokens are valid`() {
        assertTrue(UploadPolicy.isValidToken("abc123"))
        assertTrue(UploadPolicy.isValidToken("A-b_c.d~e+f/g=="))
        assertTrue(UploadPolicy.isValidToken("!~"))
        assertTrue(UploadPolicy.isValidToken("x".repeat(UploadPolicy.MAX_TOKEN_LENGTH)))
    }

    @Test
    fun `whitespace and control characters are rejected`() {
        for (bad in listOf("a b", " abc", "abc ", "abc\n", "a\tb", "ab\r\ncd", "ab\u0000cd", "ab\u007Fcd", "\u0001")) {
            assertFalse(UploadPolicy.isValidToken(bad), "token ${bad.map { it.code }}")
        }
    }

    @Test
    fun `non-ascii characters are rejected`() {
        for (bad in listOf("tøken", "token’", " token", "tok​en", "日本語")) {
            assertFalse(UploadPolicy.isValidToken(bad))
        }
    }

    @Test
    fun `empty and overlong tokens are rejected`() {
        assertFalse(UploadPolicy.isValidToken(""))
        assertFalse(UploadPolicy.isValidToken("x".repeat(UploadPolicy.MAX_TOKEN_LENGTH + 1)))
    }

    @Test
    fun `io failures are described by kind only, never with the exception text`() {
        val secret = "https://host/path?token=SECRET123"
        val all = listOf(
            UploadPolicy.describeIo(UnknownHostException(secret)),
            UploadPolicy.describeIo(SocketTimeoutException(secret)),
            UploadPolicy.describeIo(ConnectException(secret)),
            UploadPolicy.describeIo(SSLHandshakeException(secret)),
            UploadPolicy.describeIo(IOException(secret)),
            UploadPolicy.describeIo(IllegalStateException(secret)),
        )
        assertEquals(all.size, all.toSet().size, "each kind has its own fixed text")
        all.forEach { assertFalse(it.contains("SECRET") || it.contains("host/path"), it) }
    }
}

class NetworkChangeFilterTest {
    private val wifi = NetworkInfo(hasInternet = true, unmetered = true, vpn = false, validated = true)

    @Test
    fun `the first report and a lost network wake the worker`() {
        val f = NetworkChangeFilter()
        assertTrue(f.shouldWake(wifi))
        assertTrue(f.shouldWake(null))
        assertFalse(f.shouldWake(null), "still gone")
        assertTrue(f.shouldWake(wifi))
    }

    @Test
    fun `repeated identical capabilities do not wake it`() {
        val f = NetworkChangeFilter()
        assertTrue(f.shouldWake(wifi))
        repeat(20) { assertFalse(f.shouldWake(wifi.copy())) }
    }

    @Test
    fun `each property the rule looks at wakes it when it changes`() {
        val f = NetworkChangeFilter()
        f.shouldWake(wifi)
        assertTrue(f.shouldWake(wifi.copy(validated = false)))
        assertTrue(f.shouldWake(wifi))
        assertTrue(f.shouldWake(wifi.copy(unmetered = false)))
        assertTrue(f.shouldWake(wifi.copy(unmetered = false, vpn = true)))
        assertTrue(f.shouldWake(wifi.copy(hasInternet = false)))
    }
}
