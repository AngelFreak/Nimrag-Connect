package dev.nimrag.app.ui.shell.logic

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

class ServerOriginTest {
    private val server = ServerOrigin.parse("https://nimrag.example.ts.net")!!.origin

    @Test
    fun `a plain https address is a target with a trailing slash`() {
        val t = ServerOrigin.parse("https://nimrag.example.ts.net")!!
        assertEquals("https://nimrag.example.ts.net/", t.startUrl)
        assertEquals(Origin("https", "nimrag.example.ts.net", 443), t.origin)
    }

    @Test
    fun `whitespace and a trailing slash are tolerated`() {
        val t = ServerOrigin.parse("  http://192.168.1.20:8080/  ")!!
        assertEquals("http://192.168.1.20:8080/", t.startUrl)
        assertEquals(Origin("http", "192.168.1.20", 8080), t.origin)
    }

    @Test
    fun `a path prefix is kept in the start url`() {
        val t = ServerOrigin.parse("https://host.example/nimrag")!!
        assertEquals("https://host.example/nimrag", t.startUrl)
    }

    @Test
    fun `blank and invalid addresses are not targets`() {
        for (raw in listOf("", "   ", "nimrag.local", "ftp://host", "https://", "file:///sdcard/x", "javascript:alert(1)", "https://host?x=1", "https://host#a")) {
            assertNull(ServerOrigin.parse(raw), "'$raw'")
        }
    }

    @Test
    fun `credentials in the address are refused`() {
        assertNull(ServerOrigin.parse("https://good.example@evil.example"))
        assertNull(ServerOrigin.parse("https://user:pw@host.example"))
    }

    @Test
    fun `isInvalid is false for blank, true for a bad non-blank address`() {
        assertFalse(ServerOrigin.isInvalid(""))
        assertFalse(ServerOrigin.isInvalid("https://ok.example"))
        assertTrue(ServerOrigin.isInvalid("not a url"))
    }

    @Test
    fun `origin of a url fills in the default port and lower-cases scheme and host`() {
        assertEquals(Origin("https", "host.example", 443), ServerOrigin.originOf("HTTPS://Host.Example/a/b?c#d"))
        assertEquals(Origin("http", "host.example", 80), ServerOrigin.originOf("http://host.example"))
        assertEquals(Origin("http", "[::1]", 8080), ServerOrigin.originOf("http://[::1]:8080/x"))
    }

    @Test
    fun `origin of non-web or broken urls is null`() {
        for (u in listOf(null, "", "about:blank", "data:text/html,hi", "file:///etc/passwd", "javascript:1", "https://", "http://host:notaport", "ht tp://x")) {
            assertNull(ServerOrigin.originOf(u), "'$u'")
        }
    }

    @Test
    fun `rule leaves out the default port and keeps another`() {
        assertEquals("https://host.example", Origin("https", "host.example", 443).rule)
        assertEquals("http://host.example", Origin("http", "host.example", 80).rule)
        assertEquals("http://192.168.1.20:8080", Origin("http", "192.168.1.20", 8080).rule)
        assertEquals("https://host.example:80", Origin("https", "host.example", 80).rule)
    }

    @Test
    fun `same origin ignores path, query and default port spelling`() {
        assertTrue(ServerOrigin.isSameOrigin("https://nimrag.example.ts.net/activities?type=strength", server))
        assertTrue(ServerOrigin.isSameOrigin("https://NIMRAG.example.ts.net:443/", server))
    }

    @Test
    fun `different scheme, host, port or lookalike hosts are not the same origin`() {
        assertFalse(ServerOrigin.isSameOrigin("http://nimrag.example.ts.net/", server))
        assertFalse(ServerOrigin.isSameOrigin("https://nimrag.example.ts.net:8443/", server))
        assertFalse(ServerOrigin.isSameOrigin("https://evil.example/", server))
        assertFalse(ServerOrigin.isSameOrigin("https://nimrag.example.ts.net.evil.example/", server))
        assertFalse(ServerOrigin.isSameOrigin("https://evil.example/nimrag.example.ts.net", server))
        assertFalse(ServerOrigin.isSameOrigin("https://nimrag.example.ts.net@evil.example/", server))
    }

    @Test
    fun `no server, no url or a non-web url is never the server origin`() {
        assertFalse(ServerOrigin.isSameOrigin("https://nimrag.example.ts.net/", null))
        assertFalse(ServerOrigin.isSameOrigin(null, server))
        assertFalse(ServerOrigin.isSameOrigin("about:blank", server))
        assertFalse(ServerOrigin.isSameOrigin("file:///x", server))
    }

    @Test
    fun `navigation on the server origin is allowed in any frame`() {
        assertEquals(NavigationDecision.Allow, ServerOrigin.decide("https://nimrag.example.ts.net/health", server, mainFrame = true))
        assertEquals(NavigationDecision.Allow, ServerOrigin.decide("https://nimrag.example.ts.net/x", server, mainFrame = false))
    }

    @Test
    fun `another web site opens in the browser from the main frame only`() {
        assertEquals(NavigationDecision.OpenExternal, ServerOrigin.decide("https://example.org/docs", server, mainFrame = true))
        assertEquals(NavigationDecision.OpenExternal, ServerOrigin.decide("http://example.org/", server, mainFrame = true))
        assertEquals(NavigationDecision.Block, ServerOrigin.decide("https://example.org/docs", server, mainFrame = false))
    }

    @Test
    fun `other schemes are blocked, never sent to an intent`() {
        for (u in listOf("intent://x#Intent;scheme=http;end", "mailto:a@b.c", "tel:112", "file:///sdcard/x", "content://x/y", "javascript:alert(1)", "data:text/html,x", "about:blank", "market://details?id=x")) {
            assertEquals(NavigationDecision.Block, ServerOrigin.decide(u, server, mainFrame = true), u)
        }
    }

    @Test
    fun `without a server every web link is external, never allowed`() {
        assertEquals(NavigationDecision.OpenExternal, ServerOrigin.decide("https://example.org/", null, mainFrame = true))
    }
}
