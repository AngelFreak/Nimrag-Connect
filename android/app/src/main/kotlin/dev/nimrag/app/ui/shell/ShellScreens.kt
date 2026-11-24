package dev.nimrag.app.ui.shell

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
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
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import dev.nimrag.app.R
import dev.nimrag.app.ui.components.CardTitle
import dev.nimrag.app.ui.components.NimragButton
import dev.nimrag.app.ui.components.SectionCard
import dev.nimrag.app.ui.shell.logic.LocalCopyText

/**
 * Laid over the web view when the server cannot be reached and the PWA has no cached copy of the
 * page (with a cached copy the web view just shows it).
 */
@Composable
fun ServerUnreachableScreen(retrying: Boolean, onRetry: () -> Unit, modifier: Modifier = Modifier) {
    Box(
        modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(20.dp),
        contentAlignment = Alignment.Center,
    ) {
        SectionCard {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                Icon(Icons.Filled.Warning, contentDescription = null, tint = MaterialTheme.colorScheme.error)
                CardTitle(stringResource(R.string.server_unreachable_title))
            }
            Text(
                stringResource(R.string.server_unreachable_body),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            if (retrying) {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    CircularProgressIndicator(modifier = Modifier.size(24.dp), strokeWidth = 3.dp)
                    Text(stringResource(R.string.server_unreachable_retrying), style = MaterialTheme.typography.bodyMedium)
                }
            } else {
                NimragButton(onClick = onRetry) { Text(stringResource(R.string.server_unreachable_retry)) }
            }
        }
    }
}

/** Stand-in for the web view in Compose previews, which cannot host one. */
@Composable
fun WebPreviewPlaceholder(modifier: Modifier = Modifier) {
    Box(modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
        Column(horizontalAlignment = Alignment.CenterHorizontally) {
            Text(
                stringResource(R.string.web_preview_placeholder),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

/**
 * Above the web view while it shows the phone's own copy: says so (and how many changes made on it
 * wait for the server), and offers the server.
 */
@Composable
fun LocalCopyBanner(pendingEdits: Int, onTryServer: () -> Unit, modifier: Modifier = Modifier) {
    Row(
        modifier
            .fillMaxWidth()
            .padding(horizontal = 16.dp, vertical = 2.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.SpaceBetween,
    ) {
        val text = when (val shown = LocalCopyText.of(pendingEdits)) {
            LocalCopyText.Plain -> stringResource(R.string.local_copy_banner)
            is LocalCopyText.Waiting -> pluralStringResource(R.plurals.local_copy_banner_waiting, shown.count, shown.count)
        }
        Text(
            text,
            modifier = Modifier.weight(1f),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        TextButton(onClick = onTryServer) { Text(stringResource(R.string.local_copy_try_server)) }
    }
}

/**
 * A change made offline that the server refused ("A change made offline could not be saved: …").
 * Shown until the person says OK; [more] says how many messages follow it.
 */
@Composable
fun EditNoticeBanner(message: String, more: Int, onDismiss: () -> Unit, modifier: Modifier = Modifier) {
    Row(
        modifier
            .fillMaxWidth()
            .padding(horizontal = 16.dp, vertical = 2.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Icon(Icons.Filled.Warning, contentDescription = null, tint = MaterialTheme.colorScheme.error)
        Text(
            if (more > 0) stringResource(R.string.edit_notice_more, message, more) else message,
            modifier = Modifier.weight(1f),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.error,
        )
        TextButton(onClick = onDismiss) { Text(stringResource(R.string.edit_notice_ok)) }
    }
}
