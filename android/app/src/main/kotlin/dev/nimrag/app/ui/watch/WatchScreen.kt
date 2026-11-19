package dev.nimrag.app.ui.watch

import android.Manifest
import android.bluetooth.BluetoothAdapter
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.provider.Settings
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Info
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import dev.nimrag.app.R
import dev.nimrag.app.model.AppState
import dev.nimrag.app.model.FoundDevice
import dev.nimrag.app.model.LinkState
import dev.nimrag.app.model.WatchController
import dev.nimrag.app.model.WatchState
import dev.nimrag.app.ui.components.BatteryIndicator
import dev.nimrag.app.ui.components.CardTitle
import dev.nimrag.app.ui.components.DetailRow
import dev.nimrag.app.ui.components.SectionCard
import dev.nimrag.app.ui.components.SignalBars
import dev.nimrag.app.ui.components.StatusChip
import dev.nimrag.app.ui.components.WatchIcon
import dev.nimrag.app.ui.components.WithAbsoluteTime
import dev.nimrag.app.ui.components.MessageCard
import dev.nimrag.app.ui.components.relativeTime
import dev.nimrag.app.ui.theme.status

@Composable
fun WatchScreen(
    state: AppState,
    controller: WatchController,
    contentPadding: PaddingValues,
    modifier: Modifier = Modifier,
) {
    val context = LocalContext.current
    val permissions = state.permissions

    // True once we have asked and something is still missing: Android then often shows no dialog
    // any more, so the card also offers the app settings.
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

    val missing = buildList {
        if (!permissions.bluetooth) {
            add(Manifest.permission.BLUETOOTH_SCAN)
            add(Manifest.permission.BLUETOOTH_CONNECT)
        }
        if (!permissions.notifications && Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            add(Manifest.permission.POST_NOTIFICATIONS)
        }
    }

    Column(
        modifier = modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(contentPadding)
            .padding(horizontal = 16.dp, vertical = 8.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        if (!permissions.bluetooth || !permissions.notifications) {
            PermissionCard(
                needsBluetooth = !permissions.bluetooth,
                needsNotifications = !permissions.notifications,
                askedOnce = askedOnce,
                onGrant = { permissionLauncher.launch(missing.toTypedArray()) },
                onOpenSettings = {
                    context.startActivity(
                        Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.fromParts("package", context.packageName, null)),
                    )
                },
            )
        }
        if (permissions.bluetooth && !permissions.bluetoothEnabled) {
            BluetoothOffCard(onEnable = { enableBluetoothLauncher.launch(Intent(BluetoothAdapter.ACTION_REQUEST_ENABLE)) })
        }
        val watchReady = (state.watch as? WatchState.Paired)?.link == LinkState.Ready
        if (state.garminConnectInstalled && !watchReady) {
            GarminConnectCard(
                onOpenAppInfo = {
                    context.startActivity(
                        Intent(
                            Settings.ACTION_APPLICATION_DETAILS_SETTINGS,
                            Uri.fromParts("package", GARMIN_CONNECT_PACKAGE, null),
                        ),
                    )
                },
            )
        }

        when (val watch = state.watch) {
            WatchState.NotPaired -> NotPairedSection(
                state = state,
                canScan = permissions.bluetooth && permissions.bluetoothEnabled,
                controller = controller,
            )
            is WatchState.Pairing -> PairingCard(watch)
            is WatchState.Paired -> PairedCard(watch, syncRunning = state.sync.running, controller = controller)
        }
    }
}

// ---------------------------------------------------------------- permissions

@Composable
internal fun PermissionCard(
    needsBluetooth: Boolean,
    needsNotifications: Boolean,
    askedOnce: Boolean,
    onGrant: () -> Unit,
    onOpenSettings: () -> Unit,
    modifier: Modifier = Modifier,
) {
    MessageCard(
        icon = Icons.Filled.Info,
        title = stringResource(R.string.permissions_title),
        containerColor = MaterialTheme.colorScheme.secondaryContainer,
        contentColor = MaterialTheme.colorScheme.onSecondaryContainer,
        modifier = modifier,
        actions = {
            Button(onClick = onGrant) { Text(stringResource(R.string.permissions_grant)) }
            if (askedOnce) {
                TextButton(onClick = onOpenSettings) { Text(stringResource(R.string.permissions_open_settings)) }
            }
        },
    ) {
        if (needsBluetooth) {
            Text(stringResource(R.string.permissions_bluetooth_why), style = MaterialTheme.typography.bodyMedium)
        }
        if (needsNotifications) {
            Text(stringResource(R.string.permissions_notifications_why), style = MaterialTheme.typography.bodyMedium)
        }
        if (askedOnce) {
            Text(stringResource(R.string.permissions_denied_hint), style = MaterialTheme.typography.bodySmall)
        }
    }
}

