package dev.nimrag.app.engine.upload

import android.content.Context
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import dev.nimrag.app.engine.EngineSettings
import dev.nimrag.app.engine.logic.DroppedEdits
import dev.nimrag.app.engine.logic.EditBackoff
import dev.nimrag.app.engine.logic.EditDrain
import dev.nimrag.app.engine.logic.EditOutbox
import dev.nimrag.app.engine.logic.EditPolicy
import dev.nimrag.app.engine.logic.FileKind
import dev.nimrag.app.engine.logic.HistoryChange
import dev.nimrag.app.engine.logic.HistoryChoice
import dev.nimrag.app.engine.logic.HistoryCopier
import dev.nimrag.app.engine.logic.HistoryCopy
import dev.nimrag.app.engine.logic.HistoryDownload
import dev.nimrag.app.engine.logic.HistoryFile
import dev.nimrag.app.engine.logic.HistoryLog
import dev.nimrag.app.engine.logic.HistoryPage
import dev.nimrag.app.engine.logic.HistoryPolicy
import dev.nimrag.app.engine.logic.HistoryRecheck
import dev.nimrag.app.engine.logic.HistoryServer
import dev.nimrag.app.engine.logic.NetworkChangeFilter
import dev.nimrag.app.engine.logic.ServerReply
import dev.nimrag.app.engine.logic.StatePull
import dev.nimrag.app.engine.logic.UploadPolicy
import dev.nimrag.app.engine.store.EngineDb
import dev.nimrag.app.engine.store.EngineLog
import dev.nimrag.app.engine.store.FileRow
import dev.nimrag.app.engine.store.FileStore
import dev.nimrag.app.model.ServerTest
import dev.nimrag.app.model.StoredFile
import dev.nimrag.app.model.UploadState
import java.io.ByteArrayOutputStream
import java.io.File
import java.io.FileInputStream
import java.io.IOException
import java.io.InputStream
import java.net.HttpURLConnection
import java.net.URL
import java.util.concurrent.Semaphore
import java.util.concurrent.TimeUnit

