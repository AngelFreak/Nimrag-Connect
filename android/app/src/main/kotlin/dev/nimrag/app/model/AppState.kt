package dev.nimrag.app.model

import kotlinx.coroutines.flow.StateFlow

/*
 * Contract between the engine (BLE link, foreground service, storage, uploads)
 * and the UI. The engine implements [WatchController] and publishes one
 * [AppState]; the UI only reads the state and calls the controller. Changing
 * this file is the integrator's call (IMPLEMENTATION_PLAN.md, stages 2c–2e).
 */

/** Everything the screens show. Immutable; the engine replaces it on every change. */
data class AppState(
    val permissions: Permissions = Permissions(),
    val watch: WatchState = WatchState.NotPaired,
    val scan: ScanState = ScanState(),
    val sync: SyncState = SyncState(),
    val uploads: UploadState = UploadState(),
    val settings: Settings = Settings(),
    /** Newest first, at most 500 lines. Never contains the watch's unit id. */
    val log: List<LogLine> = emptyList(),
    /** Why the last pairing attempt failed, shown on the Watch screen; cleared by the next scan or pairing. */
    val pairingError: String? = null,
    /**
     * Garmin Connect on this phone. Only one app can hold the watch's protocol session, so the
     * Watch screen explains how to keep it out of the way.
     */
    val garminConnect: GarminConnect = GarminConnect.Absent,
)

enum class GarminConnect {
    /** Not installed. */
    Absent,

    /** Installed with Bluetooth access ("Nearby devices"): it takes the watch whenever it runs. */
    HasBluetooth,

    /** Installed, Bluetooth access turned off: it cannot reach the watch, Nimrag can. */
    BluetoothOff,
}

/** Runtime permissions the engine needs; the UI asks for the missing ones. */
data class Permissions(
    val bluetooth: Boolean = false, // BLUETOOTH_SCAN + BLUETOOTH_CONNECT
    val notifications: Boolean = false, // POST_NOTIFICATIONS (foreground service notice)
    val bluetoothEnabled: Boolean = false, // the adapter is switched on
)

sealed interface WatchState {
    /** No watch remembered: the Watch screen offers scanning and pairing. */
    data object NotPaired : WatchState

    /** Bonding with [address] is in progress (the system pairing dialog may be showing). */
    data class Pairing(val address: String, val name: String) : WatchState

    /** A remembered watch. */
    data class Paired(
        val address: String,
        val name: String,
        /** Model name from the device catalogue, e.g. "Instinct 2". */
        val model: String,
        val link: LinkState,
        /** From HandshakeDone; null until the first handshake on this install. */
        val softwareVersion: String? = null,
        /** 0–100, from Event.Battery; null until known. */
        val batteryPercent: Int? = null,
        val batteryAtMs: Long? = null,
        /** Wall-clock ms of the last finished handshake. */
        val lastConnectedAtMs: Long? = null,
        /** The catalogue says this model cannot sync yet (reason), else null. */
        val unsupportedReason: String? = null,
    ) : WatchState
}

enum class LinkState {
    /** Service stopped or Bluetooth off. */
    Offline,
    /** Waiting for the watch to come in range (background auto-connect). */
    Waiting,
    /** GATT up; multi-link bring-up and handshake running. */
    Connecting,
    /** Handshake done; idle or syncing. */
    Ready,
    /**
     * The phone no longer holds a bond with the watch (removed in system settings, or the
     * watch was reset). The engine stops retrying; the UI offers "Pair again" (forget, then scan).
     */
    BondLost,
}

data class ScanState(
    val scanning: Boolean = false,
    /** Garmin devices seen in this scan, strongest signal first. */
    val found: List<FoundDevice> = emptyList(),
)

data class FoundDevice(
    val address: String,
    /** Advertised name, or the model name when the advertisement has none. */
    val name: String,
    val model: String,
    val rssi: Int,
    /** Null when the catalogue says the model can sync. */
    val unsupportedReason: String? = null,
)

data class SyncState(
    val running: Boolean = false,
    /** Files in the current cycle's listing (null before the listing arrives). */
    val filesInCycle: Int? = null,
    val filesDone: Int = 0,
    /** Bytes of the file being downloaded / its size, for a progress bar. */
    val currentBytes: Long = 0,
    val currentSize: Long = 0,
    val lastResult: SyncResult? = null,
)

data class SyncResult(
    val finishedAtMs: Long,
    val downloaded: Int,
    val archived: Int,
    val failed: Int,
)

data class UploadState(
    /** Stored on the phone, not yet accepted by the server. */
    val pending: Int = 0,
    val uploadedTotal: Int = 0,
    val storedTotal: Int = 0,
    val lastUploadAtMs: Long? = null,
    /** Last failure in words (HTTP status or network error), cleared on success. */
    val lastError: String? = null,
    /** True while uploads wait because the network rule does not allow them. */
    val waitingForNetwork: Boolean = false,
    /** Most recent stored files, newest first, at most 50. */
    val recent: List<StoredFile> = emptyList(),
)

data class StoredFile(
    val sha256: String,
    val sizeBytes: Long,
    /** "FIT 128/4" style label plus a name where known (activity, monitoring, sleep, …). */
    val kind: String,
    /** Watch timestamp as wall-clock ms, or null if the watch gave none. */
    val watchTimeMs: Long?,
    val receivedAtMs: Long,
    val uploadedAtMs: Long?,
)

data class Settings(
    /** e.g. "https://nimrag.example.ts.net"; blank = uploads off. */
    val serverUrl: String = "",
    /** True when a device token is saved (the token itself never goes into state). */
    val hasToken: Boolean = false,
    /** Upload over mobile data too (default: Wi-Fi, Ethernet or VPN only). */
    val allowMobileData: Boolean = false,
    /** Minutes between automatic sync requests while connected; 0 = only when the watch asks. */
    val autoSyncMinutes: Int = 60,
    /** Our 5050 capability mask as hex (advanced; see IMPLEMENTATION_PLAN.md 2c). */
    val capabilitiesHex: String = "",
)

data class LogLine(val atMs: Long, val level: Level, val text: String) {
    enum class Level { Info, Warning, Error }
}

/** Result of [WatchController.testServer]. */
sealed interface ServerTest {
    data object Ok : ServerTest
    data class Failed(val reason: String) : ServerTest
}

interface WatchController {
    val state: StateFlow<AppState>

    /** Re-read permissions and the adapter state (call after a permission dialog). */
    fun refreshPermissions()

    fun startScan()
    fun stopScan()

    /** Bond with a found device, remember it and connect (first connection after bonding). */
    fun pair(address: String)

    /** Forget the watch: disconnect, remove the bond, clear remembered state. Stored files stay. */
    fun forget()

    /** Ask the watch for a sync now (Input.StartSync); also refreshes the battery. */
    fun syncNow()

    /** Retry pending uploads now, ignoring the backoff (still obeys the network rule). */
    fun uploadNow()

    /** [token] null keeps the saved one; "" removes it. */
    fun saveServer(url: String, token: String?, allowMobileData: Boolean)

    /** GET /api/health, then an authenticated request, off the main thread. */
    suspend fun testServer(): ServerTest

    fun setAutoSyncMinutes(minutes: Int)

    /** Validated hex; takes effect on the next connection. Returns false if invalid. */
    fun setCapabilitiesHex(hex: String): Boolean
}

/** Set once in Application.onCreate by the engine; read by the UI. */
object AppGraph {
    lateinit var controller: WatchController
}
