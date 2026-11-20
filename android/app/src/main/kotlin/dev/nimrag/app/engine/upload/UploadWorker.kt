package dev.nimrag.app.engine.upload

import android.content.Context
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import dev.nimrag.app.engine.EngineSettings
import dev.nimrag.app.engine.logic.FileKind
import dev.nimrag.app.engine.logic.NetworkChangeFilter
import dev.nimrag.app.engine.logic.UploadPolicy
import dev.nimrag.app.engine.store.EngineDb
import dev.nimrag.app.engine.store.EngineLog
import dev.nimrag.app.engine.store.FileRow
import dev.nimrag.app.engine.store.FileStore
import dev.nimrag.app.model.ServerTest
import dev.nimrag.app.model.StoredFile
import dev.nimrag.app.model.UploadState
import java.io.File
import java.io.FileInputStream
import java.io.IOException
import java.net.HttpURLConnection
import java.net.URL
import java.util.concurrent.Semaphore
import java.util.concurrent.TimeUnit

/**
 * Uploads stored files on its own thread, never on the BLE thread or the main thread.
 * Woken by a new file, a network change, [uploadNow], a settings change and at start.
 *
 * Rules (IMPLEMENTATION_PLAN.md stage 2d): the active network must be unmetered or a VPN
 * unless "allow mobile data" is on. 200/201 marks the file uploaded; 401/403 stops all
 * uploads and tells [onTokenRejected] (the account forgets the token and asks for a sign-in); 413 or another 4xx parks that file; IO errors, 5xx,
 * 408 and 429 back off 30 s doubling to 1 h per file. Files stay on the phone afterwards.
 */
