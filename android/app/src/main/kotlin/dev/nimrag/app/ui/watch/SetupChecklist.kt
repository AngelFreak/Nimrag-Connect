package dev.nimrag.app.ui.watch

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import dev.nimrag.app.R
import dev.nimrag.app.model.AppState
import dev.nimrag.app.model.FoundDevice
import dev.nimrag.app.model.Permissions
import dev.nimrag.app.model.WatchController
import dev.nimrag.app.ui.components.ConfirmDialog
import dev.nimrag.app.ui.components.MessageCard
import dev.nimrag.app.ui.components.NimragButton
import dev.nimrag.app.ui.components.RowTrailing
import dev.nimrag.app.ui.components.SectionCard
import dev.nimrag.app.ui.components.CardTitle
import dev.nimrag.app.ui.components.SettingsGroup
import dev.nimrag.app.ui.components.SettingsRow
import dev.nimrag.app.ui.components.SignalBars
import dev.nimrag.app.ui.components.WatchIllustration
import dev.nimrag.app.ui.theme.status
import dev.nimrag.app.ui.watch.logic.PairingSetup
import dev.nimrag.app.ui.watch.logic.SetupAction

/**
 * Not paired: a short checklist and one button that always does the next step. Watches found by
 * the scan appear below as rows; picking one asks for a confirmation first.
 */
@Composable
internal fun SetupSection(state: AppState, controller: WatchController, actions: WatchActions) {
    var pendingDevice by rememberSaveable { mutableStateOf<String?>(null) }
    var scannedOnce by rememberSaveable { mutableStateOf(false) }
    val scan = state.scan

    // Never leave a scan running when this section goes away (page change, pairing started).
    DisposableEffect(controller) { onDispose { controller.stopScan() } }

    state.pairingError?.let { PairingFailedCard(it) }

    SetupContent(
        permissions = state.permissions,
        scanning = scan.scanning,
        found = scan.found,
        scannedOnce = scannedOnce,
        onAllowBluetooth = actions.allowBluetooth,
        onTurnOnBluetooth = actions.turnOnBluetooth,
        onScan = {
            scannedOnce = true
            controller.startScan()
        },
        onStop = controller::stopScan,
        onPick = { pendingDevice = it.address },
    )

    val device = scan.found.firstOrNull { it.address == pendingDevice }
    if (device != null) {
        ConfirmDialog(
            title = stringResource(R.string.pair_confirm_title, device.name),
            confirmLabel = stringResource(R.string.pair_confirm),
            onConfirm = {
                pendingDevice = null
                controller.pair(device.address)
            },
            onDismiss = { pendingDevice = null },
        ) {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(stringResource(R.string.pair_confirm_body))
                device.unsupportedReason?.let {
                    Text(stringResource(R.string.cannot_sync_yet, it), color = MaterialTheme.status.warn)
                }
            }
        }
    }
}

