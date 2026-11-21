package dev.nimrag.app.engine

import android.content.IntentSender
import android.os.SystemClock
import dev.nimrag.app.engine.ble.BleScanner
import dev.nimrag.app.engine.ble.WatchLink
import dev.nimrag.app.engine.logic.CapabilityHex
import dev.nimrag.app.engine.logic.ServiceStartGate
import dev.nimrag.app.engine.logic.StartDecision
import dev.nimrag.app.engine.logic.StartRequest
import dev.nimrag.app.engine.logic.UploadPolicy
import dev.nimrag.app.engine.local.LocalWeb
import dev.nimrag.app.engine.phone.PhoneBridge
import dev.nimrag.app.model.AccountController
import dev.nimrag.app.model.AppChoice
import dev.nimrag.app.model.AppState
import dev.nimrag.app.model.GarminConnectMode
import dev.nimrag.app.model.ServerTest
import dev.nimrag.app.model.WatchController
import dev.nimrag.app.model.WebMode
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.withContext
import android.content.Intent

/** Who asked to start the watch service; written to the log so the path that ran is known. */
enum class StartSource(val label: String, val request: StartRequest) {
    ProcessStart("app process start", StartRequest.ProcessStart),
    Boot("boot or app update", StartRequest.Boot),
    CompanionPresence("companion presence (the watch came into range)", StartRequest.Presence),

    /** The permissions or the Bluetooth state really changed. */
    Permissions("permissions changed", StartRequest.PermissionsChanged),

    /** The app was brought to the foreground and the service was not running. */
    AppOpened("app opened", StartRequest.AppOpened),
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

    /** One start at a time: see [ServiceStartGate]. */
    private val startGate = ServiceStartGate()

    override val state: StateFlow<AppState> get() = hub.state

    override val account: AccountController get() = hub.account

    /** The phone features, for the notification listener service (it lives in the same process). */
    val phoneBridge: PhoneBridge get() = hub.phone

    /** `Application.onCreate`: never blocks the main thread on IO. */
    fun onProcessStart() {
        // The remembered watch is shown at once (one SharedPreferences read), so the UI never
        // flashes "no watch" at start. Everything else waits for the HandlerThread.
        hub.update { it.copy(settings = hub.settings.toState()) }
        link.restore()
        hub.handler.post {
            hub.log.load()
            link.refreshLeftForGarminConnect()
            hub.refreshPermissions()
            startServiceIfPaired(StartSource.ProcessStart)
        }
        hub.uploader.start()
        hub.local.start()
        // Is the session from the last run still good? (a cheap GET /api/me with the saved cookie)
        hub.account.checkSession()
    }

    override fun localWeb(): LocalWeb? = hub.local.web

    override fun webMode(): WebMode = hub.settings.webMode

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
        when (startGate.decide(source.request, SystemClock.elapsedRealtime())) {
            StartDecision.Skip -> {
                // Opening the app is routine and stays out of the log; a second start from the
                // same boot or update is worth one line.
                if (source != StartSource.AppOpened) {
                    hub.log.info("Watch service start by ${source.label} skipped, it is already running or starting")
                }
                return
            }
            StartDecision.Nudge -> {
                hub.handler.post { link.onWatchPresent() }
                return
            }
            StartDecision.Start -> Unit
        }
        val association = if (hub.companion.isAssociated(watch.address)) "with" else "without"
        if (WatchService.start(hub.app)) {
            ReopenNotice.cancel(hub.app)
            hub.log.info("Watch service started by ${source.label}, $association companion association")
        } else {
            startGate.refused()
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
        startGate.started()
        hub.handler.post { onResult(link.start()) }
    }

    /** The auto-sync alarm went off ([AutoSyncReceiver]); [done] ends the broadcast once the link has looked at it. */
    fun onAutoSyncAlarm(done: () -> Unit) {
        hub.handler.post {
            try {
                link.onAutoSyncAlarm()
            } finally {
                done()
            }
        }
    }

    fun onServiceStopped() {
        startGate.stopped()
        hub.handler.post { link.stop() }
    }

    // ---- WatchController ----

    override fun refreshPermissions() {
        // The UI calls this on every resume. Only a real change counts as "permissions changed";
        // otherwise the service is started only when it is not running (the app was opened).
        hub.handler.post {
            val changed = hub.refreshPermissions()
            startServiceIfPaired(if (changed) StartSource.Permissions else StartSource.AppOpened)
            // The owner may have given or taken away notification access in system settings.
            link.onDutiesChanged()
        }
    }

    /** The notification listener (dis)connected: the system's access changed, so re-read it and apply. */
    fun onNotificationAccessChanged() {
        hub.handler.post {
            hub.refreshPermissions()
            link.onDutiesChanged()
        }
    }

    override fun appOpened() {
        hub.handler.post { link.onAppOpened() }
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

    override fun setAllowMobileData(allow: Boolean) {
        hub.settings.allowMobileData = allow
        publishSettings()
        hub.uploader.onSettingsChanged()
        hub.log.info(if (allow) "Uploads may use mobile data" else "Uploads wait for Wi-Fi, Ethernet or VPN")
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

    override fun setGarminConnectMode(mode: GarminConnectMode) {
        if (hub.settings.garminConnectMode == mode) return
        hub.settings.garminConnectMode = mode
        publishSettings()
        hub.handler.post { link.onGarminConnectModeChanged(mode) }
    }

    override fun setPhoneNotifications(on: Boolean) {
        if (hub.settings.phoneNotifications == on) return
        hub.settings.phoneNotifications = on
        phoneFeatureChanged("Notifications on the watch", on)
    }

    override fun setPhoneMusic(on: Boolean) {
        if (hub.settings.phoneMusic == on) return
        hub.settings.phoneMusic = on
        phoneFeatureChanged("Music control", on)
    }

    override fun setPhoneFindPhone(on: Boolean) {
        if (hub.settings.phoneFindPhone == on) return
        hub.settings.phoneFindPhone = on
        phoneFeatureChanged("Find my phone", on)
    }

    private fun phoneFeatureChanged(name: String, on: Boolean) {
        publishSettings()
        hub.log.info("$name: ${if (on) "on" else "off"}")
        hub.handler.post {
            hub.refreshPermissions()
            link.onDutiesChanged()
        }
    }

    override fun setAppExcluded(packageName: String, excluded: Boolean) {
        hub.phone.setExcluded(packageName, excluded)
    }

    override suspend fun launchableApps(): List<AppChoice> = withContext(Dispatchers.IO) {
        val pm = hub.app.packageManager
        val intent = Intent(Intent.ACTION_MAIN).addCategory(Intent.CATEGORY_LAUNCHER)
        try {
            // The flags overload needs API 33; the plain one works everywhere (minSdk is 31).
            @Suppress("DEPRECATION")
            pm.queryIntentActivities(intent, 0)
                .map { AppChoice(it.activityInfo.packageName, it.loadLabel(pm).toString()) }
                .distinctBy { it.packageName }
                .filter { it.packageName != hub.app.packageName }
                .sortedBy { it.label.lowercase() }
        } catch (_: Exception) {
            emptyList()
        }
    }

    override fun findMyWatch(start: Boolean) {
        hub.handler.post { hub.phone.findWatch(start) }
    }

    override fun stopFindPhone() {
        hub.phone.stopFindPhone()
    }

    private fun publishSettings() = hub.update { it.copy(settings = hub.settings.toState()) }

    private companion object {
        const val MAX_AUTO_SYNC_MINUTES = 24 * 60
    }
}
