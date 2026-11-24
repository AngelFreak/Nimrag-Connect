package dev.nimrag.app.engine.local

import dev.nimrag.app.engine.logic.EditOutbox

/**
 * The server inside the app (both flavors; the library is built with ffi feature `local-server`).
 * Every stored watch file is also fed to it, so the web interface can show the newest data from
 * the phone itself, with no network. The public flavor derives with Nimrag's own FIT field map,
 * the personal one with Garmin's profile.
 */
interface LocalServerHost {
    /** Starts the server in the background and ingests the files it has not seen. Idempotent. */
    fun start()

    /** A watch file is durable in the store: ingest it (off the caller's thread). */
    fun onFileStored()

    /** How to reach the server once it runs; null before that and after a failure. */
    val web: LocalWeb?

    /**
     * The changes made on the phone's copy that the real server has not got yet (offline edits),
     * and the state pulled back from it. Null before the server runs and after a failure.
     */
    val edits: EditOutbox?
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
