package dev.nimrag.app.engine

import android.Manifest
import android.app.Application
import android.bluetooth.BluetoothManager
import android.content.pm.PackageManager
import android.os.Build
import android.os.Handler
import android.os.HandlerThread
import dev.nimrag.app.engine.ble.CompanionAssociation
import dev.nimrag.app.engine.store.EngineDb
import dev.nimrag.app.engine.store.EngineLog
import dev.nimrag.app.engine.store.FileStore
import dev.nimrag.app.engine.upload.UploadWorker
import dev.nimrag.app.model.AppState
import dev.nimrag.app.model.Permissions
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
    val uploader = UploadWorker(
        context = app,
        db = db,
        store = store,
        settings = settings,
        log = log,
        onState = { uploads -> update { it.copy(uploads = uploads) } },
    )

    fun update(block: (AppState) -> AppState) = flow.update(block)

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

    /** Re-reads the runtime permissions and the adapter state into `AppState.permissions`. */
    fun refreshPermissions() {
        val permissions = Permissions(
            bluetooth = hasBluetoothPermissions(),
            notifications = hasNotificationPermission(),
            bluetoothEnabled = bluetoothEnabled(),
        )
        val garminConnect = garminConnectInstalled()
        update { it.copy(permissions = permissions, garminConnectInstalled = garminConnect) }
    }

    /** Needs the `<queries>` entry in the manifest to see the package. */
    fun garminConnectInstalled(): Boolean = try {
        app.packageManager.getPackageInfo(GARMIN_CONNECT_PACKAGE, 0)
        true
    } catch (_: PackageManager.NameNotFoundException) {
        false
    }

    private fun granted(permission: String) =
        app.checkSelfPermission(permission) == PackageManager.PERMISSION_GRANTED

    companion object {
        const val GARMIN_CONNECT_PACKAGE = "com.garmin.android.apps.connectmobile"
    }
}
