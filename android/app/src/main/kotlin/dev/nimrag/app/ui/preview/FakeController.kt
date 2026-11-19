package dev.nimrag.app.ui.preview

import dev.nimrag.app.model.AppState
import dev.nimrag.app.model.GarminConnectMode
import dev.nimrag.app.model.LinkState
import dev.nimrag.app.model.LogLine
import dev.nimrag.app.model.ServerTest
import dev.nimrag.app.model.SyncResult
import dev.nimrag.app.model.SyncState
import dev.nimrag.app.model.WatchController
import dev.nimrag.app.model.WatchState
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/**
 * A [WatchController] with scripted behaviour, for previews and for the demo shell that opens when
 * no engine is installed. Scan, pairing, sync and uploads play out with short delays; nothing
 * touches Bluetooth, the network or storage.
 */
class FakeController(initial: AppState = PreviewData.notPaired()) : WatchController {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private val mutable = MutableStateFlow(initial)
    override val state: StateFlow<AppState> = mutable.asStateFlow()

    private fun log(level: LogLine.Level, text: String) = mutable.update {
        it.copy(log = (listOf(LogLine(System.currentTimeMillis(), level, text)) + it.log).take(500))
    }

    override fun refreshPermissions() = Unit

    override fun startScan() {
        mutable.update { it.copy(scan = it.scan.copy(scanning = true, found = emptyList())) }
        scope.launch {
            for (device in PreviewData.foundDevices) {
                delay(900)
                mutable.update { s -> if (s.scan.scanning) s.copy(scan = s.scan.copy(found = s.scan.found + device)) else s }
            }
        }
    }

    override fun stopScan() {
        mutable.update { it.copy(scan = it.scan.copy(scanning = false)) }
    }

    override fun pair(address: String) {
        val device = PreviewData.foundDevices.firstOrNull { it.address == address } ?: return
        mutable.update { it.copy(watch = WatchState.Pairing(address, device.name), scan = it.scan.copy(scanning = false)) }
        scope.launch {
            delay(2500)
            val now = System.currentTimeMillis()
            mutable.update {
                it.copy(
                    watch = PreviewData.paired(now, LinkState.Connecting).copy(name = device.name, model = device.model, address = address),
                )
            }
            delay(1500)
            mutable.update { s ->
                val w = s.watch
                if (w is WatchState.Paired) s.copy(watch = w.copy(link = LinkState.Ready, lastConnectedAtMs = System.currentTimeMillis())) else s
            }
        }
    }

    override fun forget() {
        mutable.update { it.copy(watch = WatchState.NotPaired, sync = SyncState()) }
    }

    override fun syncNow() {
        if (mutable.value.sync.running) return
        scope.launch {
            val total = 4
            mutable.update { it.copy(sync = it.sync.copy(running = true, filesInCycle = null, filesDone = 0)) }
            delay(800)
            mutable.update { it.copy(sync = it.sync.copy(filesInCycle = total)) }
            for (i in 0 until total) {
                val size = 200_000L * (i + 1)
                for (step in 0..10) {
                    mutable.update { it.copy(sync = it.sync.copy(filesDone = i, currentSize = size, currentBytes = size * step / 10)) }
                    delay(120)
                }
            }
            val now = System.currentTimeMillis()
            mutable.update {
                it.copy(
                    sync = if (it.settings.garminConnectMode == GarminConnectMode.Alongside) {
                        SyncState(
                            lastResult = SyncResult(now, downloaded = total, archived = 0, failed = 0),
                            leftForGarminConnect = it.sync.leftForGarminConnect + total,
                        )
                    } else {
                        SyncState(lastResult = SyncResult(now, downloaded = total, archived = total, failed = 0))
                    },
                    uploads = it.uploads.copy(pending = it.uploads.pending + total, storedTotal = it.uploads.storedTotal + total),
                )
            }
            log(LogLine.Level.Info, "Demo sync finished: $total files")
        }
    }

    override fun uploadNow() {
        scope.launch {
            delay(1000)
            mutable.update {
                it.uploads.let { u ->
                    it.copy(
                        uploads = u.copy(
                            pending = 0,
                            uploadedTotal = u.storedTotal,
                            lastUploadAtMs = System.currentTimeMillis(),
                            lastError = null,
                            waitingForNetwork = false,
                            recent = u.recent.map { f -> f.copy(uploadedAtMs = f.uploadedAtMs ?: System.currentTimeMillis()) },
                        ),
                    )
                }
            }
        }
    }

    override fun saveServer(url: String, token: String?, allowMobileData: Boolean) {
        mutable.update {
            it.copy(
                settings = it.settings.copy(
                    serverUrl = url,
                    hasToken = when (token) {
                        null -> it.settings.hasToken
                        else -> token.isNotEmpty()
                    },
                    allowMobileData = allowMobileData,
                ),
            )
        }
    }

    override suspend fun testServer(): ServerTest {
        delay(900)
        val s = mutable.value.settings
        return when {
            s.serverUrl.isBlank() -> ServerTest.Failed("No server URL saved")
            !s.hasToken -> ServerTest.Failed("HTTP 401: no token")
            else -> ServerTest.Ok
        }
    }

    override fun setAutoSyncMinutes(minutes: Int) {
        mutable.update { it.copy(settings = it.settings.copy(autoSyncMinutes = minutes)) }
    }

    override fun setCapabilitiesHex(hex: String): Boolean {
        val valid = hex.isNotEmpty() && hex.length % 2 == 0 && hex.all { it in '0'..'9' || it in 'a'..'f' || it in 'A'..'F' }
        if (valid) mutable.update { it.copy(settings = it.settings.copy(capabilitiesHex = hex)) }
        return valid
    }

    override fun setGarminConnectMode(mode: GarminConnectMode) {
        mutable.update { it.copy(settings = it.settings.copy(garminConnectMode = mode)) }
        log(LogLine.Level.Info, "Demo: Garmin Connect mode ${mode.name}")
    }

    companion object {
        /** One shared demo instance so it survives activity recreation. */
        val demo: FakeController by lazy { FakeController(PreviewData.notPaired()) }
    }
}
