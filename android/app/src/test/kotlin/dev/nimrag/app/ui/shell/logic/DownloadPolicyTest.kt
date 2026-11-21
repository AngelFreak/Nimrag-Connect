package dev.nimrag.app.ui.shell.logic

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

class DownloadPolicyTest {
    private val server = ServerOrigin.parse("https://nimrag.example.ts.net")!!.origin

    @Test
    fun `only a download from the server's own origin is taken`() {
        assertTrue(DownloadPolicy.allow("https://nimrag.example.ts.net/api/export", server))
        assertFalse(DownloadPolicy.allow("https://evil.example/api/export", server))
        assertFalse(DownloadPolicy.allow("http://nimrag.example.ts.net/api/export", server))
        assertFalse(DownloadPolicy.allow("https://nimrag.example.ts.net:8443/api/export", server))
        assertFalse(DownloadPolicy.allow("file:///sdcard/x.zip", server))
        assertFalse(DownloadPolicy.allow("blob:https://nimrag.example.ts.net/1234", server))
        assertFalse(DownloadPolicy.allow(null, server))
        assertFalse(DownloadPolicy.allow("https://nimrag.example.ts.net/api/export", null))
    }

    @Test
    fun `the name comes from Content-Disposition, quoted or not`() {
        assertEquals(
            "nimrag-export-alice-2026-11-21.zip",
            DownloadPolicy.fileName("attachment; filename=\"nimrag-export-alice-2026-11-21.zip\"", "https://h/api/export"),
        )
        assertEquals("x.zip", DownloadPolicy.fileName("attachment; filename=x.zip", "https://h/api/export"))
        assertEquals("x.zip", DownloadPolicy.fileName("Attachment; FILENAME = \"x.zip\" ; size=3", null))
    }

    @Test
    fun `without a header the last path segment is the name, else a fallback`() {
        assertEquals("export", DownloadPolicy.fileName(null, "https://h/api/export?x=1#y"))
        assertEquals("export", DownloadPolicy.fileName("attachment", "https://h/api/export/"))
        assertEquals(DownloadPolicy.FALLBACK_NAME, DownloadPolicy.fileName(null, null))
        assertEquals(DownloadPolicy.FALLBACK_NAME, DownloadPolicy.fileName("attachment; filename=\"\"", null))
    }

    @Test
    fun `a name cannot leave the downloads folder or hide`() {
        assertEquals("passwd", DownloadPolicy.fileName("attachment; filename=\"../../etc/passwd\"", null))
        assertEquals("evil.zip", DownloadPolicy.fileName("attachment; filename=\"C:\\\\dir\\\\evil.zip\"", null))
        assertEquals("hidden", DownloadPolicy.fileName("attachment; filename=\"..hidden\"", null))
        assertEquals("a_b_c.zip", DownloadPolicy.fileName("attachment; filename=\"a b\u00e5c.zip\"", null))
        assertEquals(100, DownloadPolicy.fileName("attachment; filename=\"${"a".repeat(300)}\"", null).length)
    }
}
