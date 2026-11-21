package dev.nimrag.app.model

import dev.nimrag.app.engine.local.LocalWeb
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
    /** Port of the server inside the app on 127.0.0.1 once it runs (personal flavor), else null. */
    val localWebPort: Int? = null,
    /**
     * Counts the times new watch data reached a server the web page may show: an upload pass that
     * took at least one file, or (personal flavor) the phone's own server taking files. The web
     * view tells the page on every change (`nimrag:data`), so it fetches again.
     */
    val dataVersion: Int = 0,
    /** What the system allows the phone features (Watch tab, "Phone features") to do right now. */
    val phoneAccess: PhoneAccess = PhoneAccess(),
    /** What the phone features are doing right now. */
    val phone: PhoneStatus = PhoneStatus(),
)

/**
 * The phone features of stage 2f: what the phone does for the watch besides syncing. Every
 * switch is off until the owner turns it on (IMPLEMENTATION_PLAN.md, stage 2f).
 */
data class PhoneFeatures(
    /** Show the phone's notifications on the watch (all apps except the ones in [excludedApps]). */
    val notifications: Boolean = false,
    /** Control the phone's music from the watch and show what plays. */
    val music: Boolean = false,
    /** Let the watch ring the phone. */
    val findPhone: Boolean = false,
    /** Answer the watch's weather widget: the Nimrag server gets the forecast from Open-Meteo. */
    val weather: Boolean = false,
    /**
     * GPS assist: the server downloads satellite data from Garmin and the phone gives it to the watch,
     * so the watch finds GPS faster. Nimrag's only contact with Garmin is that download.
     */
    val gpsAssist: Boolean = false,
    /** When satellite data last reached the watch (unix ms); 0 = never. Kept across restarts. */
    val gpsAssistLastSentMs: Long = 0L,
    /** Package names whose notifications stay on the phone. */
    val excludedApps: Set<String> = emptySet(),
)

/** What the system lets the phone features do (re-read on every resume). */
data class PhoneAccess(
    /** "Notification access" for this app: needed to read notifications and the music player. */
    val notificationAccess: Boolean = false,
    /** Android 14+ lets the owner turn off full-screen alerts per app; without it "Found it" is a heads-up. */
    val fullScreenAlerts: Boolean = true,
)

/** The live state of the phone features. */
data class PhoneStatus(
    /** The phone is ringing because the watch asked. */
    val findPhoneRinging: Boolean = false,
    val findWatch: FindWatchUi = FindWatchUi.Idle,
    /** The weather feature since the app started (held in memory only). */
    val weather: WeatherStatus = WeatherStatus(),
)

/** When the watch last took a weather forecast and what happened to the latest request. */
data class WeatherStatus(
    /** Unix milliseconds of the last forecast the watch took, or null. */
    val sentAtMs: Long? = null,
    val last: WeatherLast = WeatherLast.None,
)

/** What the Phone features page says about the latest weather request. */
enum class WeatherLast {
    /** No request since the app started. */
    None,

    /** The watch took a fresh forecast. */
    Sent,

    /** The watch took an earlier forecast because the server could not be reached. */
    Reused,

    /** The server gave nothing and there was no earlier forecast for this area. */
    NoForecast,

    /** The watch asked without a usable position. */
    NoPosition,

    /** The forecast was ready but the watch did not take it. */
    NotTaken,
}

/** Progress of "Find my watch" as the Phone features page shows it. */
enum class FindWatchUi {
    Idle,

    /** Asked, the watch has not answered yet. */
    Asking,
    Ringing,

    /** The watch does not offer it. */
    NotSupported,
    Failed,
}

/** An installed app the owner can exclude from the watch's notifications. */
data class AppChoice(val packageName: String, val label: String)

enum class GarminConnect {
    /** Not installed. */
    Absent,

    /** Installed with Bluetooth access ("Nearby devices"): it takes the watch whenever it runs. */
    HasBluetooth,

    /** Installed, Bluetooth access turned off: it cannot reach the watch, Nimrag can. */
    BluetoothOff,
}

/** How Nimrag shares the watch with Garmin Connect (IMPLEMENTATION_PLAN.md, stage 2g). */
enum class GarminConnectMode {
    /** Nimrag is the only sync app: it archives each file on the watch after storing it. */
    Off,

    /** Garmin Connect keeps syncing too: Nimrag copies every file and leaves it for Connect. */
    Alongside,
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
    /** Alongside: files copied in the last 60 days (the skip list); Garmin Connect may have archived some since. */
    val leftForGarminConnect: Int = 0,
)

