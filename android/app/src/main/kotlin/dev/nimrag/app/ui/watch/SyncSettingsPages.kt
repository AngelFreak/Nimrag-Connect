package dev.nimrag.app.ui.watch

import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Info
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import dev.nimrag.app.R
import dev.nimrag.app.model.GarminConnect
import dev.nimrag.app.ui.components.Footnote
import dev.nimrag.app.ui.components.MessageCard
import dev.nimrag.app.ui.components.NimragButton
import dev.nimrag.app.ui.components.PageColumn
import dev.nimrag.app.ui.components.RowTrailing
import dev.nimrag.app.ui.components.SettingsGroup
import dev.nimrag.app.ui.components.SettingsPage
import dev.nimrag.app.ui.components.SettingsRow
import dev.nimrag.app.ui.theme.status
import dev.nimrag.app.ui.watch.logic.SyncIntervals

// ---------------------------------------------------------------- automatic sync

/** How often the phone asks a connected watch to sync: a radio list, one footer line. */
@Composable
internal fun AutoSyncPage(minutes: Int, onSelect: (Int) -> Unit, onBack: () -> Unit) {
    SettingsPage(title = stringResource(R.string.auto_sync_title), onBack = onBack) {
        PageColumn {
            SettingsGroup(
                modifier = Modifier.selectableGroup(),
                footer = stringResource(if (minutes == 0) R.string.auto_sync_off_hint else R.string.auto_sync_hint),
            ) {
                SyncIntervals.options(minutes).forEach { option ->
                    item {
                        SettingsRow(
                            title = intervalLabel(option),
                            value = when (option) {
                                0 -> stringResource(R.string.auto_sync_choice_off)
                                15 -> stringResource(R.string.auto_sync_choice_15)
                                else -> null
                            },
                            trailing = RowTrailing.Radio(option == minutes),
                            onClick = { onSelect(option) },
                        )
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------- Garmin Connect

/**
 * What to know about Garmin Connect: Nimrag is the only app that syncs the watch, and the server's
 * "Send to Garmin" (in the web profile) keeps the Garmin account up to date. [onOpenProfile]
 * opens that page; it is offered only when a server is set up.
 */
@Composable
internal fun GarminConnectPage(
    serverConfigured: Boolean,
    garminConnect: GarminConnect,
    onOpenProfile: () -> Unit,
    onHelp: () -> Unit,
    onBack: () -> Unit,
) {
    SettingsPage(title = stringResource(R.string.settings_garmin_connect), onBack = onBack) {
        PageColumn {
            MessageCard(
                icon = Icons.Filled.Info,
                title = stringResource(R.string.garmin_info_title),
                containerColor = MaterialTheme.colorScheme.secondaryContainer,
                contentColor = MaterialTheme.colorScheme.onSecondaryContainer,
            ) {
                Text(stringResource(R.string.garmin_info_body), style = MaterialTheme.typography.bodyMedium)
            }
            if (serverConfigured) {
                SettingsGroup(heading = stringResource(R.string.garmin_forward_heading), footer = stringResource(R.string.garmin_forward_body)) {
                    item {
                        SettingsRow(
                            title = stringResource(R.string.garmin_forward_row),
                            value = stringResource(R.string.garmin_forward_row_value),
                            trailing = RowTrailing.Chevron,
                            onClick = onOpenProfile,
                        )
                    }
                }
            } else {
                Footnote(stringResource(R.string.garmin_forward_body), Modifier.fillMaxWidth())
            }
            if (garminConnect != GarminConnect.Absent) {
                SettingsGroup {
                    item {
                        SettingsRow(
                            title = stringResource(R.string.garmin_help_row),
                            trailing = RowTrailing.Chevron,
                            onClick = onHelp,
                        )
                    }
                }
            }
        }
    }
}

/**
 * "How to fix": what Garmin Connect does to the watch and how to keep it out of the way. The text
 * follows what Connect can do on this phone right now.
 *
 * Garmin Connect holds the watch's session whenever it runs, and the system restarts it when the
 * watch is in range, so Force stop does not last. Revoking its "Nearby devices" permission does.
 */
@Composable
internal fun GarminHelpPage(
    garminConnect: GarminConnect,
    onOpenGarminConnectInfo: () -> Unit,
    onBack: () -> Unit,
) {
    val open: @Composable () -> Unit = {
        NimragButton(onClick = onOpenGarminConnectInfo) { Text(stringResource(R.string.garmin_connect_open)) }
    }
    val bluetoothOff = garminConnect == GarminConnect.BluetoothOff
    SettingsPage(title = stringResource(R.string.garmin_help_title), onBack = onBack) {
        PageColumn {
            when {
                garminConnect == GarminConnect.Absent -> Footnote(stringResource(R.string.garmin_help_absent), Modifier.fillMaxWidth())
                bluetoothOff -> MessageCard(
                    icon = Icons.Filled.Info,
                    title = stringResource(R.string.garmin_connect_off_title),
                    containerColor = MaterialTheme.colorScheme.secondaryContainer,
                    contentColor = MaterialTheme.colorScheme.onSecondaryContainer,
                    actions = open,
                ) {
                    Text(stringResource(R.string.garmin_connect_off_body), style = MaterialTheme.typography.bodyMedium)
                }
                else -> MessageCard(
                    icon = Icons.Filled.Warning,
                    title = stringResource(R.string.garmin_connect_title),
                    containerColor = MaterialTheme.status.warnContainer,
                    contentColor = MaterialTheme.status.onWarnContainer,
                    actions = open,
                ) {
                    Text(stringResource(R.string.garmin_connect_body), style = MaterialTheme.typography.bodyMedium)
                    Text(stringResource(R.string.garmin_connect_test), style = MaterialTheme.typography.bodyMedium)
                    Text(stringResource(R.string.garmin_connect_switch), style = MaterialTheme.typography.bodySmall)
                }
            }
        }
    }
}
