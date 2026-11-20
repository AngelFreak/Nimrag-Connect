package dev.nimrag.app.ui.components

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import dev.nimrag.app.ui.theme.NimragColors
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.nimrag.app.R

/** A calm, low-contrast container for one topic. */
@Composable
fun SectionCard(
    modifier: Modifier = Modifier,
    containerColor: Color = MaterialTheme.colorScheme.surfaceContainerLow,
    content: @Composable ColumnScope.() -> Unit,
) {
    Card(
        modifier = modifier.fillMaxWidth(),
        colors = CardDefaults.cardColors(containerColor = containerColor),
    ) {
        Column(
            modifier = Modifier.padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
            content = content,
        )
    }
}

@Composable
fun CardTitle(text: String, modifier: Modifier = Modifier) {
    Text(
        text,
        modifier = modifier,
        style = MaterialTheme.typography.titleMedium,
        color = MaterialTheme.colorScheme.onSurface,
    )
}

/** Icon, title and body in a tinted card, with an optional row of buttons underneath. */
@Composable
fun MessageCard(
    icon: ImageVector,
    title: String,
    containerColor: Color,
    contentColor: Color,
    modifier: Modifier = Modifier,
    actions: @Composable (() -> Unit)? = null,
    body: @Composable ColumnScope.() -> Unit,
) {
    Card(
        modifier = modifier.fillMaxWidth(),
        colors = CardDefaults.cardColors(containerColor = containerColor, contentColor = contentColor),
    ) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                Icon(icon, contentDescription = null)
                Text(title, style = MaterialTheme.typography.titleMedium)
            }
            body()
            if (actions != null) {
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                    actions()
                }
            }
        }
    }
}

/** Label on the left, value on the right. */
@Composable
fun DetailRow(label: String, modifier: Modifier = Modifier, value: @Composable () -> Unit) {
    Row(
        modifier = modifier.fillMaxWidth(),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.SpaceBetween,
    ) {
        Text(
            label,
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(Modifier.width(16.dp))
        value()
    }
}

@Composable
fun DetailRow(label: String, text: String, modifier: Modifier = Modifier) {
    DetailRow(label, modifier) {
        Text(
            text,
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurface,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
    }
}

/** A small rounded status pill: a dot in [dot], the [text], on [container]. */
@Composable
fun StatusChip(text: String, container: Color, content: Color, dot: Color, modifier: Modifier = Modifier) {
    Row(
        modifier = modifier
            .clip(RoundedCornerShape(50))
            .background(container)
            .padding(horizontal = 12.dp, vertical = 6.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Spacer(
            Modifier
                .size(8.dp)
                .clip(CircleShape)
                .background(dot),
        )
        Text(text, style = MaterialTheme.typography.labelLarge, color = content)
    }
}

/** Battery outline filled to [percent]; the number is shown next to it by the caller. */
@Composable
fun BatteryIndicator(percent: Int, color: Color, modifier: Modifier = Modifier) {
    val outline = MaterialTheme.colorScheme.outline
    Canvas(modifier = modifier.size(width = 26.dp, height = 14.dp)) {
        val stroke = 1.5.dp.toPx()
        val capW = 2.dp.toPx()
        val bodyW = size.width - capW
        drawRoundRect(
            color = outline,
            topLeft = Offset(stroke / 2, stroke / 2),
            size = Size(bodyW - stroke, size.height - stroke),
            cornerRadius = CornerRadius(3.dp.toPx()),
            style = androidx.compose.ui.graphics.drawscope.Stroke(width = stroke),
        )
        val inner = 2.5.dp.toPx()
        val fillW = ((bodyW - 2 * inner) * (percent.coerceIn(0, 100) / 100f)).coerceAtLeast(if (percent > 0) 2.dp.toPx() else 0f)
        drawRoundRect(
            color = color,
            topLeft = Offset(inner, inner),
            size = Size(fillW, size.height - 2 * inner),
            cornerRadius = CornerRadius(1.5.dp.toPx()),
        )
        drawLine(
            color = outline,
            start = Offset(size.width - capW / 2, size.height * 0.3f),
            end = Offset(size.width - capW / 2, size.height * 0.7f),
            strokeWidth = capW,
            cap = StrokeCap.Round,
        )
    }
}

/** Four ascending bars; 1 to 4 are filled depending on [rssi] (dBm). */
@Composable
fun SignalBars(rssi: Int, modifier: Modifier = Modifier) {
    val level = when {
        rssi >= -60 -> 4
        rssi >= -70 -> 3
        rssi >= -80 -> 2
        else -> 1
    }
    val description = when (level) {
        4 -> stringResource(R.string.signal_excellent)
        3 -> stringResource(R.string.signal_good)
        2 -> stringResource(R.string.signal_fair)
        else -> stringResource(R.string.signal_weak)
    }
    val on = MaterialTheme.colorScheme.primary
    val off = MaterialTheme.colorScheme.outlineVariant
    Row(
        modifier = modifier
            .height(20.dp)
            .semantics { contentDescription = description },
        horizontalArrangement = Arrangement.spacedBy(2.dp),
        verticalAlignment = Alignment.Bottom,
    ) {
        for (i in 1..4) {
            Spacer(
                Modifier
                    .width(4.dp)
                    .height((5 + i * 4).dp)
                    .clip(RoundedCornerShape(1.dp))
                    .background(if (i <= level) on else off),
            )
        }
    }
}

/** Hides decoration from TalkBack. */
fun Modifier.decorative(): Modifier = clearAndSetSemantics { }

/** The canvas's primary button: 12 dp corners, at least 48 dp tall (44 dp touch target rule). */
@Composable
fun NimragButton(
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    content: @Composable RowScope.() -> Unit,
) {
    val interaction = remember { MutableInteractionSource() }
    val pressed by interaction.collectIsPressedAsState()
    // The web's .btn--primary: the accent fill, the hover colour while pressed, dark text, no shadow or tint.
    Button(
        onClick = onClick,
        modifier = modifier.heightIn(min = 48.dp),
        enabled = enabled,
        shape = RoundedCornerShape(12.dp),
        colors = ButtonDefaults.buttonColors(
            containerColor = if (pressed) NimragColors.AccentHover else NimragColors.Accent,
            contentColor = NimragColors.OnAccent,
        ),
        elevation = ButtonDefaults.buttonElevation(0.dp, 0.dp, 0.dp, 0.dp, 0.dp),
        contentPadding = PaddingValues(horizontal = 20.dp, vertical = 10.dp),
        interactionSource = interaction,
        content = content,
    )
}

/** Secondary button: same shape, outlined with the canvas's line colour. */
@Composable
fun NimragOutlinedButton(
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    colors: androidx.compose.material3.ButtonColors = ButtonDefaults.outlinedButtonColors(),
    content: @Composable RowScope.() -> Unit,
) {
    OutlinedButton(
        onClick = onClick,
        modifier = modifier.heightIn(min = 48.dp),
        enabled = enabled,
        shape = RoundedCornerShape(12.dp),
        colors = colors,
        border = BorderStroke(1.dp, MaterialTheme.colorScheme.outline),
        contentPadding = PaddingValues(horizontal = 20.dp, vertical = 10.dp),
        content = content,
    )
}