class UploadWorker(
    context: Context,
    private val db: EngineDb,
    private val store: FileStore,
    private val settings: EngineSettings,
    private val log: EngineLog,
    private val onState: (UploadState) -> Unit,
    /** An upload came back 401 or 403: the phone's token no longer works. */
    private val onTokenRejected: () -> Unit = {},
    private val clock: () -> Long = System::currentTimeMillis,
) {
    private val connectivity = context.getSystemService(ConnectivityManager::class.java)
    private val networkFilter = NetworkChangeFilter()
    private val signal = Semaphore(0)
    private val startLock = Any()
    private var started = false

    @Volatile private var authBlocked = false

    @Volatile private var lastError: String? = null

    @Volatile private var waitingForNetwork = false

    fun start() {
        synchronized(startLock) {
            if (started) return
            started = true
        }
        // Everything that touches the database or a system service happens on the worker thread,
        // not on the caller's (the main thread at process start).
        Thread({
            init()
            loop()
        }, "nimrag-upload").apply { isDaemon = true }.start()
    }

    private fun init() {
        lastError = runCatching { db.firstPendingError() }.getOrNull()
        try {
            connectivity.registerDefaultNetworkCallback(object : ConnectivityManager.NetworkCallback() {
                // onAvailable is followed by onCapabilitiesChanged; signal-strength noise arrives
                // there too, so only a real change of what the rule looks at wakes the worker.
                override fun onCapabilitiesChanged(network: Network, caps: NetworkCapabilities) {
                    if (networkFilter.shouldWake(infoOf(caps))) wake()
                }

                override fun onLost(network: Network) {
                    if (networkFilter.shouldWake(null)) wake()
                }
            })
        } catch (e: Exception) {
            log.warn("Could not watch network changes: ${e.javaClass.simpleName}")
        }
    }

    /** A file was stored, or anything else changed that may make an upload possible. */
    fun wake() {
        signal.release()
    }

    /** "Upload now": ignore the backoff (the network rule still applies). */
    fun uploadNow() {
        Thread({
            runCatching { db.resetBackoff() }
            wake()
        }, "nimrag-upload-reset").start()
    }

    /** The server URL, token or network rule changed: forget earlier failures and try again. */
    fun onSettingsChanged() {
        authBlocked = false
        lastError = null
        uploadNow()
    }

    /** The current numbers for `AppState`. Reads the database: call from a background thread. */
    fun snapshot(): UploadState {
        val counts = db.counts()
        val recent = db.recent(RECENT).map { it.toStoredFile() }
        return UploadState(
            pending = counts.pending,
            uploadedTotal = counts.uploaded,
            storedTotal = counts.stored,
            lastUploadAtMs = db.lastUploadAt(),
            lastError = lastError,
            waitingForNetwork = waitingForNetwork && counts.pending > 0,
            recent = recent,
        )
    }

    /** `GET /api/health`, then an authenticated request. Blocking: call off the main thread. */
    fun testServer(): ServerTest {
        val base = UploadPolicy.normalizeServerUrl(settings.serverUrl)
            ?: return ServerTest.Failed("Enter a server address starting with http:// or https://")
        try {
            val health = request("$base/api/health", "GET", null)
            if (health != 200) return ServerTest.Failed("The server answered HTTP $health to /api/health")
            val token = settings.token ?: return ServerTest.Failed("The server is reachable, but this phone is not signed in")
            // Any path behind the token check will do; an unknown hash gives 404 with a good token.
            val auth = request("$base/api/files/${"0".repeat(64)}", "GET", token)
            return when (auth) {
                200, 404 -> ServerTest.Ok
                401, 403 -> ServerTest.Failed("The server no longer accepts this phone (HTTP $auth)")
                else -> ServerTest.Failed("The server answered HTTP $auth to an authenticated request")
            }
        } catch (e: IOException) {
            return ServerTest.Failed(UploadPolicy.describeIo(e))
        } catch (_: Exception) {
            // Never the exception's text: it could name the URL or worse.
            return ServerTest.Failed(UploadPolicy.describeIo(IllegalStateException()))
        }
    }

    // ---- worker thread ----

    private fun loop() {
        while (true) {
            val waitMs = try {
                runOnce()
            } catch (t: Throwable) {
                log.error("Upload worker error: ${t.javaClass.simpleName}")
                RETRY_AFTER_ERROR_MS
            }
            signal.tryAcquire(waitMs, TimeUnit.MILLISECONDS)
            signal.drainPermits()
        }
    }

    /** One pass over the due files. Returns how long to sleep unless woken. */
    private fun runOnce(): Long {
        val base = UploadPolicy.normalizeServerUrl(settings.serverUrl)
        val token = settings.token
        if (base == null || token == null) {
            waitingForNetwork = false
            publish()
            return IDLE_MS
        }
        if (authBlocked) {
            waitingForNetwork = false
            publish()
            return IDLE_MS
        }
        while (true) {
            val pending = db.counts().pending
            val allowed = UploadPolicy.networkAllows(currentNetwork(), settings.allowMobileData)
            waitingForNetwork = pending > 0 && !allowed
            if (!allowed || pending == 0) {
                publish()
                return IDLE_MS
            }
            val row = db.nextDue(clock()) ?: break
            val outcome = upload(base, token, row)
            applyOutcome(row, outcome)
            publish()
            if (outcome is UploadPolicy.Outcome.StopAll) {
                authBlocked = true
                onTokenRejected()
                return IDLE_MS
            }
        }
        publish()
        val next = db.earliestRetryAt() ?: return IDLE_MS
        return (next - clock()).coerceIn(1_000L, IDLE_MS)
    }

    private fun upload(base: String, token: String, row: FileRow): UploadPolicy.Outcome {
        val file: File = store.fileFor(row.sha256)
        if (!file.isFile) return UploadPolicy.Outcome.FailFile("The stored file is missing on the phone")
        val url = UploadPolicy.uploadUrl(
            base,
            UploadPolicy.UploadMeta(
                dataType = row.dataType,
                subType = row.subType,
                fileIndex = row.fileIndex,
                fileNumber = row.fileNumber,
                watchTimestamp = row.watchTimestamp,
                sha256 = row.sha256,
            ),
        )
        return try {
            val status = request(url, "POST", token, file)
            UploadPolicy.outcome(status)
        } catch (e: IOException) {
            UploadPolicy.ioFailure(UploadPolicy.describeIo(e))
        } catch (_: Exception) {
            UploadPolicy.ioFailure(UploadPolicy.describeIo(IllegalStateException()))
        }
    }

    private fun applyOutcome(row: FileRow, outcome: UploadPolicy.Outcome) {
        val kind = FileKind.label(row.dataType, row.subType)
        when (outcome) {
            UploadPolicy.Outcome.Uploaded -> {
                db.markUploaded(row.sha256, clock())
                lastError = null
                log.info("Uploaded a $kind file (${row.size} bytes)")
            }
            is UploadPolicy.Outcome.StopAll -> {
                lastError = outcome.reason
                log.error("${outcome.reason}. Uploads stop until this phone signs in again.")
            }
            is UploadPolicy.Outcome.FailFile -> {
                db.markFailed(row.sha256, row.attempts + 1, UploadPolicy.PARKED, outcome.reason)
                lastError = outcome.reason
                log.error("Upload of a $kind file failed: ${outcome.reason}")
            }
            is UploadPolicy.Outcome.Retry -> {
                val attempts = row.attempts + 1
                db.markFailed(row.sha256, attempts, UploadPolicy.nextAttemptAt(clock(), attempts), outcome.reason)
                lastError = outcome.reason
                log.warn("Upload of a $kind file failed: ${outcome.reason}")
            }
        }
    }

    private fun publish() {
        try {
            onState(snapshot())
        } catch (e: Exception) {
            log.error("Could not read the upload state: ${e.javaClass.simpleName}")
        }
    }

    private fun currentNetwork(): UploadPolicy.NetworkInfo? {
        return try {
            val network = connectivity.activeNetwork ?: return null
            val caps = connectivity.getNetworkCapabilities(network) ?: return null
            infoOf(caps)
        } catch (_: SecurityException) {
            null
        }
    }

    private fun infoOf(caps: NetworkCapabilities) = UploadPolicy.NetworkInfo(
        hasInternet = caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET),
        unmetered = caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_NOT_METERED),
        vpn = caps.hasTransport(NetworkCapabilities.TRANSPORT_VPN),
        validated = caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_VALIDATED),
    )

    /** One HTTP exchange; returns the status code. The token only ever goes into the header. */
    private fun request(url: String, method: String, token: String?, body: File? = null): Int {
        val conn = URL(url).openConnection() as HttpURLConnection
        try {
            conn.requestMethod = method
            conn.connectTimeout = TIMEOUT_MS
            conn.readTimeout = TIMEOUT_MS
            conn.instanceFollowRedirects = false
            conn.useCaches = false
            if (token != null) conn.setRequestProperty("Authorization", "Bearer $token")
            if (body != null) {
                conn.doOutput = true
                conn.setRequestProperty("Content-Type", "application/octet-stream")
                conn.setFixedLengthStreamingMode(body.length())
                try {
                    FileInputStream(body).use { input ->
                        conn.outputStream.use { out -> input.copyTo(out, 16 * 1024) }
                    }
                } catch (e: IOException) {
                    // A server may answer (413, 401) and close before the body is sent. If its
                    // answer can be read, that is the real outcome; otherwise it stays an IO error.
                    val early = try {
                        conn.responseCode
                    } catch (_: IOException) {
                        -1
                    }
                    if (early > 0) {
                        (if (early >= 400) conn.errorStream else null)?.close()
                        return early
                    }
                    throw e
                }
            }
            val code = conn.responseCode
            // Drain so the connection ends cleanly.
            (if (code >= 400) conn.errorStream else conn.inputStream)?.use { it.readBytes() }
            return code
        } finally {
            conn.disconnect()
        }
    }

    private fun FileRow.toStoredFile() = StoredFile(
        sha256 = sha256,
        sizeBytes = size,
        kind = FileKind.label(dataType, subType),
        watchTimeMs = if (watchTimestamp > 0) (watchTimestamp + GARMIN_EPOCH_UNIX_S) * 1000 else null,
        receivedAtMs = receivedAt,
        uploadedAtMs = uploadedAt,
    )

    private companion object {
        const val RECENT = 50
        const val TIMEOUT_MS = 30_000
        const val IDLE_MS = 3_600_000L
        const val RETRY_AFTER_ERROR_MS = 60_000L
        const val GARMIN_EPOCH_UNIX_S = 631_065_600L
    }
}
