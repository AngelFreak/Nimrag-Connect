package dev.nimrag.app.engine.phone

import dev.nimrag.app.engine.phone.logic.WeatherHttp
import dev.nimrag.app.engine.phone.logic.WeatherRules
import dev.nimrag.app.engine.workouts.logic.HttpReply
import java.io.ByteArrayOutputStream
import java.io.IOException
import java.net.HttpURLConnection
import java.net.URL

/**
 * [WeatherHttp] over `HttpURLConnection`, set up like the file upload (no redirects, no caches)
 * but with short timeouts: the watch is waiting, and the protocol client gives a started answer
 * only 30 s. The server gives Open-Meteo 10 s, so 15 s of reading covers a slow upstream.
 */
class UrlConnectionWeatherHttp : WeatherHttp {
    override fun post(url: String, token: String, json: String, maxBytes: Int): HttpReply {
        val body = json.toByteArray(Charsets.UTF_8)
        val conn = URL(url).openConnection() as HttpURLConnection
        try {
            conn.requestMethod = "POST"
            conn.connectTimeout = WeatherRules.CONNECT_TIMEOUT_MS
            conn.readTimeout = WeatherRules.READ_TIMEOUT_MS
            conn.instanceFollowRedirects = false
            conn.useCaches = false
            conn.doOutput = true
            conn.setRequestProperty("Authorization", "Bearer $token")
            conn.setRequestProperty("Content-Type", "application/json")
            conn.setFixedLengthStreamingMode(body.size)
            conn.outputStream.use { it.write(body) }
            val code = conn.responseCode
            val stream = if (code >= 400) conn.errorStream else conn.inputStream
            val bytes = stream?.use { input ->
                val out = ByteArrayOutputStream()
                val buffer = ByteArray(8 * 1024)
                while (true) {
                    val n = input.read(buffer)
                    if (n < 0) break
                    if (out.size() + n > maxBytes) throw IOException("The server's answer is too long")
                    out.write(buffer, 0, n)
                }
                out.toByteArray()
            } ?: ByteArray(0)
            return HttpReply(code, null, bytes)
        } finally {
            conn.disconnect()
        }
    }
}
