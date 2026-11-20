package dev.nimrag.app.ui.components

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.dp
import dev.nimrag.app.ui.theme.status

/** From this font scale on, the button goes under the text instead of squeezing it. */
private const val STACK_FROM_FONT_SCALE = 1.3f

/**
 * One problem in one line, with at most one fix button. Warm container colours from the theme's
 * status tokens; the message reads first, the button is a separate element for TalkBack. The
 * caller decides when it shows, so it disappears by itself once the problem is gone.
 */
@Composable
fun StatusBanner(
    message: String,
    modifier: Modifier = Modifier,
    actionLabel: String? = null,
    onAction: () -> Unit = {},
) {
    val colors = MaterialTheme.status
    val stacked = LocalDensity.current.fontScale >= STACK_FROM_FONT_SCALE
    val text: @Composable (Modifier) -> Unit = {
        Text(message, modifier = it, style = MaterialTheme.typography.bodyMedium, color = colors.onWarnContainer)
    }
    val action: @Composable () -> Unit = {
        if (actionLabel != null) {
            Button(
                onClick = onAction,
                modifier = Modifier.heightIn(min = 48.dp),
                shape = RoundedCornerShape(10.dp),
                colors = ButtonDefaults.buttonColors(
                    containerColor = colors.warn,
                    contentColor = MaterialTheme.colorScheme.onPrimary,
                ),
                contentPadding = ButtonDefaults.TextButtonContentPadding,
            ) { Text(actionLabel, style = MaterialTheme.typography.labelLarge, softWrap = false) }
        }
    }
    val shape = MaterialTheme.shapes.medium
    if (stacked) {
        Column(
            modifier
                .fillMaxWidth()
                .clip(shape)
                .background(colors.warnContainer)
                .padding(horizontal = 16.dp, vertical = 12.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                Icon(Icons.Filled.Warning, contentDescription = null, tint = colors.warn)
                text(Modifier.weight(1f))
            }
            action()
        }
    } else {
        Row(
            modifier
                .fillMaxWidth()
                .clip(shape)
                .background(colors.warnContainer)
                .padding(horizontal = 16.dp, vertical = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Icon(Icons.Filled.Warning, contentDescription = null, tint = colors.warn)
            text(Modifier.weight(1f).padding(vertical = 4.dp))
            action()
        }
    }
}
