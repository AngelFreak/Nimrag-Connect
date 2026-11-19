package dev.nimrag.app.ui.components

import android.text.format.DateUtils
import androidx.compose.foundation.layout.Box
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.PlainTooltip
import androidx.compose.material3.Text
import androidx.compose.material3.TooltipAnchorPosition
import androidx.compose.material3.TooltipBox
import androidx.compose.material3.TooltipDefaults
import androidx.compose.material3.rememberTooltipState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import dev.nimrag.app.R
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.util.Locale

/** "Now" in epoch ms, refreshed about every 30 s by the app shell so relative times tick. */
val LocalNowMs = compositionLocalOf { System.currentTimeMillis() }

/** "just now", "5 min ago", "3 h ago", "2 d ago", then an absolute date. */
@Composable
fun relativeTime(thenMs: Long): String {
    val minutes = ((LocalNowMs.current - thenMs).coerceAtLeast(0L)) / 60_000L
    return when {
        minutes < 1 -> stringResource(R.string.time_just_now)
        minutes < 60 -> stringResource(R.string.time_min_ago, minutes.toInt())
        minutes < 24 * 60 -> stringResource(R.string.time_hours_ago, (minutes / 60).toInt())
        minutes < 7 * 24 * 60 -> stringResource(R.string.time_days_ago, (minutes / (24 * 60)).toInt())
        else -> absoluteDateTime(thenMs)
    }
}

/** Date and time in the user's locale and 12/24 h preference, e.g. "19 Nov, 12:31". */
@Composable
fun absoluteDateTime(ms: Long): String {
    val context = LocalContext.current
    return remember(ms) {
        DateUtils.formatDateTime(
            context,
            ms,
            DateUtils.FORMAT_SHOW_DATE or DateUtils.FORMAT_SHOW_TIME or DateUtils.FORMAT_ABBREV_MONTH,
        )
    }
}

/** Time of day only, e.g. "12:31". */
@Composable
fun clockTime(ms: Long): String {
    val context = LocalContext.current
    return remember(ms) { DateUtils.formatDateTime(context, ms, DateUtils.FORMAT_SHOW_TIME) }
}

private val LogTimeFormat = DateTimeFormatter.ofPattern("HH:mm:ss")

/** Time of day with seconds, for the log. Fixed 24 h format on purpose: it must line up in a column. */
fun logTime(ms: Long): String =
    LogTimeFormat.format(Instant.ofEpochMilli(ms).atZone(ZoneId.systemDefault()))

/** 512 B, 12.3 KB, 4.6 MB (1024-based). */
fun formatSize(bytes: Long): String = when {
    bytes < 1024L -> "$bytes B"
    bytes < 1024L * 1024L -> String.format(Locale.getDefault(), "%.1f KB", bytes / 1024.0)
    else -> String.format(Locale.getDefault(), "%.1f MB", bytes / (1024.0 * 1024.0))
}

/** Long-press on [content] shows the absolute time of [ms] in a tooltip. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun WithAbsoluteTime(ms: Long, modifier: Modifier = Modifier, content: @Composable () -> Unit) {
    TooltipBox(
        positionProvider = TooltipDefaults.rememberTooltipPositionProvider(TooltipAnchorPosition.Above),
        tooltip = { PlainTooltip { Text(absoluteDateTime(ms)) } },
        state = rememberTooltipState(),
        modifier = modifier,
    ) {
        Box { content() }
    }
}