@Composable
internal fun BluetoothOffCard(onEnable: () -> Unit, modifier: Modifier = Modifier) {
    MessageCard(
        icon = Icons.Filled.Warning,
        title = stringResource(R.string.bluetooth_off_title),
        containerColor = MaterialTheme.status.warnContainer,
        contentColor = MaterialTheme.status.onWarnContainer,
        modifier = modifier,
        actions = { Button(onClick = onEnable) { Text(stringResource(R.string.bluetooth_turn_on)) } },
    ) {
        Text(stringResource(R.string.bluetooth_off_body), style = MaterialTheme.typography.bodyMedium)
    }
}

private const val GARMIN_CONNECT_PACKAGE = "com.garmin.android.apps.connectmobile"

/** Garmin Connect holds the watch's session whenever it runs; only one app can talk to the watch. */
@Composable
internal fun GarminConnectCard(onOpenAppInfo: () -> Unit, modifier: Modifier = Modifier) {
    MessageCard(
        icon = Icons.Filled.Info,
        title = stringResource(R.string.garmin_connect_title),
        containerColor = MaterialTheme.status.warnContainer,
        contentColor = MaterialTheme.status.onWarnContainer,
        modifier = modifier,
        actions = { Button(onClick = onOpenAppInfo) { Text(stringResource(R.string.garmin_connect_open)) } },
    ) {
        Text(stringResource(R.string.garmin_connect_body), style = MaterialTheme.typography.bodyMedium)
        Text(stringResource(R.string.garmin_connect_test), style = MaterialTheme.typography.bodyMedium)
        Text(stringResource(R.string.garmin_connect_switch), style = MaterialTheme.typography.bodySmall)
    }
}

@Composable
internal fun PairingFailedCard(reason: String, modifier: Modifier = Modifier) {
    MessageCard(
        icon = Icons.Filled.Warning,
        title = stringResource(R.string.pairing_failed_title),
        containerColor = MaterialTheme.colorScheme.errorContainer,
        contentColor = MaterialTheme.colorScheme.onErrorContainer,
        modifier = modifier,
    ) {
        Text(reason, style = MaterialTheme.typography.bodyMedium)
        Text(stringResource(R.string.pairing_failed_hint), style = MaterialTheme.typography.bodySmall)
    }
}

// ---------------------------------------------------------------- not paired

@Composable
private fun NotPairedSection(state: AppState, canScan: Boolean, controller: WatchController) {
    var pendingDevice by rememberSaveable { mutableStateOf<String?>(null) }
    var scannedOnce by rememberSaveable { mutableStateOf(false) }
    val scan = state.scan

    // Never leave a scan running when this section goes away (tab change, pairing started).
    DisposableEffect(controller) { onDispose { controller.stopScan() } }

    state.pairingError?.let { PairingFailedCard(it) }

    NotPairedContent(
        scanning = scan.scanning,
        found = scan.found,
        canScan = canScan,
        scannedOnce = scannedOnce,
        onScan = {
            scannedOnce = true
            controller.startScan()
        },
        onStop = controller::stopScan,
        onPick = { pendingDevice = it.address },
    )

    val device = scan.found.firstOrNull { it.address == pendingDevice }
    if (device != null) {
        PairConfirmDialog(
            device = device,
            onConfirm = {
                pendingDevice = null
                controller.pair(device.address)
            },
            onDismiss = { pendingDevice = null },
        )
    }
}

@Composable
internal fun NotPairedContent(
    scanning: Boolean,
    found: List<FoundDevice>,
    canScan: Boolean,
    scannedOnce: Boolean,
    onScan: () -> Unit,
    onStop: () -> Unit,
    onPick: (FoundDevice) -> Unit,
) {
    SectionCard {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            Icon(WatchIcon, contentDescription = null, tint = MaterialTheme.colorScheme.primary)
            CardTitle(stringResource(R.string.pair_title))
        }
        Text(
            stringResource(R.string.pair_instructions),
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        if (scanning) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                CircularProgressIndicator(modifier = Modifier.size(24.dp), strokeWidth = 3.dp)
                Text(
                    stringResource(R.string.scan_running),
                    modifier = Modifier.weight(1f),
                    style = MaterialTheme.typography.bodyMedium,
                )
                OutlinedButton(onClick = onStop) { Text(stringResource(R.string.scan_stop)) }
            }
        } else {
            Button(onClick = onScan, enabled = canScan, modifier = Modifier.fillMaxWidth()) {
                Text(stringResource(if (scannedOnce) R.string.scan_again else R.string.scan_start))
            }
            if (!canScan) {
                Text(
                    stringResource(R.string.scan_unavailable),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }

    if (found.isNotEmpty()) {
        Text(
            stringResource(R.string.scan_found_title),
            style = MaterialTheme.typography.titleSmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(start = 4.dp),
        )
        Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
            found.forEach { device -> FoundDeviceRow(device, onClick = { onPick(device) }) }
        }
    } else if (scannedOnce && !scanning) {
        Text(
            stringResource(R.string.scan_none_found),
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(horizontal = 4.dp),
        )
    }
}

