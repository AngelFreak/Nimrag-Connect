package dev.nimrag.app.engine.auth

import android.content.Context
import android.webkit.CookieManager
import android.webkit.WebStorage
import android.webkit.WebView
import dev.nimrag.app.engine.auth.logic.SessionCookie
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.withContext
import kotlin.coroutines.resume

/**
 * The server session inside the web view's cookie jar. The app signs in natively, so the cookie
 * from the server's answer is put into the jar (for the server's origin, before the web view
 * loads) and the web app opens signed in. The cookie is rebuilt from the answer with our own
 * attributes ([SessionCookie.toSetCookie]); its value is never logged.
 *
 * CookieManager needs a Looper thread for its callbacks, so every call here runs on the main
 * thread. Only the server's session cookie is touched: the local server's secret cookie (another
 * origin) stays.
 */
class WebSession(private val context: Context) {
    /** The session cookie the jar holds for [base]'s origin, or null. */
    fun current(base: String): SessionCookie? = SessionCookie.fromJar(CookieManager.getInstance().getCookie("$base/"))

    /** Puts [cookie] into the jar for [base] (replacing an older session) and writes it to disk. */
    suspend fun install(base: String, cookie: SessionCookie) = withContext(Dispatchers.Main) {
        val manager = CookieManager.getInstance()
        manager.setAcceptCookie(true)
        suspendCancellableCoroutine { done ->
            manager.setCookie("$base/", cookie.toSetCookie()) { done.resume(Unit) }
        }
        manager.flush()
    }

    /**
     * Ends the web side of a session: the session cookie of [base] goes, and so does everything
     * the web app stored (its offline cache of the last person's data, its local storage).
     */
    suspend fun clear(base: String?) = withContext(Dispatchers.Main) {
        val manager = CookieManager.getInstance()
        if (base != null) {
            for (name in listOf(SessionCookie.NAME_SECURE, SessionCookie.NAME_INSECURE)) {
                suspendCancellableCoroutine { done ->
                    val secure = if (name == SessionCookie.NAME_SECURE) "; Secure" else ""
                    manager.setCookie("$base/", "$name=; Path=/; Max-Age=0$secure") { done.resume(Unit) }
                }
            }
        }
        manager.flush()
        WebStorage.getInstance().deleteAllData()
        // The HTTP cache is shared by every web view of the app: a throwaway one clears it.
        WebView(context).apply {
            clearCache(true)
            destroy()
        }
    }
}
