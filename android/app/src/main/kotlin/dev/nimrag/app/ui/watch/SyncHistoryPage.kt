package dev.nimrag.app.ui.watch

import android.content.Intent
import android.os.Build
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.KeyboardArrowDown
import androidx.compose.material.icons.filled.KeyboardArrowUp
import androidx.compose.material.icons.filled.Share
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.nimrag.app.R
import dev.nimrag.app.model.AppState
import dev.nimrag.app.model.GarminConnectMode
import dev.nimrag.app.model.LogLine
import dev.nimrag.app.model.StoredFile
import dev.nimrag.app.model.SyncState
import dev.nimrag.app.model.UploadState
import dev.nimrag.app.model.WatchController
import dev.nimrag.app.model.WatchState
import dev.nimrag.app.ui.components.CardTitle
import dev.nimrag.app.ui.components.NimragButton
import dev.nimrag.app.ui.components.SectionCard
import dev.nimrag.app.ui.components.SettingsPage
import dev.nimrag.app.ui.components.WithAbsoluteTime
import dev.nimrag.app.ui.components.absoluteDateTime
import dev.nimrag.app.ui.components.clockTime
import dev.nimrag.app.ui.components.formatSize
import dev.nimrag.app.ui.components.logTime
import dev.nimrag.app.ui.components.relativeTime
import dev.nimrag.app.ui.theme.status

/**
 * Everything the phone did with the watch's files: the last sync, what is stored and uploaded,
 * the recent files and the log (with Share, for a bug report).
 */