/** The checklist card, the found watches under it; stateless for previews. */
@Composable
internal fun SetupContent(
    permissions: Permissions,
    scanning: Boolean,
    found: List<FoundDevice>,
    scannedOnce: Boolean,
    onAllowBluetooth: () -> Unit,
    onTurnOnBluetooth: () -> Unit,
    onScan: () -> Unit,
    onStop: () -> Unit,
    onPick: (FoundDevice) -> Unit,
) {
    val action = PairingSetup.action(permissions, scanning)
    val step = PairingSetup.currentStep(permissions, scanning, foundAny = found.isNotEmpty())
    val bluetoothReady = permissions.bluetooth && permissions.bluetoothEnabled

    Card(
        modifier = Modifier.fillMaxWidth(),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerLow),
    ) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
            Box(Modifier.fillMaxWidth(), contentAlignment = Alignment.Center) { WatchIllustration(size = 80.dp) }
            Column(verticalArrangement = Arrangement.spacedBy(14.dp)) {
                ChecklistStep(
                    number = 1,
                    current = step,
                    title = stringResource(
                        when {
                            !permissions.bluetooth -> R.string.setup_bluetooth_allow
                            !permissions.bluetoothEnabled -> R.string.setup_bluetooth_turn_on
                            else -> R.string.setup_bluetooth_ready
                        },
                    ),
                    hint = stringResource(R.string.setup_bluetooth_hint),
                    done = bluetoothReady,
                )
                ChecklistStep(
                    number = 2,
                    current = step,
                    title = stringResource(R.string.setup_pairing_mode),
                    hint = stringResource(R.string.setup_pairing_mode_hint),
                    done = step > 2,
                )
                ChecklistStep(
                    number = 3,
                    current = step,
                    title = stringResource(R.string.setup_find),
                    hint = stringResource(R.string.setup_find_hint),
                    done = false,
                )
            }
            NimragButton(
                onClick = when (action) {
                    SetupAction.AllowBluetooth -> onAllowBluetooth
                    SetupAction.TurnOnBluetooth -> onTurnOnBluetooth
                    SetupAction.Scan -> onScan
                    // Tapping while it looks is how to stop: the button is never dead.
                    SetupAction.Scanning -> onStop
                },
                modifier = Modifier.fillMaxWidth(),
            ) {
                if (action == SetupAction.Scanning) {
                    CircularProgressIndicator(
                        modifier = Modifier.size(20.dp),
                        strokeWidth = 2.dp,
                        color = MaterialTheme.colorScheme.onPrimary,
                    )
                    Text(stringResource(R.string.scan_running), modifier = Modifier.padding(start = 12.dp))
                } else {
                    Text(
                        stringResource(
                            when (action) {
                                SetupAction.AllowBluetooth -> R.string.setup_allow_button
                                SetupAction.TurnOnBluetooth -> R.string.bluetooth_turn_on
                                else -> if (scannedOnce) R.string.scan_again else R.string.scan_start
                            },
                        ),
                    )
                }
            }
            if (scannedOnce && !scanning && found.isEmpty() && bluetoothReady) {
                Text(
                    stringResource(R.string.scan_none_found),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }

    if (found.isNotEmpty()) {
        SettingsGroup(heading = stringResource(R.string.scan_found_title)) {
            found.forEach { device ->
                item {
                    SettingsRow(
                        title = device.name,
                        value = device.unsupportedReason?.let { stringResource(R.string.cannot_sync_yet, it) }
                            ?: device.model.takeIf { it != device.name },
                        valueColor = if (device.unsupportedReason != null) MaterialTheme.status.warn else Color.Unspecified,
                        trailing = RowTrailing.Custom { SignalBars(device.rssi) },
                        onClick = { onPick(device) },
                    )
                }
            }
        }
    }
}

/** One numbered line of the checklist: a tick when done, the accent when it is the one to do now. */
@Composable
private fun ChecklistStep(number: Int, current: Int, title: String, hint: String, done: Boolean) {
    val scheme = MaterialTheme.colorScheme
    val doneText = stringResource(R.string.step_done)
    val nowText = stringResource(R.string.step_now)
    val laterText = stringResource(R.string.step_later)
    val isNow = !done && number == current
    Row(
        Modifier
            .fillMaxWidth()
            .semantics(mergeDescendants = true) { stateDescription = if (done) doneText else if (isNow) nowText else laterText },
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Box(
            Modifier
                .size(24.dp)
                .clip(CircleShape)
                .background(
                    when {
                        done -> MaterialTheme.status.goodContainer
                        isNow -> scheme.primary
                        else -> scheme.surfaceContainerHighest
                    },
                ),
            contentAlignment = Alignment.Center,
        ) {
            if (done) {
                Icon(Icons.Filled.Check, contentDescription = null, modifier = Modifier.size(16.dp), tint = MaterialTheme.status.good)
            } else {
                Text(
                    number.toString(),
                    style = MaterialTheme.typography.labelMedium.copy(fontWeight = FontWeight.SemiBold),
                    color = if (isNow) scheme.onPrimary else scheme.onSurfaceVariant,
                )
            }
        }
        Column(Modifier.weight(1f)) {
            Text(
                title,
                style = MaterialTheme.typography.bodyLarge.copy(fontWeight = FontWeight.Medium),
                color = if (done || isNow) scheme.onSurface else scheme.onSurfaceVariant,
            )
            Text(hint, style = MaterialTheme.typography.bodyMedium, color = scheme.onSurfaceVariant)
        }
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

/** Bonding is running: nothing to do but accept on the phone and the watch. */
@Composable
internal fun PairingCard(name: String) {
    SectionCard {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(16.dp)) {
            CircularProgressIndicator(modifier = Modifier.size(32.dp), strokeWidth = 3.dp)
            CardTitle(stringResource(R.string.pairing_title, name))
        }
        Text(
            stringResource(R.string.pairing_hint),
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}
