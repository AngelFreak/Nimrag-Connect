package dev.nimrag.app.ui.watch

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import dev.nimrag.app.R
import dev.nimrag.app.model.LinkState
import dev.nimrag.app.model.SyncState
import dev.nimrag.app.model.WatchState
import dev.nimrag.app.ui.components.NimragButton
import dev.nimrag.app.ui.components.StatusChip
import dev.nimrag.app.ui.components.WatchIllustration
import dev.nimrag.app.ui.components.WithAbsoluteTime
import dev.nimrag.app.ui.components.formatSize
import dev.nimrag.app.ui.components.relativeTime
import dev.nimrag.app.ui.theme.status
import dev.nimrag.app.ui.watch.logic.WatchStatus
import dev.nimrag.app.ui.watch.logic.WatchStatuses

/**
 * The top of the Watch page for a paired watch: its picture, name and one status word, a line of
 * facts, and one big button that turns into progress in place while a sync runs.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun StatusCard(
    watch: WatchState.Paired,
    sync: SyncState,
    onSync: () -> Unit,
    onPairAgain: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val result = sync.lastResult
    val status = WatchStatuses.of(watch, sync.running, result?.failed ?: 0)
    val scheme = MaterialTheme.colorScheme
    val accent = when (status) {
        WatchStatus.NotConnected -> scheme.onSurfaceVariant
        WatchStatus.PairingLost, WatchStatus.SyncFailed -> scheme.error
        else -> scheme.primary
    }
    Card(
        modifier = modifier.fillMaxWidth(),
        colors = CardDefaults.cardColors(containerColor = scheme.surfaceContainerLow),
    ) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(14.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(16.dp)) {
                WatchIllustration(accent = accent)
                Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                    Column {
                        Text(watch.name, style = MaterialTheme.typography.headlineSmall, color = scheme.onSurface)
                        if (watch.model != watch.name) {
                            Text(watch.model, style = MaterialTheme.typography.bodySmall, color = scheme.onSurfaceVariant)
                        }
                    }
                    StatusPill(status)
                }
            }

            if (sync.running) {
                SyncProgress(sync)
            } else {
                FlowRow(
                    horizontalArrangement = Arrangement.spacedBy(16.dp),
                    verticalArrangement = Arrangement.spacedBy(4.dp),
                ) {
                    watch.batteryPercent?.let { percent ->
                        Fact(
                            stringResource(R.string.label_battery),
                            stringResource(R.string.battery_percent, percent),
                            if (percent <= 15) scheme.error else Color.Unspecified,
                        )
                    }
                    if (result == null) {
                        Text(
                            stringResource(R.string.status_not_synced),
                            style = MaterialTheme.typography.bodyMedium,
                            color = scheme.onSurfaceVariant,
                        )
                    } else {
                        WithAbsoluteTime(result.finishedAtMs) {
                            Fact(stringResource(R.string.status_synced), relativeTime(result.finishedAtMs))
                        }
                        if (result.downloaded > 0) {
                            Text(
                                pluralStringResource(R.plurals.status_new_files, result.downloaded, result.downloaded),
                                style = MaterialTheme.typography.bodyMedium,
                                color = scheme.onSurfaceVariant,
                            )
                        }
                        if (result.failed > 0) {
                            Text(
                                pluralStringResource(R.plurals.status_failed_files, result.failed, result.failed),
                                style = MaterialTheme.typography.bodyMedium,
                                color = scheme.error,
                            )
                        }
                    }
                }
                linkHint(watch.link)?.let {
                    Text(
                        it,
                        style = MaterialTheme.typography.bodySmall,
                        color = if (watch.link == LinkState.BondLost) scheme.error else scheme.onSurfaceVariant,
                    )
                }
                if (watch.link == LinkState.BondLost) {
                    // Lost pairing: forgetting returns the page to scanning, without a confirmation.
                    NimragButton(onClick = onPairAgain, modifier = Modifier.fillMaxWidth()) {
                        Text(stringResource(R.string.pair_again))
                    }
                } else {
                    NimragButton(
                        onClick = onSync,
                        enabled = watch.link == LinkState.Ready && watch.unsupportedReason == null,
                        modifier = Modifier.fillMaxWidth(),
                    ) { Text(stringResource(R.string.sync_now)) }
                }
            }
        }
    }
}

/** "Battery 74%": a muted label and the value in the text colour. */
@Composable
private fun Fact(label: String, value: String, valueColor: Color = Color.Unspecified) {
    Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
        Text(label, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        Text(
            value,
            style = MaterialTheme.typography.bodyMedium.copy(fontWeight = FontWeight.SemiBold),
            color = if (valueColor == Color.Unspecified) MaterialTheme.colorScheme.onSurface else valueColor,
        )
    }
}