@Composable
private fun FoundDeviceRow(device: FoundDevice, onClick: () -> Unit) {
    Card(
        modifier = Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainer),
    ) {
        Row(
            modifier = Modifier.padding(horizontal = 16.dp, vertical = 12.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                Text(device.name, style = MaterialTheme.typography.titleSmall, color = MaterialTheme.colorScheme.onSurface)
                if (device.model != device.name) {
                    Text(
                        device.model,
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                device.unsupportedReason?.let {
                    Text(
                        stringResource(R.string.cannot_sync_yet, it),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.status.warn,
                    )
                }
            }
            SignalBars(device.rssi)
        }
    }
}

@Composable
private fun PairConfirmDialog(device: FoundDevice, onConfirm: () -> Unit, onDismiss: () -> Unit) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.pair_confirm_title, device.name)) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(stringResource(R.string.pair_confirm_body))
                device.unsupportedReason?.let {
                    Text(
                        stringResource(R.string.cannot_sync_yet, it),
                        color = MaterialTheme.status.warn,
                    )
                }
            }
        },
        confirmButton = { TextButton(onClick = onConfirm) { Text(stringResource(R.string.pair_confirm)) } },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.cancel)) } },
    )
}

// ---------------------------------------------------------------- pairing

@Composable
internal fun PairingCard(watch: WatchState.Pairing) {
    SectionCard {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(16.dp)) {
            CircularProgressIndicator(modifier = Modifier.size(32.dp), strokeWidth = 3.dp)
            CardTitle(stringResource(R.string.pairing_title, watch.name))
        }
        Text(
            stringResource(R.string.pairing_hint),
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

// ---------------------------------------------------------------- paired

@Composable
private fun PairedCard(watch: WatchState.Paired, syncRunning: Boolean, controller: WatchController) {
    var confirmForget by rememberSaveable { mutableStateOf(false) }
    PairedCardContent(
        watch = watch,
        syncRunning = syncRunning,
        onSync = controller::syncNow,
        onForget = { confirmForget = true },
        // Lost pairing: forgetting returns the screen to scanning, without a confirmation.
        onPairAgain = controller::forget,
    )
    if (confirmForget) {
        AlertDialog(
            onDismissRequest = { confirmForget = false },
            title = { Text(stringResource(R.string.forget_title, watch.name)) },
            text = { Text(stringResource(R.string.forget_body)) },
            confirmButton = {
                TextButton(
                    onClick = {
                        confirmForget = false
                        controller.forget()
                    },
                    colors = ButtonDefaults.textButtonColors(contentColor = MaterialTheme.colorScheme.error),
                ) { Text(stringResource(R.string.forget_confirm)) }
            },
            dismissButton = { TextButton(onClick = { confirmForget = false }) { Text(stringResource(R.string.cancel)) } },
        )
    }
}

@Composable
internal fun PairedCardContent(
    watch: WatchState.Paired,
    syncRunning: Boolean,
    onSync: () -> Unit,
    onForget: () -> Unit,
    onPairAgain: () -> Unit = {},
) {
    SectionCard {
        Row(verticalAlignment = Alignment.Top, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            Icon(
                WatchIcon,
                contentDescription = null,
                tint = MaterialTheme.colorScheme.primary,
                modifier = Modifier
                    .padding(top = 2.dp)
                    .size(32.dp),
            )
            Column(Modifier.weight(1f)) {
                Text(watch.name, style = MaterialTheme.typography.titleLarge, color = MaterialTheme.colorScheme.onSurface)
                Text(
                    watch.model,
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            LinkChip(watch.link)
        }
        Text(
            linkExplanation(watch.link),
            style = MaterialTheme.typography.bodySmall,
            color = if (watch.link == LinkState.BondLost) {
                MaterialTheme.colorScheme.error
            } else {
                MaterialTheme.colorScheme.onSurfaceVariant
            },
        )
        watch.unsupportedReason?.let {
            Row(verticalAlignment = Alignment.Top, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Icon(Icons.Filled.Warning, contentDescription = null, tint = MaterialTheme.status.warn)
                Text(
                    stringResource(R.string.cannot_sync_yet, it),
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.status.warn,
                )
            }
        }

        DetailRow(stringResource(R.string.label_battery)) { BatteryValue(watch.batteryPercent, watch.batteryAtMs) }
        DetailRow(stringResource(R.string.label_software), watch.softwareVersion ?: stringResource(R.string.unknown))
        DetailRow(stringResource(R.string.label_last_connected)) {
            val at = watch.lastConnectedAtMs
            if (at == null) {
                Text(stringResource(R.string.never), style = MaterialTheme.typography.bodyMedium)
            } else {
                WithAbsoluteTime(at) { Text(relativeTime(at), style = MaterialTheme.typography.bodyMedium) }
            }
        }

        val canSync = watch.link == LinkState.Ready && !syncRunning && watch.unsupportedReason == null
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(top = 4.dp),
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            if (watch.link == LinkState.BondLost) {
                Button(onClick = onPairAgain, modifier = Modifier.weight(1f)) {
                    Text(stringResource(R.string.pair_again))
                }
            } else {
                Button(onClick = onSync, enabled = canSync, modifier = Modifier.weight(1f)) {
                    Text(stringResource(if (syncRunning) R.string.sync_running_button else R.string.sync_now))
                }
            }
            OutlinedButton(
                onClick = onForget,
                modifier = Modifier.weight(1f),
                colors = ButtonDefaults.outlinedButtonColors(contentColor = MaterialTheme.colorScheme.error),
            ) { Text(stringResource(R.string.forget_watch)) }
        }
    }
}

@Composable
private fun linkExplanation(link: LinkState): String = stringResource(
    when (link) {
        LinkState.Offline -> R.string.link_offline_hint
        LinkState.Waiting -> R.string.link_waiting_hint
        LinkState.Connecting -> R.string.link_connecting_hint
        LinkState.Ready -> R.string.link_ready_hint
        LinkState.BondLost -> R.string.link_bond_lost_hint
    },
)

@Composable
internal fun LinkChip(link: LinkState, modifier: Modifier = Modifier) {
    val scheme = MaterialTheme.colorScheme
    val status = MaterialTheme.status
    val label = stringResource(
        when (link) {
            LinkState.Offline -> R.string.link_offline
            LinkState.Waiting -> R.string.link_waiting
            LinkState.Connecting -> R.string.link_connecting
            LinkState.Ready -> R.string.link_ready
            LinkState.BondLost -> R.string.link_bond_lost
        },
    )
    val description = stringResource(R.string.link_state_description, label)
    when (link) {
        LinkState.Offline -> StatusChip(label, scheme.surfaceContainerHighest, scheme.onSurfaceVariant, scheme.outline, modifier.semantics { contentDescription = description })
        LinkState.Waiting -> StatusChip(label, status.warnContainer, status.onWarnContainer, status.warn, modifier.semantics { contentDescription = description })
        LinkState.Connecting -> StatusChip(label, scheme.secondaryContainer, scheme.onSecondaryContainer, scheme.primary, modifier.semantics { contentDescription = description })
        LinkState.Ready -> StatusChip(label, status.goodContainer, status.onGoodContainer, status.good, modifier.semantics { contentDescription = description })
        LinkState.BondLost -> StatusChip(label, scheme.errorContainer, scheme.onErrorContainer, scheme.error, modifier.semantics { contentDescription = description })
    }
}

@Composable
private fun BatteryValue(percent: Int?, atMs: Long?) {
    if (percent == null) {
        Text(stringResource(R.string.unknown), style = MaterialTheme.typography.bodyMedium)
        return
    }
    val low = percent <= 15
    val color = if (low) MaterialTheme.colorScheme.error else MaterialTheme.status.good
    Column(horizontalAlignment = Alignment.End) {
        Row(
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            modifier = Modifier.semantics(mergeDescendants = true) {},
        ) {
            BatteryIndicator(percent, color)
            Text(
                stringResource(R.string.battery_percent, percent),
                style = MaterialTheme.typography.bodyMedium,
                color = if (low) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurface,
            )
        }
        if (atMs != null) {
            WithAbsoluteTime(atMs) {
                Text(
                    stringResource(R.string.updated_ago, relativeTime(atMs)),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}