@Composable
internal fun SyncHistoryPage(
    state: AppState,
    controller: WatchController,
    onOpenServer: () -> Unit,
    onBack: () -> Unit,
    modifier: Modifier = Modifier,
) {
    var logExpanded by rememberSaveable { mutableStateOf(false) }
    val log = state.log
    val context = LocalContext.current
    val shareTitle = stringResource(R.string.log_share)

    SettingsPage(title = stringResource(R.string.history_title), onBack = onBack, modifier = modifier) {
        LazyColumn(
            modifier = Modifier.fillMaxSize(),
            contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = 8.dp, bottom = 24.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            item(key = "sync") {
                SyncCard(
                    state.sync,
                    paired = state.watch is WatchState.Paired,
                    alongside = state.settings.garminConnectMode == GarminConnectMode.Alongside,
                )
            }
            item(key = "uploads") {
                UploadsCard(
                    uploads = state.uploads,
                    serverConfigured = state.settings.serverUrl.isNotBlank(),
                    allowMobileData = state.settings.allowMobileData,
                    onUploadNow = controller::uploadNow,
                    onOpenServer = onOpenServer,
                )
            }
            item(key = "recent") { RecentFilesCard(state.uploads.recent) }

            // The log: header and lines are separate lazy items that are shaped to read as one card.
            item(key = "log-header") {
                LogHeader(
                    count = log.size,
                    expanded = logExpanded,
                    onToggle = { logExpanded = !logExpanded },
                    onShare = {
                        val send = Intent(Intent.ACTION_SEND)
                            .setType("text/plain")
                            .putExtra(Intent.EXTRA_SUBJECT, "Nimrag Connect log")
                            .putExtra(Intent.EXTRA_TEXT, logReport(log, appVersion(context)))
                        context.startActivity(Intent.createChooser(send, shareTitle))
                    },
                )
            }
            if (logExpanded) {
                if (log.isEmpty()) {
                    item(key = "log-empty") {
                        LogContainer(last = true) {
                            Text(
                                stringResource(R.string.log_empty),
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                                modifier = Modifier.padding(16.dp),
                            )
                        }
                    }
                } else {
                    val lastIndex = log.lastIndex
                    items(log.size, key = { "log-$it" }) { index ->
                        LogContainer(last = index == lastIndex) {
                            LogRow(log[index])
                        }
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------- sync

@Composable
internal fun SyncCard(sync: SyncState, paired: Boolean, modifier: Modifier = Modifier, alongside: Boolean = false) {
    SectionCard(modifier) {
        if (sync.running) {
            CardTitle(stringResource(R.string.sync_title_running))
            val total = sync.filesInCycle
            Text(
                if (total == null) {
                    stringResource(R.string.sync_reading_list)
                } else {
                    pluralStringResource(R.plurals.sync_pulling, total, (sync.filesDone + 1).coerceAtMost(maxOf(total, 1)), total)
                },
                style = MaterialTheme.typography.bodyLarge,
            )
            if (sync.currentSize > 0) {
                val fraction = (sync.currentBytes.toFloat() / sync.currentSize.toFloat()).coerceIn(0f, 1f)
                val percent = (fraction * 100).toInt()
                val progressDescription = stringResource(R.string.sync_progress_description, percent)
                LinearProgressIndicator(
                    progress = { fraction },
                    modifier = Modifier
                        .fillMaxWidth()
                        .semantics { contentDescription = progressDescription },
                )
                Text(
                    stringResource(R.string.sync_bytes_of, formatSize(sync.currentBytes), formatSize(sync.currentSize)),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            } else {
                LinearProgressIndicator(Modifier.fillMaxWidth())
            }
        } else {
            CardTitle(stringResource(R.string.sync_title_last))
            val result = sync.lastResult
            if (result == null) {
                Text(
                    stringResource(if (paired) R.string.sync_none_yet else R.string.sync_pair_first),
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            } else {
                val problems = result.failed > 0
                val line = when {
                    alongside && result.skipped > 0 ->
                        stringResource(R.string.sync_result_alongside_skipped, result.downloaded, result.failed, result.skipped)
                    alongside -> stringResource(R.string.sync_result_alongside, result.downloaded, result.failed)
                    result.skipped > 0 ->
                        stringResource(R.string.sync_result_skipped, result.downloaded, result.archived, result.failed, result.skipped)
                    else -> stringResource(R.string.sync_result, result.downloaded, result.archived, result.failed)
                }
                Text(
                    line + " · " + clockTime(result.finishedAtMs),
                    style = MaterialTheme.typography.bodyLarge,
                    color = if (problems) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurface,
                )
                WithAbsoluteTime(result.finishedAtMs) {
                    Text(
                        relativeTime(result.finishedAtMs),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        }
        if (alongside && sync.leftForGarminConnect > 0) {
            Text(
                stringResource(R.string.sync_left_for_connect, sync.leftForGarminConnect),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

// ---------------------------------------------------------------- uploads

/**
 * What is waiting, uploaded and stored. The counters appear once there is something to count
 * (a row of zeros looks broken), and Upload now only while files wait.
 */
@Composable
internal fun UploadsCard(
    uploads: UploadState,
    serverConfigured: Boolean,
    allowMobileData: Boolean,
    onUploadNow: () -> Unit,
    onOpenServer: () -> Unit,
    modifier: Modifier = Modifier,
) {
    SectionCard(modifier) {
        CardTitle(stringResource(R.string.uploads_title))
        if (!serverConfigured) {
            Text(
                stringResource(R.string.uploads_not_configured),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            TextButton(onClick = onOpenServer) { Text(stringResource(R.string.uploads_open_server)) }
            return@SectionCard
        }
        val any = uploads.pending > 0 || uploads.uploadedTotal > 0 || uploads.storedTotal > 0
        if (any) {
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                Stat(uploads.pending.toString(), stringResource(R.string.stat_pending))
                Stat(uploads.uploadedTotal.toString(), stringResource(R.string.stat_uploaded))
                Stat(uploads.storedTotal.toString(), stringResource(R.string.stat_stored))
            }
        }
        val last = uploads.lastUploadAtMs
        if (last == null) {
            Text(
                stringResource(R.string.uploads_never),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        } else {
            WithAbsoluteTime(last) {
                Text(
                    stringResource(R.string.uploads_last, relativeTime(last)),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        if (uploads.waitingForNetwork) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Icon(Icons.Filled.Warning, contentDescription = null, tint = MaterialTheme.status.warn)
                Text(
                    stringResource(if (allowMobileData) R.string.uploads_waiting_network else R.string.uploads_waiting_wifi),
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.status.warn,
                )
            }
        }
        uploads.lastError?.let {
            Text(
                stringResource(R.string.uploads_last_error, it),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.error,
            )
        }
        if (uploads.pending > 0) {
            NimragButton(onClick = onUploadNow) { Text(stringResource(R.string.uploads_now)) }
        }
    }
}

@Composable
private fun Stat(value: String, label: String) {
    Column(Modifier.semantics(mergeDescendants = true) {}) {
        Text(value, style = MaterialTheme.typography.headlineSmall, color = MaterialTheme.colorScheme.onSurface)
        Text(label, style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

// ---------------------------------------------------------------- stored files

@Composable
internal fun RecentFilesCard(files: List<StoredFile>, modifier: Modifier = Modifier) {
    SectionCard(modifier) {
        CardTitle(stringResource(R.string.recent_title))
        if (files.isEmpty()) {
            Text(
                stringResource(R.string.recent_empty),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        } else {
            Column {
                files.forEachIndexed { index, file ->
                    if (index > 0) HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
                    StoredFileRow(file)
                }
            }
        }
    }
}

@Composable
private fun StoredFileRow(file: StoredFile) {
    val uploaded = file.uploadedAtMs != null
    val uploadedText = stringResource(R.string.file_uploaded)
    val pendingText = stringResource(R.string.file_pending)
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(vertical = 10.dp)
            .semantics(mergeDescendants = true) {},
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text(
                file.kind,
                style = MaterialTheme.typography.bodyLarge,
                color = MaterialTheme.colorScheme.onSurface,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            val watchTime = file.watchTimeMs?.let { absoluteDateTime(it) }
            Text(
                listOfNotNull(formatSize(file.sizeBytes), watchTime?.let { stringResource(R.string.file_on_watch, it) })
                    .joinToString(" · "),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            WithAbsoluteTime(file.receivedAtMs) {
                Text(
                    stringResource(R.string.file_received, relativeTime(file.receivedAtMs)),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        if (uploaded) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                Icon(Icons.Filled.Check, contentDescription = null, tint = MaterialTheme.status.good)
                Text(uploadedText, style = MaterialTheme.typography.labelMedium, color = MaterialTheme.status.good)
            }
        } else {
            Text(pendingText, style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}

// ---------------------------------------------------------------- log

@Composable
internal fun LogHeader(
    count: Int,
    expanded: Boolean,
    onToggle: () -> Unit,
    modifier: Modifier = Modifier,
    onShare: (() -> Unit)? = null,
) {
    val shape = if (expanded) {
        RoundedCornerShape(topStart = 12.dp, topEnd = 12.dp)
    } else {
        RoundedCornerShape(12.dp)
    }
    val stateText = stringResource(if (expanded) R.string.log_collapse else R.string.log_expand)
    Row(
        modifier = modifier
            .fillMaxWidth()
            .clip(shape)
            .background(MaterialTheme.colorScheme.surfaceContainerLow)
            .clickable(onClickLabel = stateText, role = Role.Button, onClick = onToggle)
            .padding(horizontal = 16.dp, vertical = 14.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Text(
            stringResource(R.string.log_title, count),
            style = MaterialTheme.typography.titleMedium,
            modifier = Modifier.weight(1f),
        )
        if (onShare != null && count > 0) {
            IconButton(onClick = onShare) {
                Icon(Icons.Filled.Share, contentDescription = stringResource(R.string.log_share))
            }
        }
        Icon(
            if (expanded) Icons.Filled.KeyboardArrowUp else Icons.Filled.KeyboardArrowDown,
            contentDescription = stateText,
        )
    }
}

/**
 * The log as plain text, oldest first, for sharing. The log never holds the unit id or the
 * token, so it is safe to send as it is.
 */
internal fun logReport(log: List<LogLine>, version: String): String = buildString {
    append("Nimrag Connect ").append(version)
    append(", Android ").append(Build.VERSION.RELEASE).append(" (API ").append(Build.VERSION.SDK_INT).append(")")
    append(", ").append(Build.MANUFACTURER).append(' ').append(Build.MODEL).append('\n')
    log.asReversed().forEach { line ->
        val level = when (line.level) {
            LogLine.Level.Info -> "I"
            LogLine.Level.Warning -> "W"
            LogLine.Level.Error -> "E"
        }
        append(ReportTime.format(Instant.ofEpochMilli(line.atMs).atZone(ZoneId.systemDefault()))).append(' ').append(level).append("  ").append(line.text).append('\n')
    }
}

private val ReportTime: DateTimeFormatter = DateTimeFormatter.ofPattern("yyyy-MM-dd HH:mm:ss")

private fun appVersion(context: android.content.Context): String =
    runCatching { context.packageManager.getPackageInfo(context.packageName, 0).versionName }.getOrNull() ?: "?"

@Composable
private fun LogContainer(last: Boolean, content: @Composable () -> Unit) {
    val shape = RoundedCornerShape(
        bottomStart = if (last) 12.dp else 0.dp,
        bottomEnd = if (last) 12.dp else 0.dp,
    )
    Box(
        Modifier
            .fillMaxWidth()
            .clip(shape)
            .background(MaterialTheme.colorScheme.surfaceContainerLow),
    ) { content() }
}

@Composable
private fun LogRow(line: LogLine) {
    val color = when (line.level) {
        LogLine.Level.Info -> MaterialTheme.colorScheme.onSurfaceVariant
        LogLine.Level.Warning -> MaterialTheme.status.warn
        LogLine.Level.Error -> MaterialTheme.colorScheme.error
    }
    val levelLabel = when (line.level) {
        LogLine.Level.Info -> "I"
        LogLine.Level.Warning -> "W"
        LogLine.Level.Error -> "E"
    }
    Text(
        "${logTime(line.atMs)} $levelLabel  ${line.text}",
        modifier = Modifier
            .fillMaxWidth()
            .padding(horizontal = 16.dp, vertical = 2.dp),
        style = MaterialTheme.typography.bodySmall.copy(fontFamily = FontFamily.Monospace, fontSize = 11.sp, lineHeight = 15.sp),
        color = color,
    )
}
