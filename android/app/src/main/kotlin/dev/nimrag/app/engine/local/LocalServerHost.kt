package dev.nimrag.app.engine.local

/**
 * The server inside the app (personal flavor only). Every stored watch file is also fed to it,
 * so the web interface can show the newest data from the phone itself, with no network.
 * The `public` flavor has a no-op implementation ([LocalServerFactory] lives in each flavor's
 * source set): its library has no server and no FIT profile.
 */
interface LocalServerHost {
    /** Starts the server in the background and ingests the files it has not seen. Idempotent. */
    fun start()

    /** A watch file is durable in the store: ingest it (off the caller's thread). */
    fun onFileStored()

    /** How to reach the server once it runs; null before that, after a failure, and in `public`. */
    val web: LocalWeb?
}

/**
 * Address and launch secret of the running local server. The secret goes only into the web
 * view's cookie jar ([cookie]); [toString] never shows it.
 */
class LocalWeb(val port: Int, private val secret: String, private val cookieName: String) {
    val startUrl: String get() = "http://127.0.0.1:$port/"

    /** `Set-Cookie` value for `startUrl`: every request without it gets 403 from the server. */
    fun cookie(): String = "$cookieName=$secret; Path=/; HttpOnly; SameSite=Strict"

    override fun toString(): String = "LocalWeb(port=$port)"
}
