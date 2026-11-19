package dev.nimrag.app.engine

import android.content.IntentSender
import dev.nimrag.app.engine.ble.BleScanner
import dev.nimrag.app.engine.ble.WatchLink
import dev.nimrag.app.engine.logic.CapabilityHex
import dev.nimrag.app.engine.logic.UploadPolicy
import dev.nimrag.app.model.AppState
import dev.nimrag.app.model.ServerTest
import dev.nimrag.app.model.WatchController
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.withContext

/** Who asked to start the watch service; written to the log so the path that ran is known. */
enum class StartSource(val label: String) {
    ProcessStart("app process start"),
    Boot("boot or app update"),
    CompanionPresence("companion presence (the watch came into range)"),
    Permissions("permissions changed"),
}

/**
 * The engine behind `AppGraph.controller`: publishes one [AppState] and turns the UI's calls
 * into work on the engine's HandlerThread (BLE link) or the upload thread. Created once by
 * `NimragApplication`; the foreground [WatchService] keeps the process alive and starts and
 * stops the link.
 *
 * Deviation from the plan's wording: the link objects live here, in the process, not inside
 * the service instance, so the UI's calls work while the service is not running (nothing
 * to do then) and a service restart does not lose the state. The service owns the
 * *lifecycle* (the link only runs while it does).
 */
class WatchEngine(private val hub: Hub) : WatchController {
    private val scanner = BleScanner(hub)
    private val link = WatchLink(hub, scanner)

    override val state: StateFlow<AppState> get() = hub.state

    /** `Application.onCreate`: never blocks the main thread on IO. */
    fun onProcessStart() {
        // The remembered watch is shown at once (one SharedPreferences read), so the UI never
        // flashes "no watch" at start. Everything else waits for the HandlerThread.
        hub.update { it.copy(settings = hub.settings.toState()) }
        link.restore()
        hub.handler.post {
            hub.log.load()
            hub.refreshPermissions()
            startServiceIfPaired(StartSource.ProcessStart)
        }
        hub.uploader.start()
    }

    /**
     * Process start, boot, the companion presence service: run the service when a watch is
     * remembered. Since Android 12 a background start can be refused (the process was killed
     * and nothing in the foreground started it). Then a normal notification asks the user to
     * open the app, which can start the service from the foreground. A watch with a companion
     * association is brought back by [WatchPresenceService], which is exempt from the rule.
     */
    fun startServiceIfPaired(source: StartSource = StartSource.ProcessStart) {
        val watch = hub.settings.watch ?: return
        if (!hub.hasBluetoothPermissions()) return
        val association = if (hub.companion.isAssociated(watch.address)) "with" else "without"
        if (WatchService.start(hub.app)) {
            ReopenNotice.cancel(hub.app)
            hub.log.info("Watch service started by ${source.label}, $association companion association")
        } else {
            hub.log.warn("Android refused to start the watch service from ${source.label}, $association companion association")
            ReopenNotice.post(hub.app)
        }
    }

    // ---- companion association dialog (MainActivity launches it) ----

    /** The system's association dialog, when one waits to be shown; null otherwise. */
    val associationChooser: StateFlow<IntentSender?> get() = hub.companion.chooser

    /** MainActivity started the dialog from [associationChooser]. */
    fun onAssociationLaunched() = hub.companion.chooserLaunched()

    /** The dialog finished: [ok] is `RESULT_OK`. */
    fun onAssociationResult(ok: Boolean) {
        hub.handler.post { hub.companion.onChooserResult(ok) }
    }

    /** From [WatchService.onStartCommand]; [onResult] false means there is nothing to keep alive. */
    fun onServiceStarted(onResult: (Boolean) -> Unit) {
        hub.handler.post { onResult(link.start()) }
    }

    fun onServiceStopped() {
        hub.handler.post { link.stop() }
    }

    // ---- WatchController ----

    override fun refreshPermissions() {
        hub.handler.post {
            hub.refreshPermissions()
            startServiceIfPaired(StartSource.Permissions)
        }
    }

    override fun startScan() {
        hub.handler.post { scanner.start() }
    }

    override fun stopScan() {
        hub.handler.post { scanner.stop() }
    }

    override fun pair(address: String) {
        val found = hub.state.value.scan.found.firstOrNull { it.address == address }
        hub.handler.post {
            link.pair(
                address = address,
                name = found?.name ?: "Garmin watch",
                model = found?.model ?: "Garmin device",
                unsupportedReason = found?.unsupportedReason,
            )
        }
    }

    override fun forget() {
        hub.handler.post {
            scanner.stop()
            link.forget()
        }
    }

    override fun syncNow() {
        hub.handler.post { link.syncNow() }
    }

    override fun uploadNow() {
        hub.uploader.uploadNow()
    }

    override fun saveServer(url: String, token: String?, allowMobileData: Boolean) {
        val trimmed = url.trim()
        // An invalid address is kept as typed (the settings screen shows it); uploads stay off.
        hub.settings.serverUrl = UploadPolicy.normalizeServerUrl(trimmed) ?: trimmed
        // The token goes into an HTTP header: printable ASCII only. Surrounding whitespace from a
        // paste is trimmed; anything else invalid keeps the old token and says so (no secrets).
        var tokenRejected = false
        if (token != null) {
            val clean = token.trim()
            if (token.isEmpty()) {
                hub.settings.token = null
            } else if (UploadPolicy.isValidToken(clean)) {
                hub.settings.token = clean
            } else {
                tokenRejected = true
                hub.log.warn("token rejected: invalid characters")
            }
        }
        hub.settings.allowMobileData = allowMobileData
        publishSettings()
        hub.uploader.onSettingsChanged()
        hub.log.info(
            when {
                UploadPolicy.normalizeServerUrl(trimmed) == null -> "Server settings saved, uploads are off"
                tokenRejected -> "Server settings saved, the old token was kept"
                else -> "Server settings saved"
            },
        )
    }

    override suspend fun testServer(): ServerTest = withContext(Dispatchers.IO) { hub.uploader.testServer() }

    override fun setAutoSyncMinutes(minutes: Int) {
        hub.settings.autoSyncMinutes = minutes.coerceIn(0, MAX_AUTO_SYNC_MINUTES)
        publishSettings()
        hub.handler.post { link.onAutoSyncChanged() }
    }

    override fun setCapabilitiesHex(hex: String): Boolean {
        val normal = CapabilityHex.normalize(hex) ?: return false
        hub.settings.capabilitiesHex = normal
        publishSettings()
        hub.log.info("Capability mask saved, used from the next connection")
        return true
    }

    private fun publishSettings() = hub.update { it.copy(settings = hub.settings.toState()) }

    private companion object {
        const val MAX_AUTO_SYNC_MINUTES = 24 * 60
    }
}
