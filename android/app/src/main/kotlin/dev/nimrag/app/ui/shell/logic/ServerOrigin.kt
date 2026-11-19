package dev.nimrag.app.ui.shell.logic

import dev.nimrag.app.engine.logic.UploadPolicy
import java.net.URI

/*
 * Pure rules for which addresses the embedded web view may load (IMPLEMENTATION_PLAN.md, stage
 * 3n). No Android types: java.net.URI only, so the unit tests run on the JVM.
 */

/** scheme + host + port of an http(s) address; the default port is always filled in. */
data class Origin(val scheme: String, val host: String, val port: Int) {
    /**
     * The form WebViewCompat.addDocumentStartJavaScript takes as an allowed origin rule:
     * `scheme://host[:port]`, the default port left out, no path.
     */
    val rule: String
        get() = if (port == defaultPort(scheme)) "$scheme://$host" else "$scheme://$host:$port"

    companion object {
        fun defaultPort(scheme: String): Int = if (scheme == "https") 443 else 80
    }
}

/** A usable server address: where to load first, and the only origin the web view may stay on. */
data class ServerTarget(val startUrl: String, val origin: Origin)

/** What to do with a navigation the web view is about to make. */
enum class NavigationDecision {
    /** Same origin as the server: the web view loads it. */
    Allow,

    /** Another http(s) site: open it in the browser, the web view stays where it is. */
    OpenExternal,

    /** Anything else (other schemes, unparsable, a frame on another origin): dropped. */
    Block,
}

object ServerOrigin {
    /**
     * The saved server address as a [ServerTarget], or null when it is blank or not usable
     * (not http or https, no host, a query or fragment, credentials in the address). The same
     * rule the engine uses for uploads, plus the credentials check: `https://good@evil` must
     * never be read as "good".
     */
    fun parse(raw: String): ServerTarget? {
        val normal = UploadPolicy.normalizeServerUrl(raw) ?: return null
        val uri = try {
            URI(normal)
        } catch (_: Exception) {
            return null
        }
        if (uri.rawUserInfo != null) return null
        val origin = originOf(uri) ?: return null
        val path = uri.rawPath.orEmpty()
        return ServerTarget(startUrl = if (path.isEmpty()) "$normal/" else normal, origin = origin)
    }

    /** True when [raw] is not blank but still not a usable address (the Settings screen says so). */
    fun isInvalid(raw: String): Boolean = raw.isNotBlank() && parse(raw) == null

    /** The origin of an http(s) address; null for other schemes, no host, or anything unparsable. */
    fun originOf(url: String?): Origin? {
        if (url.isNullOrBlank()) return null
        val uri = try {
            URI(url.trim())
        } catch (_: Exception) {
            return null
        }
        return originOf(uri)
    }

    private fun originOf(uri: URI): Origin? {
        val scheme = uri.scheme?.lowercase() ?: return null
        if (scheme != "http" && scheme != "https") return null
        val host = uri.host?.lowercase()?.takeIf { it.isNotEmpty() } ?: return null
        val port = if (uri.port == -1) Origin.defaultPort(scheme) else uri.port
        return Origin(scheme, host, port)
    }

    /** True when [currentUrl] is on [server]'s origin. A missing server or URL never matches. */
    fun isSameOrigin(currentUrl: String?, server: Origin?): Boolean {
        if (server == null) return false
        return originOf(currentUrl) == server
    }

    /**
     * Decides a navigation. [mainFrame] false is a frame inside the page: it may stay on the
     * server origin, but a frame on another origin is dropped (never sent to the browser).
     */
    fun decide(requestUrl: String?, server: Origin?, mainFrame: Boolean): NavigationDecision {
        val target = originOf(requestUrl) ?: return NavigationDecision.Block
        if (server != null && target == server) return NavigationDecision.Allow
        return if (mainFrame) NavigationDecision.OpenExternal else NavigationDecision.Block
    }
}