data class SyncResult(
    val finishedAtMs: Long,
    val downloaded: Int,
    val archived: Int,
    val failed: Int,
    /** Files left alone because they were copied before (Alongside). */
    val skipped: Int = 0,
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
    /** True when this phone holds an upload token from its sign-in (the token itself never goes into state). */
    val hasToken: Boolean = false,
    /** Upload over mobile data too (default: Wi-Fi, Ethernet or VPN only). */
    val allowMobileData: Boolean = false,
    /** Minutes between automatic sync requests while connected; 0 = only when the watch asks. */
    val autoSyncMinutes: Int = 15,
    /** Our 5050 capability mask as hex (advanced; see IMPLEMENTATION_PLAN.md 2c). */
    val capabilitiesHex: String = "",
    /** Takes effect on the next connection; changing it reconnects. */
    val garminConnectMode: GarminConnectMode = GarminConnectMode.Off,
    /** The phone features (stage 2f); changing a switch reconnects the watch once. */
    val phone: PhoneFeatures = PhoneFeatures(),
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

    /** Sign-in, unlock and sign-out with the server's accounts. */
    val account: AccountController get() = NoAccount

    /** Re-read permissions and the adapter state (call after a permission dialog). */
    fun refreshPermissions()

    fun startScan()
    fun stopScan()

    /** Bond with a found device, remember it and connect (first connection after bonding). */
    fun pair(address: String)

    /** Forget the watch: disconnect, remove the bond, clear remembered state. Stored files stay. */
    fun forget()

    /**
     * The app came to the foreground (every `ON_RESUME`): starts a sync when the link is ready and
     * the last one is older than 5 minutes. Does nothing otherwise; never starts the service.
     */
    fun appOpened() {}

    /** Ask the watch for a sync now (Input.StartSync); also refreshes the battery. */
    fun syncNow()

    /** Retry pending uploads now, ignoring the backoff (still obeys the network rule). */
    fun uploadNow()

    /** Upload over mobile data too. The server address and the token come from signing in ([account]). */
    fun setAllowMobileData(allow: Boolean)

    /** GET /api/health, then an authenticated request, off the main thread. */
    suspend fun testServer(): ServerTest

    fun setAutoSyncMinutes(minutes: Int)

    /** Validated hex; takes effect on the next connection. Returns false if invalid. */
    fun setCapabilitiesHex(hex: String): Boolean

    /** Saved; a connected watch is reconnected so the new mode applies at once. */
    fun setGarminConnectMode(mode: GarminConnectMode)

    /** Notifications on the watch (default off). Reconnects the watch once to tell it. */
    fun setPhoneNotifications(on: Boolean) {}

    /** Music control (default off). Reconnects the watch once to tell it. */
    fun setPhoneMusic(on: Boolean) {}

    /** Let the watch ring the phone (default off). Reconnects the watch once to tell it. */
    fun setPhoneFindPhone(on: Boolean) {}

    /** Weather on the watch (default off). Reconnects the watch once to tell it. */
    fun setPhoneWeather(on: Boolean) {}
    /** GPS assist (default off). Reconnects the watch once to tell it. */
    fun setPhoneGpsAssist(on: Boolean) {}

    /** Keep (or stop keeping) one app's notifications on the phone. */
    fun setAppExcluded(packageName: String, excluded: Boolean) {}

    /** Apps with a launcher icon, A to Z, for the exclusion list. Off the main thread. */
    suspend fun launchableApps(): List<AppChoice> = emptyList()

    /** Make the watch ring (`start`) or stop it. The answer is [PhoneStatus.findWatch]. */
    fun findMyWatch(start: Boolean) {}

    /** "Found it" in the app: stop the phone ringing. */
    fun stopFindPhone() {}

    /** How to reach the server inside the app (personal flavor), once [AppState.localWebPort] is set. */
    fun localWeb(): LocalWeb? = null

    /** Which web source the owner chose (debug override of the automatic rule). */
    fun webMode(): WebMode = WebMode.Auto
}

/** The account of a controller that has none (previews, the UI-only demo): signed in, nothing to do. */
private val NoAccount: AccountController = FixedAccountController()

/** Where the web view loads from: see `WebSourcePolicy`. */
enum class WebMode { Auto, Local, Remote }

/** Set once in Application.onCreate by the engine; read by the UI. */
object AppGraph {
    lateinit var controller: WatchController
}