/**
 * Uploads stored files on its own thread, never on the BLE thread or the main thread.
 * Woken by a new file, a new edit ([editsChanged]), a network change, [uploadNow], a settings
 * change, the app coming back ([onAppResumed]) and at start.
 *
 * Rules (IMPLEMENTATION_PLAN.md stage 2d): the active network must be unmetered or a VPN
 * unless "allow mobile data" is on. 200/201 marks the file uploaded; 401/403 stops all
 * uploads and tells [onTokenRejected] (the account forgets the token and asks for a sign-in); 413 or another 4xx parks that file; IO errors, 5xx,
 * 408 and 429 back off 30 s doubling to 1 h per file. Files stay on the phone afterwards.
 *
 * After the files of each pass come the offline edits (docs/design/offline-edits.md): the changes
 * made on the phone's copy go to `POST /api/device/edits`, oldest first, under the same network
 * rule, the same 401/403 handling and the same back-off. Once the outbox is empty the server's
 * state is pulled back onto the phone (at start, after a pass that sent something, on resume).
 *
 * Last comes the history step (docs/design/history-on-phone.md), only when the person chose to
 * keep the server's history on the phone: files the server has and the phone lacks are copied
 * into the phone's copy of the server, 25 files or 10 MB per pass, newest first. They never go
 * through [store], so they are never uploaded back.
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
    /** A pass finished and the server took a file, or the phone took the server's state: new data is there to show. */
    private val onUploaded: () -> Unit = {},
    /** The phone's copy and its outbox of changes; null while the local server is not running. */
    private val edits: () -> EditOutbox? = { null },
    /** Changes the server refused, one line each for the person (shown once). */
    private val onEditNotices: (List<String>) -> Unit = {},
    /** The phone's copy as server history is put into it; null while the local server is not running. */
    private val history: () -> HistoryCopy? = { null },
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

    /** Files the server took in the current pass (worker thread only). */
    private var uploadedInPass = 0

    /** The server's state went onto the phone in the current pass (worker thread only). */
    private var stateTakenInPass = false

    /** The server took a change or gave its state in the current pass: it is reachable (worker thread only). */
    private var serverAnsweredInPass = false

    private val editBackoff = EditBackoff()

    /** The server's state should be pulled when the outbox is empty: at start, after sending, on resume. */
    @Volatile private var pullWanted = true

    @Volatile private var lastPullAt = 0L

    /** Its own back-off: a failing history step must never hold up the uploads or the edits. */
    private val historyBackoff = EditBackoff()

    private val historyRecheck = HistoryRecheck(settings.historyCheckedAtMs)

    private val historyCopier = HistoryCopier(
        copy = object : HistoryCopy {
            private fun copy() = history() ?: throw IllegalStateException("The local server is not running")

            override fun missing(sha256s: List<String>) = copy().missing(sha256s)

            override fun ingest(bytes: ByteArray, file: HistoryFile) = copy().ingest(bytes, file)

            override fun prune(keepSince: Long?) = copy().prune(keepSince)
        },
        server = object : HistoryServer {
            override fun list(cursor: String?, limit: Int, since: Long?): HistoryPage {
                val base = historyBase ?: return HistoryPage.Failed("No server address")
                val token = historyToken ?: return HistoryPage.Failed("This phone is not signed in")
                return HistoryPolicy.page(exchange(HistoryPolicy.listUrl(base, cursor, limit, since), "GET", token, null))
            }

            override fun download(sha256: String): HistoryDownload {
                val base = historyBase ?: return HistoryDownload.Failed("No server address")
                val token = historyToken ?: return HistoryDownload.Failed("This phone is not signed in")
                return fetchBytes(HistoryPolicy.contentUrl(base, sha256), token)
            }
        },
    )

    /** The server and token the current history pass talks to (worker thread sets them, the copier reads them). */
    @Volatile private var historyBase: String? = null

    @Volatile private var historyToken: String? = null

    /** `snapshot` reads, then publishes: two threads doing it at once must not leave the older numbers last. */
    private val publishLock = Any()

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

    /** The number of waiting changes changed (see [EditWatcher]): show it, and send a new one. */
    fun editsChanged(newEdit: Boolean) {
        publish()
        if (newEdit) wake()
    }

    /** The choice "Server history on this phone" changed: widening copies at once, narrowing removes the copies outside. */
    fun onHistoryChoiceChanged() {
        wake()
    }

    /** The app came back to the screen: the server may have changed things; pull its state (not more than every 30 s). */
    fun onAppResumed() {
        if (EditPolicy.resumePullDue(lastPullAt, clock())) {
            pullWanted = true
            wake()
        }
    }

    /** "Upload now": ignore the backoff (the network rule still applies). */
    fun uploadNow() {
        Thread({
            runCatching { db.resetBackoff() }
            editBackoff.succeeded()
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
        val editsWaiting = pendingEdits()
        return UploadState(
            pending = counts.pending,
            pendingEdits = editsWaiting,
            uploadedTotal = counts.uploaded,
            storedTotal = counts.stored,
            lastUploadAtMs = db.lastUploadAt(),
            lastError = lastError,
            waitingForNetwork = waitingForNetwork && (counts.pending > 0 || editsWaiting > 0),
            recent = recent,
        )
    }

    /** The same network rule the uploads follow (the workout sender uses it too). Safe from any thread. */
    fun networkAllowsUploads(): Boolean = UploadPolicy.networkAllows(currentNetwork(), settings.allowMobileData)

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
            uploadedInPass = 0
            stateTakenInPass = false
            serverAnsweredInPass = false
            val waitMs = try {
                runOnce()
            } catch (t: Throwable) {
                log.error("Upload worker error: ${t.javaClass.simpleName}")
                RETRY_AFTER_ERROR_MS
            }
            // Once per pass, not per file: a sync stores many files and the page needs one refresh.
            if (uploadedInPass > 0 || stateTakenInPass) runCatching { onUploaded() }
            signal.tryAcquire(waitMs, TimeUnit.MILLISECONDS)
            signal.drainPermits()
        }
    }

    /** One pass: the due files, then the offline edits. Returns how long to sleep unless woken. */
    private fun runOnce(): Long {
        // The person's choice first: removing copies needs no network and no sign-in.
        applyHistoryChoice()
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
        val filesWait = uploadFiles(base, token) ?: return IDLE_MS
        // New uploads reached the server: its history may have grown, so look again (at most every 6 hours).
        if (uploadedInPass > 0) historyRecheck.onUploadsReached()
        val editsWait = syncEdits(base, token) ?: return IDLE_MS
        val historyWait = syncHistory(base, token) ?: return IDLE_MS
        publish()
        // Files skipped for their retry delay while the server just answered: it is back, send them now.
        if (serverAnsweredInPass && db.retryWaitingNow(clock()) > 0) return 0L
        return minOf(filesWait, editsWait, historyWait)
    }

    /** The due files, oldest first. Returns how long until the next retry, or null when the token was refused. */
    private fun uploadFiles(base: String, token: String): Long? {
        val editsWaiting = pendingEdits()
        while (true) {
            val pending = db.counts().pending
            val allowed = UploadPolicy.networkAllows(currentNetwork(), settings.allowMobileData)
            waitingForNetwork = (pending > 0 || editsWaiting > 0) && !allowed
            if (!allowed || pending == 0) return IDLE_MS
            val row = db.nextDue(clock()) ?: break
            val outcome = upload(base, token, row)
            applyOutcome(row, outcome)
            publish()
            if (outcome is UploadPolicy.Outcome.StopAll) {
                authBlocked = true
                onTokenRejected()
                return null
            }
        }
        val next = db.earliestRetryAt() ?: return IDLE_MS
        return (next - clock()).coerceIn(1_000L, IDLE_MS)
    }

    /**
     * The offline edits: send the outbox, then pull the server's state once it is empty. Same
     * network rule as the files. Returns how long until the next try, or null when the token was refused.
     */
    private fun syncEdits(base: String, token: String): Long? {
        val outbox = edits() ?: return IDLE_MS
        reportDropped(outbox) // refusals from an earlier run too
        if (!UploadPolicy.networkAllows(currentNetwork(), settings.allowMobileData)) return IDLE_MS
        val wait = editBackoff.waitMs(clock())
        if (wait > 0) return wait.coerceIn(1_000L, IDLE_MS)

        if (outbox.pending() > 0) {
            val report = EditDrain(outbox).run { edit ->
                exchange("$base/api/device/edits", "POST", token, EditPolicy.requestJson(edit))
            }
            reportDropped(outbox)
            if (report.applied > 0) {
                serverAnsweredInPass = true
                pullWanted = true
                lastError = null
                log.info("Sent ${report.applied} offline change(s) to the server")
            }
            when (val end = report.end) {
                EditDrain.End.Empty -> editBackoff.succeeded()
                is EditDrain.End.Kept -> {
                    lastError = end.reason
                    log.warn("Sending an offline change failed: ${end.reason}")
                    return editBackoff.failed(clock()).coerceIn(1_000L, IDLE_MS)
                }
                is EditDrain.End.TokenRejected -> return rejectToken(end.status)
            }
        }
        if (!pullWanted) return IDLE_MS
        when (val pull = StatePull(outbox).run { exchange("$base/api/device/state", "GET", token, null) }) {
            StatePull.Result.Applied -> {
                pullWanted = false
                lastPullAt = clock()
                stateTakenInPass = true
                serverAnsweredInPass = true
                editBackoff.succeeded()
                log.info("Took the server's settings, workouts and entries onto the phone")
            }
            // Changes wait (a send failed, or one was made meanwhile): the pull follows once they are sent.
            StatePull.Result.Skipped, StatePull.Result.Later -> Unit
            is StatePull.Result.Failed -> {
                log.warn("Could not pull the server's state: ${pull.reason}")
                return editBackoff.failed(clock()).coerceIn(1_000L, IDLE_MS)
            }
            is StatePull.Result.TokenRejected -> return rejectToken(pull.status)
        }
        return IDLE_MS
    }

    /**
     * Matches the phone's copy to the person's choice: a narrower one removes the copied files
     * outside it (then the derived data is rebuilt), a wider one starts looking at once. What the
     * copy matches is remembered ([EngineSettings.historyApplied]), so a change made while the
     * local server was not running is still carried out later.
     */
    private fun applyHistoryChoice() {
        val wanted = settings.historyRetention
        val applied = settings.historyApplied
        when (HistoryChoice.change(applied, wanted)) {
            HistoryChange.None -> return
            HistoryChange.Widened -> {
                historyRecheck.onWidened()
                settings.historyCheckedAtMs = 0L
                historyCopier.reset()
                historyBackoff.succeeded()
                log.info(HistoryLog.choice(wanted))
            }
            HistoryChange.Narrowed -> {
                val copy = history() ?: return // retried when the local server runs
                val removed = try {
                    copy.prune(HistoryChoice.pruneSince(wanted, clock()))
                } catch (e: Exception) {
                    log.error("Could not remove the server's history from the phone: ${e.javaClass.simpleName}")
                    return
                }
                historyCopier.reset()
                log.info(HistoryLog.choice(wanted))
                if (removed > 0) {
                    log.info(HistoryLog.pruned(removed, wanted))
                    stateTakenInPass = true // the data on the page changed
                }
            }
        }
        settings.historyApplied = wanted
    }

    /**
     * The history step: copy a pass of the server's files the phone lacks. Same network rule,
     * same 401/403 handling as the uploads, its own back-off. Returns how long until the next try
     * (0 while files are left, so the next pass follows at once), or null when the token was refused.
     */
    private fun syncHistory(base: String, token: String): Long? {
        val retention = settings.historyRetention
        if (!retention.copies || retention != settings.historyApplied) return IDLE_MS
        if (history() == null) return IDLE_MS
        if (!UploadPolicy.networkAllows(currentNetwork(), settings.allowMobileData)) return IDLE_MS
        val backoff = historyBackoff.waitMs(clock())
        if (backoff > 0) return backoff.coerceIn(1_000L, IDLE_MS)
        val wait = historyRecheck.waitMs(clock()) ?: return IDLE_MS
        if (wait > 0) return wait.coerceIn(1_000L, IDLE_MS)

        historyBase = base
        historyToken = token
        val result = try {
            historyCopier.pass(retention, clock())
        } finally {
            historyBase = null
            historyToken = null
        }
        when (result) {
            is HistoryCopier.Result.More -> {
                historyBackoff.succeeded()
                historyRecheck.onWorkLeft()
                // Not `serverAnsweredInPass`: passes follow each other at once while files are
                // left, and a file whose upload keeps failing would be retried after every one.
                stateTakenInPass = stateTakenInPass || result.copied > 0
                if (result.copied > 0) log.info(HistoryLog.pass(result.copied, result.left))
                return 0L
            }
            is HistoryCopier.Result.Done -> {
                historyBackoff.succeeded()
                historyRecheck.onChecked(clock())
                settings.historyCheckedAtMs = clock()
                serverAnsweredInPass = true
                stateTakenInPass = stateTakenInPass || result.copied > 0
                if (result.copied > 0) log.info(HistoryLog.pass(result.copied, 0))
                if (result.total > 0) log.info(HistoryLog.done(result.total))
                return IDLE_MS
            }
            is HistoryCopier.Result.Failed -> {
                stateTakenInPass = stateTakenInPass || result.copied > 0
                if (result.copied > 0) log.info(HistoryLog.pass(result.copied, historyCopier.left()))
                log.warn(HistoryLog.failed(result.reason))
                return historyBackoff.failed(clock()).coerceIn(1_000L, IDLE_MS)
            }
            is HistoryCopier.Result.TokenRejected -> {
                stateTakenInPass = stateTakenInPass || result.copied > 0
                return rejectToken(result.status)
            }
        }
    }

    private fun rejectToken(status: Int): Long? {
        val reason = EditPolicy.keptReason(ServerReply(status))
        lastError = reason
        log.error("$reason. Uploads stop until this phone signs in again.")
        authBlocked = true
        onTokenRejected()
        return null
    }

    /** The changes the server refused: one log line each and one message each for the person, once. */
    private fun reportDropped(outbox: EditOutbox) {
        val dropped = try {
            outbox.takeDropped()
        } catch (e: Exception) {
            log.error("Could not read the refused changes: ${e.javaClass.simpleName}")
            return
        }
        if (dropped.isEmpty()) return
        dropped.forEach { log.warn(DroppedEdits.logLine(it)) }
        onEditNotices(dropped.map(DroppedEdits::message))
    }

    private fun pendingEdits(): Int = try {
        edits()?.pending() ?: 0
    } catch (_: Exception) {
        0
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
                uploadedInPass++
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
        synchronized(publishLock) {
            try {
                onState(snapshot())
            } catch (e: Exception) {
                log.error("Could not read the upload state: ${e.javaClass.simpleName}")
            }
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

    /**
     * One JSON exchange with the server for the offline edits. A change is only ever decided by a
     * real answer of the server ([EditPolicy.accept]); an IO error is "no answer".
     */
    private fun exchange(url: String, method: String, token: String, json: String?): ServerReply {
        val conn = URL(url).openConnection() as HttpURLConnection
        try {
            conn.requestMethod = method
            conn.connectTimeout = TIMEOUT_MS
            conn.readTimeout = TIMEOUT_MS
            conn.instanceFollowRedirects = false
            conn.useCaches = false
            conn.setRequestProperty("Authorization", "Bearer $token")
            conn.setRequestProperty("Accept", "application/json")
            if (json != null) {
                val bytes = json.toByteArray(Charsets.UTF_8)
                conn.doOutput = true
                conn.setRequestProperty("Content-Type", "application/json")
                conn.setFixedLengthStreamingMode(bytes.size)
                conn.outputStream.use { it.write(bytes) }
            }
            val code = conn.responseCode
            val stream = if (code >= 400) conn.errorStream else conn.inputStream
            val body = if (stream == null) "" else stream.use { readLimited(it) }
                ?: return ServerReply.noAnswer("The server's answer was too large")
            return EditPolicy.accept(code, conn.contentType, body)
        } catch (e: IOException) {
            return ServerReply.noAnswer(UploadPolicy.describeIo(e))
        } catch (_: Exception) {
            return ServerReply.noAnswer(UploadPolicy.describeIo(IllegalStateException()))
        } finally {
            conn.disconnect()
        }
    }

    /**
     * One file of the server's history. A 200 is the bytes (the phone's copy checks their hash, so
     * a broken download is never stored); 404 means the file went away meanwhile.
     */
    private fun fetchBytes(url: String, token: String): HistoryDownload {
        val conn = URL(url).openConnection() as HttpURLConnection
        try {
            conn.requestMethod = "GET"
            conn.connectTimeout = TIMEOUT_MS
            conn.readTimeout = TIMEOUT_MS
            conn.instanceFollowRedirects = false
            conn.useCaches = false
            conn.setRequestProperty("Authorization", "Bearer $token")
            val code = conn.responseCode
            if (code != 200) {
                (if (code >= 400) conn.errorStream else conn.inputStream)?.use { it.readBytes() }
                return HistoryPolicy.downloadFailure(code)
            }
            val bytes = conn.inputStream.use { readBytesLimited(it) }
                ?: return HistoryDownload.Failed("A file of the server's history is too large")
            return HistoryDownload.Bytes(bytes)
        } catch (e: IOException) {
            return HistoryDownload.Failed(UploadPolicy.describeIo(e))
        } catch (_: Exception) {
            return HistoryDownload.Failed(UploadPolicy.describeIo(IllegalStateException()))
        } finally {
            conn.disconnect()
        }
    }

    private fun readBytesLimited(input: InputStream): ByteArray? {
        val out = ByteArrayOutputStream()
        val buffer = ByteArray(16 * 1024)
        while (true) {
            val n = input.read(buffer)
            if (n < 0) break
            out.write(buffer, 0, n)
            if (out.size() > HistoryPolicy.MAX_FILE_BYTES) return null
        }
        return out.toByteArray()
    }

    /** The whole text, or null when it is longer than [MAX_ANSWER_BYTES]. */
    private fun readLimited(input: InputStream): String? {
        val out = ByteArrayOutputStream()
        val buffer = ByteArray(16 * 1024)
        while (true) {
            val n = input.read(buffer)
            if (n < 0) break
            out.write(buffer, 0, n)
            if (out.size() > MAX_ANSWER_BYTES) return null
        }
        return out.toString(Charsets.UTF_8.name())
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
        const val MAX_ANSWER_BYTES = 16 * 1024 * 1024
        const val GARMIN_EPOCH_UNIX_S = 631_065_600L
    }
}
