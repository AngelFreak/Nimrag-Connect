package dev.nimrag.app.engine.gps

import dev.nimrag.app.engine.gps.logic.GpsAssistHttp
import dev.nimrag.app.engine.gps.logic.GpsReply
import java.io.ByteArrayOutputStream
import java.io.IOException
import java.net.HttpURLConnection
import java.net.URL

/**
 * [GpsAssistHttp] over `HttpURLConnection`, as the workout client does it: no redirects (the token
 * must not follow one to another host), no caches, the Bearer token in the header only.
 */
class UrlConnectionGpsHttp : GpsAssistHttp {
    override fun get(url: String, token: String, ifNoneMatch: String?, timeoutMs: Int, maxBytes: Int): GpsReply {
        val conn = URL(url).openConnection() as HttpURLConnection
        try {
            conn.requestMethod = "GET"
            conn.connectTimeout = timeoutMs
            conn.readTimeout = timeoutMs
            conn.instanceFollowRedirects = false
            conn.useCaches = false
            conn.setRequestProperty("Authorization", "Bearer $token")
            if (ifNoneMatch != null) conn.setRequestProperty("If-None-Match", ifNoneMatch)
            val code = conn.responseCode
            val stream = if (code >= 400) conn.errorStream else conn.inputStream
            val bytes = if (code == 200) stream?.use { readLimited(it, maxBytes) } ?: ByteArray(0) else ByteArray(0)
            return GpsReply(code, conn.getHeaderField("ETag"), conn.getHeaderField("Age")?.trim()?.toLongOrNull(), bytes)
        } finally {
            conn.disconnect()
        }
    }

    private fun readLimited(input: java.io.InputStream, max: Int): ByteArray {
        val out = ByteArrayOutputStream()
        val buffer = ByteArray(8 * 1024)
        while (true) {
            val n = input.read(buffer)
            if (n < 0) break
            if (out.size() + n > max) throw IOException("The server's answer is too long")
            out.write(buffer, 0, n)
        }
        return out.toByteArray()
    }
}
