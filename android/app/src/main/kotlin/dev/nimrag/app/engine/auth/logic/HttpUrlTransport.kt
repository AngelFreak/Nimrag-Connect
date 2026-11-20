package dev.nimrag.app.engine.auth.logic

import java.io.ByteArrayOutputStream
import java.io.IOException
import java.io.InputStream
import java.net.HttpURLConnection
import java.net.URL

/**
 * [HttpTransport] over `HttpURLConnection` (java.net only, so it runs in JVM tests too).
 * No redirects (a POST must not be silently replayed elsewhere), no caches, short timeouts, and
 * the answer body is read up to [MAX_BODY] bytes.
 */
class HttpUrlTransport(private val timeoutMs: Int = TIMEOUT_MS) : HttpTransport {
    override fun send(request: HttpRequest): HttpResponse {
        val conn = URL(request.url).openConnection() as HttpURLConnection
        try {
            conn.requestMethod = request.method
            conn.connectTimeout = timeoutMs
            conn.readTimeout = timeoutMs
            conn.instanceFollowRedirects = false
            conn.useCaches = false
            for ((k, v) in request.headers) conn.setRequestProperty(k, v)
            val body = request.body
            if (body != null) {
                val bytes = body.toByteArray(Charsets.UTF_8)
                conn.doOutput = true
                conn.setFixedLengthStreamingMode(bytes.size)
                conn.outputStream.use { it.write(bytes) }
            }
            val status = conn.responseCode
            val text = (if (status >= 400) conn.errorStream else conn.inputStream)?.use(::readCapped).orEmpty()
            val headers = HashMap<String, List<String>>()
            for ((name, values) in conn.headerFields) {
                if (name != null && values != null) headers[name.lowercase()] = values
            }
            return HttpResponse(status, headers, text)
        } finally {
            conn.disconnect()
        }
    }

    private fun readCapped(input: InputStream): String {
        val out = ByteArrayOutputStream()
        val buf = ByteArray(4096)
        while (out.size() <= MAX_BODY) {
            val n = input.read(buf)
            if (n < 0) break
            out.write(buf, 0, n)
        }
        if (out.size() > MAX_BODY) throw IOException("answer too long")
        return String(out.toByteArray(), Charsets.UTF_8)
    }

    companion object {
        const val TIMEOUT_MS = 15_000
        const val MAX_BODY = 64 * 1024
    }
}
