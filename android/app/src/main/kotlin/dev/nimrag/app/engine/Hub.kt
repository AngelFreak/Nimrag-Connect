package dev.nimrag.app.engine

import android.Manifest
import android.app.Application
import android.bluetooth.BluetoothManager
import android.app.NotificationManager
import android.content.ComponentName
import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import android.os.Handler
import android.os.HandlerThread
import dev.nimrag.app.engine.auth.AccountManager
import dev.nimrag.app.engine.auth.AuthStore
import dev.nimrag.app.engine.auth.SecretVault
import dev.nimrag.app.engine.auth.WebSession
import dev.nimrag.app.engine.ble.CompanionAssociation
import dev.nimrag.app.engine.gps.GpsAssist
import dev.nimrag.app.engine.local.LocalServerFactory
import dev.nimrag.app.engine.local.LocalServerHost
import dev.nimrag.app.engine.phone.NimragNotificationListener
import dev.nimrag.app.engine.phone.PhoneBridge
import dev.nimrag.app.engine.phone.WeatherBridge
import dev.nimrag.app.engine.store.EngineDb
import dev.nimrag.app.engine.store.EngineLog
import dev.nimrag.app.engine.store.FileStore
import dev.nimrag.app.engine.upload.UploadWorker
import dev.nimrag.app.engine.workouts.WorkoutSender
import dev.nimrag.app.model.AppState
import dev.nimrag.app.model.GarminConnect
import dev.nimrag.app.model.Permissions
import dev.nimrag.app.model.PhoneAccess
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.update
import java.io.File

/**
 * What the engine's parts share: the one `HandlerThread` (every BLE callback, every call
 * into the protocol client, the store writes and the timers run on it), the published
 * [AppState] and the storage objects. [update] may be called from any thread.
 */
class Hub(val app: Application) {
    val thread: HandlerThread = HandlerThread("nimrag-ble").also { it.start() }
    val handler: Handler = Handler(thread.looper)

    private val flow = MutableStateFlow(AppState())
    val state: StateFlow<AppState> get() = flow

    val settings = EngineSettings(app)
    val companion = CompanionAssociation(this)
    val db = EngineDb(app)
    val log = EngineLog(db) { lines -> update { it.copy(log = lines) } }
    val store = FileStore(File(app.filesDir, "watch"), db)
    val uploader: UploadWorker = UploadWorker(
        context = app,
        db = db,
        store = store,
        settings = settings,
        log = log,
        onState = { uploads -> update { it.copy(uploads = uploads) } },
        // A refused token (revoked on the server, or an old hand-entered one) means: sign in again.
        onTokenRejected = { account.tokenRejected() },
        onUploaded = ::newDataArrived,
    )

    /**
     * The account: first sign-in, unlock, sign out ([AccountManager]). A new token, a new server
     * address or a sign-out reach the uploader through [update] and [UploadWorker.onSettingsChanged].
     */
    val account: AccountManager = AccountManager(
        context = app,
        settings = settings,
        store = AuthStore(app.getSharedPreferences("account", Context.MODE_PRIVATE)),
        vault = SecretVault(app, app.getSharedPreferences("account_vault", Context.MODE_PRIVATE)),
        web = WebSession(app),
        log = log,
        onUploadCredentialsChanged = {
            update { it.copy(settings = settings.toState()) }
            uploader.onSettingsChanged()
        },
    )

    /**
     * The server inside the app (personal flavor; a no-op in public): the same watch files, shown
     * by the web view when the real server is not the better choice. [AppState.localWebPort]
     * is set once it runs.
     */
    val local: LocalServerHost = LocalServerFactory.create(app, db, store, log, onData = ::newDataArrived) { port ->
        update { it.copy(localWebPort = port) }
    }

    /** The phone features (stage 2f): notifications, find my phone, music. Everything on [handler]. */
    val phone = PhoneBridge(this)

    /** Weather for the watch (stage 2f): asks the Nimrag server, network on its own thread, the rest on [handler]. */
    val weather = WeatherBridge(this)

    /** Workouts built on the web, taken to the watch (stage 4a). Network on its own thread, the rest on [handler]. */
    val workouts = WorkoutSender(this)

    /** GPS assist (stage 2f): satellite data for the watch, from the server. Network on its own thread, the rest on [handler]. */
    val gpsAssist = GpsAssist(this)

    fun update(block: (AppState) -> AppState) = flow.update(block)

    /** New watch data reached a server the web page may show; the web view tells the page ([AppState.dataVersion]). Any thread. */
    fun newDataArrived() = update { it.copy(dataVersion = it.dataVersion + 1) }

    /** True when the adapter is switched on. */
    fun bluetoothEnabled(): Boolean = try {
        app.getSystemService(BluetoothManager::class.java)?.adapter?.isEnabled == true
    } catch (_: SecurityException) {
        false
    }

    fun hasBluetoothPermissions(): Boolean = granted(Manifest.permission.BLUETOOTH_SCAN) &&
        granted(Manifest.permission.BLUETOOTH_CONNECT)

    fun hasNotificationPermission(): Boolean =
        Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU || granted(Manifest.permission.POST_NOTIFICATIONS)

    /**
     * Re-reads the runtime permissions and the adapter state into `AppState.permissions`.
     * True when they differ from what was published before.
     */
    fun refreshPermissions(): Boolean {
        val permissions = Permissions(
            bluetooth = hasBluetoothPermissions(),
            notifications = hasNotificationPermission(),
            bluetoothEnabled = bluetoothEnabled(),
        )
        val garminConnect = garminConnect()
        val phoneAccess = phoneAccess()
        var changed = false
        update {
            changed = it.permissions != permissions
            it.copy(permissions = permissions, garminConnect = garminConnect, phoneAccess = phoneAccess)
        }
        return changed
    }

    /**
     * What the system lets the phone features do. "Notification access" is a special access the
     * owner gives in system settings (it cannot be asked for in a dialog).
     */
    fun phoneAccess(): PhoneAccess {
        val manager = app.getSystemService(NotificationManager::class.java)
        val access = try {
            manager?.isNotificationListenerAccessGranted(ComponentName(app, NimragNotificationListener::class.java)) == true
        } catch (_: Exception) {
            false
        }
        val fullScreen = try {
            Build.VERSION.SDK_INT < Build.VERSION_CODES.UPSIDE_DOWN_CAKE || manager?.canUseFullScreenIntent() == true
        } catch (_: Exception) {
            true
        }
        return PhoneAccess(notificationAccess = access, fullScreenAlerts = fullScreen)
    }

    /**
     * Whether Garmin Connect is installed and may use Bluetooth. Force stop does not keep it
     * away (the system restarts it when the watch is in range); revoking its "Nearby devices"
     * permission does. Needs the `<queries>` entry in the manifest to see the package.
     */
    fun garminConnect(): GarminConnect {
        val pm = app.packageManager
        try {
            pm.getPackageInfo(GARMIN_CONNECT_PACKAGE, 0)
        } catch (_: PackageManager.NameNotFoundException) {
            return GarminConnect.Absent
        }
        val connect = pm.checkPermission(Manifest.permission.BLUETOOTH_CONNECT, GARMIN_CONNECT_PACKAGE)
        return if (connect == PackageManager.PERMISSION_GRANTED) GarminConnect.HasBluetooth else GarminConnect.BluetoothOff
    }

    private fun granted(permission: String) =
        app.checkSelfPermission(permission) == PackageManager.PERMISSION_GRANTED

    companion object {
        const val GARMIN_CONNECT_PACKAGE = "com.garmin.android.apps.connectmobile"
    }
}
