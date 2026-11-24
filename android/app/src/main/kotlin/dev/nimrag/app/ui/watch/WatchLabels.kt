package dev.nimrag.app.ui.watch

import android.content.pm.PackageInfo
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalInspectionMode
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import dev.nimrag.app.R
import dev.nimrag.app.model.HistoryRetention

/** "Off", "Every 15 min", "Every hour", "Every 4 hours": the interval as a row value and as a choice. */
@Composable
internal fun intervalLabel(minutes: Int): String = when {
    minutes <= 0 -> stringResource(R.string.interval_off)
    minutes == 60 -> stringResource(R.string.interval_hour)
    minutes % 60 == 0 -> pluralStringResource(R.plurals.interval_hours, minutes / 60, minutes / 60)
    else -> stringResource(R.string.interval_minutes, minutes)
}

/** "Off", "Last 3 months", "Last 12 months", "Everything": the history choice as a row value and as a choice. */
@Composable
internal fun historyRetentionLabel(choice: HistoryRetention): String = stringResource(
    when (choice) {
        HistoryRetention.Off -> R.string.history_retention_off
        HistoryRetention.Last3Months -> R.string.history_retention_3m
        HistoryRetention.Last12Months -> R.string.history_retention_12m
        HistoryRetention.Everything -> R.string.history_retention_all
    },
)

/** The app's version name ("0.5.1"); a fixed one in previews, where there is no package. */
@Composable
internal fun appVersionName(): String = packageVersion(preview = "0.1.0") { it.versionName ?: "?" }

/** Version name and code ("0.5.1 (11)"). */
@Composable
internal fun appVersionFull(): String = packageVersion(preview = "0.1.0 (1)") { "${it.versionName} (${it.longVersionCode})" }

@Composable
private fun packageVersion(preview: String, format: (PackageInfo) -> String): String {
    val context = LocalContext.current
    val inPreview = LocalInspectionMode.current
    return remember(inPreview) {
        if (inPreview) {
            preview
        } else {
            runCatching {
                // The flags overload needs API 33; the plain one works everywhere (minSdk is 31).
                @Suppress("DEPRECATION")
                format(context.packageManager.getPackageInfo(context.packageName, 0))
            }.getOrDefault("?")
        }
    }
}
