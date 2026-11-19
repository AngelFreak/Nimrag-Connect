package dev.nimrag.app.ui.settings

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.LinkAnnotation
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextLinkStyles
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.withLink
import androidx.compose.ui.unit.dp
import dev.nimrag.app.R
import dev.nimrag.app.ui.components.SectionCard

@Composable
fun LicencesScreen(contentPadding: PaddingValues, modifier: Modifier = Modifier) {
    Column(
        modifier = modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(contentPadding)
            .padding(horizontal = 16.dp, vertical = 8.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Text(
            stringResource(R.string.licences_intro),
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        ComponentSection(stringResource(R.string.licences_app), listOf(NimragComponent))
        ComponentSection(stringResource(R.string.licences_libraries), BundledLibraries)
        ComponentSection(stringResource(R.string.licences_rust, RustCrates.size), RustCrates)
    }
}

@Composable
private fun ComponentSection(title: String, components: List<Component>) {
    SectionCard {
        Text(title, style = MaterialTheme.typography.titleMedium)
        Column {
            components.forEachIndexed { index, component ->
                if (index > 0) HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
                ComponentRow(component)
            }
        }
    }
}

@Composable
private fun ComponentRow(component: Component) {
    Column(
        Modifier
            .fillMaxWidth()
            .padding(vertical = 8.dp),
        verticalArrangement = Arrangement.spacedBy(2.dp),
    ) {
        Text(
            if (component.version.isEmpty()) component.name else "${component.name} ${component.version}",
            style = MaterialTheme.typography.bodyLarge,
            color = MaterialTheme.colorScheme.onSurface,
        )
        Text(
            licenceLinks(component.licence),
            style = MaterialTheme.typography.bodySmall.copy(color = MaterialTheme.colorScheme.onSurfaceVariant),
        )
    }
}

private val SpdxToken = Regex("[A-Za-z][A-Za-z0-9.+-]*")
private val SpdxOperators = setOf("OR", "AND", "WITH")

/** An SPDX expression with every identifier linked to https://spdx.org/licenses/<id>.html. */
@Composable
internal fun licenceLinks(expression: String): AnnotatedString {
    val linkColor = MaterialTheme.colorScheme.primary
    return remember(expression, linkColor) {
        val style = TextLinkStyles(SpanStyle(color = linkColor, textDecoration = TextDecoration.Underline))
        buildAnnotatedString {
            var last = 0
            for (match in SpdxToken.findAll(expression)) {
                append(expression.substring(last, match.range.first))
                val token = match.value
                if (token in SpdxOperators) {
                    append(token)
                } else {
                    val id = token.removeSuffix("+")
                    withLink(LinkAnnotation.Url("https://spdx.org/licenses/$id.html", style)) { append(token) }
                }
                last = match.range.last + 1
            }
            append(expression.substring(last))
        }
    }
}
