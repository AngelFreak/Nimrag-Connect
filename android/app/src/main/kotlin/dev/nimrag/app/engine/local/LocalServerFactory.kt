package dev.nimrag.app.engine.local

import android.app.Application
import android.content.Context
import android.os.SystemClock
import android.util.Log
import dev.nimrag.app.engine.store.EngineDb
import dev.nimrag.app.engine.store.EngineLog
import dev.nimrag.app.engine.store.FileStore
import java.io.File
import java.security.SecureRandom
import java.util.concurrent.Executors
import uniffi.nimrag_ffi.LocalFileMeta
import uniffi.nimrag_ffi.LocalServer

/**
 * Both flavors: the real server (nimrag-server's `embedded` module, through the FFI) in
 * this process. One worker thread does everything blocking: start, then ingest every stored file
 * that has no `local_at` yet (so files stored while the server was not running, and the whole
 * backlog after an update, are caught up), then wait for the next one.
 */
object LocalServerFactory {
    fun create(
        app: Application,
        db: EngineDb,
        store: FileStore,
        log: EngineLog,
        onData: () -> Unit,
        onRunning: (Int?) -> Unit,
    ): LocalServerHost = EmbeddedLocalServer(app, db, store, log, onData, onRunning)
}

private class EmbeddedLocalServer(
    private val app: Application,
    private val db: EngineDb,
    private val store: FileStore,
    private val log: EngineLog,
    /** The local server took files: the page (when it shows the local server) fetches again. */
    private val onData: () -> Unit,
    private val onRunning: (Int?) -> Unit,
) : LocalServerHost {
    private val worker = Executors.newSingleThreadExecutor { r -> Thread(r, "nimrag-local").apply { isDaemon = true } }
    private val prefs = app.getSharedPreferences("local-server", Context.MODE_PRIVATE)

    @Volatile private var server: LocalServer? = null

    @Volatile override var web: LocalWeb? = null
        private set

    private var started = false

    override fun start() {
        synchronized(this) {
            if (started) return
            started = true
        }
        worker.execute {
            startNow()
            drain()
        }
    }

    override fun onFileStored() {
        if (!started) return
        worker.execute { drain() }
    }

    private fun startNow() {
        val t0 = SystemClock.elapsedRealtime()
        try {
            val secret = newSecret()
            // A stable port keeps the web app's origin (its storage and service worker) across
            // launches; a taken port falls back to any free one.
            val wanted = prefs.getInt(PORT, 0)
            val srv = LocalServer.start(File(app.filesDir, "local-server").path, wanted.toUShort(), secret)
            val port = srv.port().toInt()
            if (port != wanted) prefs.edit().putInt(PORT, port).apply()
            server = srv
            web = LocalWeb(port, secret, srv.secretCookieName())
            val ms = SystemClock.elapsedRealtime() - t0
            log.info("Local server up on 127.0.0.1 in $ms ms")
            Log.i(TAG, "local server started in $ms ms on port $port")
            onRunning(port)
        } catch (e: Exception) {
            // The message may carry paths; keep only the type in the user-visible log.
            log.error("Local server did not start: ${e.javaClass.simpleName}")
            Log.w(TAG, "local server did not start", e)
        }
    }

    /** Ingest what is pending, oldest first; stops at the first failure (retried at the next trigger). */
    private fun drain() {
        val srv = server ?: return
        var n = 0
        val t0 = SystemClock.elapsedRealtime()
        while (true) {
            val row = db.nextLocalPending() ?: break
            val file = store.fileFor(row.sha256)
            try {
                if (file.isFile) {
                    srv.ingestFile(
                        file.readBytes(),
                        LocalFileMeta(
                            dataType = row.dataType.toUByte(),
                            subType = row.subType.toUByte(),
                            fileIndex = row.fileIndex.toUInt(),
                            fileNumber = row.fileNumber.toUShort(),
                            watchTimestamp = row.watchTimestamp,
                        ),
                    )
                }
                db.markLocal(row.sha256, System.currentTimeMillis())
                n++
            } catch (e: Exception) {
                log.error("Local server could not take a file: ${e.javaClass.simpleName}")
                Log.w(TAG, "ingest failed", e)
                break
            }
        }
        if (n > 0) {
            val ms = SystemClock.elapsedRealtime() - t0
            log.info("Local server took $n file(s) in $ms ms")
            Log.i(TAG, "ingested $n files in $ms ms")
            onData()
        }
    }

    private fun newSecret(): String {
        val bytes = ByteArray(32).also { SecureRandom().nextBytes(it) }
        return bytes.joinToString("") { "%02x".format(it) }
    }

    private companion object {
        const val PORT = "port"
        const val TAG = "NimragLocal"
    }
}
