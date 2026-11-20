package dev.nimrag.app.ui.watch

import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.material3.MaterialTheme
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.stringResource
import dev.nimrag.app.R
import dev.nimrag.app.model.LinkState
import dev.nimrag.app.model.WatchState
import dev.nimrag.app.ui.components.ConfirmDialog
import dev.nimrag.app.ui.components.PageColumn
import dev.nimrag.app.ui.components.RowTrailing
import dev.nimrag.app.ui.components.SettingsGroup
import dev.nimrag.app.ui.components.SettingsPage
import dev.nimrag.app.ui.components.SettingsRow
import dev.nimrag.app.ui.components.SyncIcon
import dev.nimrag.app.ui.components.relativeTime

/** What the phone knows about the paired watch, and Pair again. */
@Composable
internal fun WatchDetailsPage(watch: WatchState.Paired, onPairAgain: () -> Unit, onBack: () -> Unit) {
    var confirmPairAgain by rememberSaveable { mutableStateOf(false) }
    val low = (watch.batteryPercent ?: 100) <= 15

    SettingsPage(title = stringResource(R.string.watch_details_title), onBack = onBack) {
        PageColumn {
            SettingsGroup {
                item { SettingsRow(title = stringResource(R.string.label_model), value = watch.model) }
                item {
                    SettingsRow(
                        title = stringResource(R.string.label_software),
                        value = watch.softwareVersion ?: stringResource(R.string.unknown),
                    )
                }
                item {
                    val at = watch.lastConnectedAtMs
                    SettingsRow(
                        title = stringResource(R.string.label_last_connected),
                        value = if (at == null) stringResource(R.string.never) else relativeTime(at),
                    )
                }
                item {
                    val percent = watch.batteryPercent
                    val at = watch.batteryAtMs
                    SettingsRow(
                        title = stringResource(R.string.label_battery),
                        value = when {
                            percent == null -> stringResource(R.string.unknown)
                            at == null -> stringResource(R.string.battery_percent, percent)
                            else -> stringResource(R.string.battery_percent_updated, percent, relativeTime(at))
                        },
                        valueColor = if (low) MaterialTheme.colorScheme.error else Color.Unspecified,
                    )
                }
                item { SettingsRow(title = stringResource(R.string.label_address), value = watch.address) }
            }
            SettingsGroup(footer = stringResource(R.string.pair_again_hint)) {
                item {
                    SettingsRow(
                        title = stringResource(R.string.pair_again),
                        icon = SyncIcon,
                        trailing = RowTrailing.Chevron,
                        // Lost pairing: forgetting returns the page to scanning, without a confirmation.
                        onClick = if (watch.link == LinkState.BondLost) onPairAgain else ({ confirmPairAgain = true }),
                    )
                }
            }
        }
    }

    if (confirmPairAgain) {
        ConfirmDialog(
            title = stringResource(R.string.pair_again_title, watch.name),
            body = stringResource(R.string.forget_body),
            confirmLabel = stringResource(R.string.pair_again_confirm),
            onConfirm = {
                confirmPairAgain = false
                onPairAgain()
            },
            onDismiss = { confirmPairAgain = false },
        )
    }
}
