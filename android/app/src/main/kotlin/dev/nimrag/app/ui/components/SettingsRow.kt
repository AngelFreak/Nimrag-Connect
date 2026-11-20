package dev.nimrag.app.ui.components

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowRight
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Switch
import androidx.compose.material3.SwitchDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading as semanticsHeading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.nimrag.app.R

/** What a [SettingsRow] shows at its end, and with it how the row behaves. */
sealed interface RowTrailing {
    /** Nothing: an information row, or a row whose whole point is its click. */
    data object None : RowTrailing

    /** A chevron: the row opens another page. */
    data object Chevron : RowTrailing

    /** A switch; the whole row toggles it. */
    data class Toggle(val checked: Boolean, val onChange: (Boolean) -> Unit) : RowTrailing

    /** A radio button, for a choice of one among a few; put the rows in a [SettingsGroup]. */
    data class Radio(val selected: Boolean) : RowTrailing

    /** Anything small: a signal indicator, a status word. */
    class Custom(val content: @Composable () -> Unit) : RowTrailing
}

/** Danger rows (forget, sign out) carry the error colour in their title and icon. */
enum class RowTone { Normal, Danger }

/**
 * One line of a settings list: an optional icon in a tinted rounded square, a title, the current
 * value in one short line under it, and a [trailing] chevron, switch or radio. At least 56 dp
 * tall with 16 dp on both sides. The row is one element for TalkBack: title and value are read
 * together with its role and state. A row without [onClick] (and without a switch or radio) is
 * plain information.
 */
@Composable
fun SettingsRow(
    title: String,
    modifier: Modifier = Modifier,
    value: String? = null,
    icon: ImageVector? = null,
    trailing: RowTrailing = RowTrailing.None,
    tone: RowTone = RowTone.Normal,
    valueColor: Color = Color.Unspecified,
    onClick: (() -> Unit)? = null,
) {
    val onText = stringResource(R.string.switch_on)
    val offText = stringResource(R.string.switch_off)
    val behaviour = when {
        trailing is RowTrailing.Toggle -> Modifier
            .toggleable(
                value = trailing.checked,
                role = Role.Switch,
                onValueChange = trailing.onChange,
            )
            .semantics { stateDescription = if (trailing.checked) onText else offText }
        trailing is RowTrailing.Radio -> Modifier.selectable(
            selected = trailing.selected,
            role = Role.RadioButton,
            onClick = onClick ?: {},
        )
        onClick != null -> Modifier.clickable(role = Role.Button, onClick = onClick)
        else -> Modifier.semantics(mergeDescendants = true) {}
    }
    Row(
        modifier = modifier
            .fillMaxWidth()
            .then(behaviour)
            .heightIn(min = 56.dp)
            .padding(horizontal = 16.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        if (icon != null) IconTile(icon, tone)
        Column(Modifier.weight(1f)) {
            Text(
                title,
                style = MaterialTheme.typography.bodyLarge.copy(fontWeight = FontWeight.Medium),
                color = if (tone == RowTone.Danger) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurface,
            )
            if (value != null) {
                Text(
                    value,
                    style = MaterialTheme.typography.bodyMedium,
                    color = if (valueColor == Color.Unspecified) MaterialTheme.colorScheme.onSurfaceVariant else valueColor,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
        when (trailing) {
            RowTrailing.None -> Unit
            RowTrailing.Chevron -> Icon(
                Icons.AutoMirrored.Filled.KeyboardArrowRight,
                contentDescription = null,
                tint = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            is RowTrailing.Toggle -> Switch(
                checked = trailing.checked,
                onCheckedChange = null,
                colors = SwitchDefaults.colors(
                    checkedThumbColor = MaterialTheme.colorScheme.primary,
                    checkedTrackColor = MaterialTheme.colorScheme.primaryContainer,
                    checkedBorderColor = MaterialTheme.colorScheme.primary,
                ),
            )
            is RowTrailing.Radio -> RadioButton(selected = trailing.selected, onClick = null)
            is RowTrailing.Custom -> trailing.content()
        }
    }
}

/** The rounded square behind a row's icon. */
@Composable
private fun IconTile(icon: ImageVector, tone: RowTone) {
    val danger = tone == RowTone.Danger
    Box(
        Modifier
            .size(36.dp)
            .background(
                if (danger) MaterialTheme.colorScheme.errorContainer else MaterialTheme.colorScheme.surfaceContainerHigh,
                RoundedCornerShape(10.dp),
            ),
        contentAlignment = Alignment.Center,
    ) {
        Icon(
            icon,
            contentDescription = null,
            modifier = Modifier.size(20.dp),
            tint = if (danger) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.primary,
        )
    }
}

/** The rows of a [SettingsGroup]; the group puts a hairline between them. */
class SettingsGroupScope internal constructor() {
    internal val items = mutableListOf<@Composable () -> Unit>()

    /** One row (or any block that belongs between two hairlines). Skip it by not calling it. */
    fun item(content: @Composable () -> Unit) {
        items += content
    }
}

/**
 * A small uppercase [heading], one rounded card holding the rows, and an optional one-line
 * [footer] under the card. Rows are added with `item { SettingsRow(...) }` so that rows left
 * out (a missing permission, a hidden option) never leave a stray hairline.
 */
@Composable
fun SettingsGroup(
    modifier: Modifier = Modifier,
    heading: String? = null,
    footer: String? = null,
    content: SettingsGroupScope.() -> Unit,
) {
    val rows = SettingsGroupScope().apply(content).items
    Column(modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        if (heading != null) {
            Text(
                heading.uppercase(),
                modifier = Modifier
                    .padding(horizontal = 4.dp)
                    .semantics { semanticsHeading() },
                style = MaterialTheme.typography.labelMedium.copy(letterSpacing = 0.8.sp),
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        GroupCard {
            rows.forEachIndexed { index, row ->
                if (index > 0) HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
                row()
            }
        }
        if (footer != null) Footnote(footer)
    }
}

/** The card behind a group: the same container as [SectionCard], rows flush to its edges. */
@Composable
fun GroupCard(modifier: Modifier = Modifier, content: @Composable ColumnScope.() -> Unit) {
    Card(
        modifier = modifier.fillMaxWidth(),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerLow),
    ) {
        Column(content = content)
    }
}

/** A short explanatory line under a card; the only place longer text lives on a settings page. */
@Composable
fun Footnote(text: String, modifier: Modifier = Modifier) {
    Text(
        text,
        modifier = modifier.padding(horizontal = 4.dp),
        style = MaterialTheme.typography.bodySmall,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
    )
}
