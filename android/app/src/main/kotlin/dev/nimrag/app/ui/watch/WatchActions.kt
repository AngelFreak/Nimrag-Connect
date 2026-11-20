package dev.nimrag.app.ui.watch

import android.Manifest
import android.bluetooth.BluetoothAdapter
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.provider.Settings
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.platform.LocalContext
import dev.nimrag.app.model.WatchController
import dev.nimrag.app.ui.shell.findActivity

private const val GARMIN_CONNECT_PACKAGE = "com.garmin.android.apps.connectmobile"

/**
 * The system things the Watch page can send the person to: the Bluetooth and notification
 * permission dialogs, the "turn Bluetooth on" prompt and two app-info screens.
 */
internal class WatchActions(
    val allowBluetooth: () -> Unit,
    val allowNotifications: () -> Unit,
    val turnOnBluetooth: () -> Unit,
    val openGarminConnectInfo: () -> Unit,
)

@Composable
internal fun rememberWatchActions(controller: WatchController): WatchActions {
    val context = LocalContext.current

    // True once we have asked. When the dialog was refused for good, Android shows nothing any
    // more, so the next tap opens the app settings instead.
    var askedOnce by rememberSaveable { mutableStateOf(false) }
    val permissionLauncher = rememberLauncherForActivityResult(
        ActivityResultContracts.RequestMultiplePermissions(),
    ) {
        askedOnce = true
        controller.refreshPermissions()
    }
    val enableBluetoothLauncher = rememberLauncherForActivityResult(
        ActivityResultContracts.StartActivityForResult(),
    ) { controller.refreshPermissions() }

    return remember(context, controller) {
        fun openAppInfo(packageName: String) {
            context.startActivity(
                Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.fromParts("package", packageName, null))
                    .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
            )
        }

        fun request(permissions: List<String>) {
            val activity = context.findActivity()
            val refusedForGood = askedOnce && activity != null &&
                permissions.none { activity.shouldShowRequestPermissionRationale(it) }
            if (refusedForGood) openAppInfo(context.packageName) else permissionLauncher.launch(permissions.toTypedArray())
        }

        WatchActions(
            allowBluetooth = { request(listOf(Manifest.permission.BLUETOOTH_SCAN, Manifest.permission.BLUETOOTH_CONNECT)) },
            allowNotifications = {
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                    request(listOf(Manifest.permission.POST_NOTIFICATIONS))
                } else {
                    openAppInfo(context.packageName)
                }
            },
            turnOnBluetooth = { enableBluetoothLauncher.launch(Intent(BluetoothAdapter.ACTION_REQUEST_ENABLE)) },
            openGarminConnectInfo = { openAppInfo(GARMIN_CONNECT_PACKAGE) },
        )
    }
}