/** The one-sentence reason behind a status that is not "Connected"; null when there is nothing to add. */
@Composable
private fun linkHint(link: LinkState): String? = when (link) {
    LinkState.Offline -> stringResource(R.string.link_offline_hint)
    LinkState.Waiting -> stringResource(R.string.link_waiting_hint)
    LinkState.Connecting -> stringResource(R.string.link_connecting_hint)
    LinkState.Ready -> null
    LinkState.BondLost -> stringResource(R.string.link_bond_lost_hint)
}

/** Sync now, in place: which file of how many, and a bar for the whole run (indeterminate until the list is known). */
@Composable
private fun SyncProgress(sync: SyncState) {
    val total = sync.filesInCycle
    val label = if (total == null) {
        stringResource(R.string.sync_reading_list)
    } else {
        pluralStringResource(R.plurals.sync_pulling, total, (sync.filesDone + 1).coerceAtMost(maxOf(total, 1)), total)
    }
    Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text(label, style = MaterialTheme.typography.bodyLarge.copy(fontWeight = FontWeight.Medium))
        if (total != null && total > 0) {
            val current = if (sync.currentSize > 0) (sync.currentBytes.toFloat() / sync.currentSize.toFloat()).coerceIn(0f, 1f) else 0f
            val fraction = ((sync.filesDone + current) / total).coerceIn(0f, 1f)
            val description = stringResource(R.string.sync_progress_description, (fraction * 100).toInt())
            LinearProgressIndicator(
                progress = { fraction },
                modifier = Modifier
                    .fillMaxWidth()
                    .semantics { contentDescription = description },
            )
            if (sync.currentSize > 0) {
                Text(
                    stringResource(R.string.sync_bytes_of, formatSize(sync.currentBytes), formatSize(sync.currentSize)),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        } else {
            LinearProgressIndicator(Modifier.fillMaxWidth())
        }
    }
}

/** The status word as a chip, coloured by how much it needs the owner. */
@Composable
internal fun StatusPill(status: WatchStatus, modifier: Modifier = Modifier) {
    val scheme = MaterialTheme.colorScheme
    val colors = MaterialTheme.status
    val label = stringResource(
        when (status) {
            WatchStatus.Connected -> R.string.status_connected
            WatchStatus.Connecting -> R.string.status_connecting
            WatchStatus.NotConnected -> R.string.status_not_connected
            WatchStatus.Syncing -> R.string.status_syncing
            WatchStatus.SyncFailed -> R.string.status_sync_failed
            WatchStatus.PairingLost -> R.string.status_pairing_lost
        },
    )
    val description = stringResource(R.string.status_description, label)
    val semantic = modifier.semantics { contentDescription = description }
    when (status) {
        WatchStatus.Connected -> StatusChip(label, colors.goodContainer, colors.onGoodContainer, colors.good, semantic)
        WatchStatus.Connecting, WatchStatus.Syncing ->
            StatusChip(label, scheme.primaryContainer, scheme.onPrimaryContainer, scheme.primary, semantic)
        WatchStatus.NotConnected -> StatusChip(label, colors.warnContainer, colors.onWarnContainer, colors.warn, semantic)
        WatchStatus.SyncFailed, WatchStatus.PairingLost ->
            StatusChip(label, scheme.errorContainer, scheme.onErrorContainer, scheme.error, semantic)
    }
}
