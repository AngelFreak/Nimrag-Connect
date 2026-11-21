package dev.nimrag.app.engine.workouts

import dev.nimrag.app.engine.workouts.logic.HttpReply
import dev.nimrag.app.engine.workouts.logic.WorkoutHttp
import java.io.ByteArrayOutputStream
import java.io.IOException
import java.io.InputStream
import java.net.HttpURLConnection
import java.net.URL

/** [WorkoutHttp] over `HttpURLConnection`, as the file upload does it: no redirects, no caches, 30 s timeouts. */
class UrlConnectionHttp : WorkoutHttp {
    override fun get(url: String, token: String, maxBytes: Int): HttpReply = exchange(url, "GET", token, null, maxBytes)

    override fun post(url: String, token: String, json: String): HttpReply =
        exchange(url, "POST", token, json.toByteArray(Charsets.UTF_8), MAX_REPLY)

    private fun exchange(url: String, method: String, token: String, body: ByteArray?, maxBytes: Int): HttpReply {
        val conn = URL(url).openConnection() as HttpURLConnection
        try {
            conn.requestMethod = method
            conn.connectTimeout = TIMEOUT_MS
            conn.readTimeout = TIMEOUT_MS
            conn.instanceFollowRedirects = false
            conn.useCaches = false
            conn.setRequestProperty("Authorization", "Bearer $token")
            if (body != null) {
                conn.doOutput = true
                conn.setRequestProperty("Content-Type", "application/json")
                conn.setFixedLengthStreamingMode(body.size)
                conn.outputStream.use { it.write(body) }
            }
            val code = conn.responseCode
            val stream = if (code >= 400) conn.errorStream else conn.inputStream
            val bytes = stream?.use { readLimited(it, maxBytes) } ?: ByteArray(0)
            return HttpReply(code, conn.getHeaderField(REVISION_HEADER), bytes)
        } finally {
            conn.disconnect()
        }
    }

    private fun readLimited(input: InputStream, max: Int): ByteArray {
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

    private companion object {
        const val TIMEOUT_MS = 30_000
        const val MAX_REPLY = 64 * 1024
        const val REVISION_HEADER = "X-Nimrag-Revision"
    }
}
