package dev.nimrag.app.ui.settings

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.consumeWindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowRight
import androidx.compose.material.icons.filled.KeyboardArrowDown
import androidx.compose.material.icons.filled.KeyboardArrowUp
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalInspectionMode
import androidx.compose.ui.platform.LocalUriHandler
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import dev.nimrag.app.R
import dev.nimrag.app.model.AppState
import dev.nimrag.app.model.GarminConnectMode
import dev.nimrag.app.model.ServerTest
import dev.nimrag.app.model.Settings
import dev.nimrag.app.model.WatchController
import dev.nimrag.app.ui.components.CardTitle
import dev.nimrag.app.ui.components.DetailRow
import dev.nimrag.app.ui.components.SectionCard
import dev.nimrag.app.ui.theme.status
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch
import uniffi.nimrag_ffi.ffiVersion

internal const val SOURCE_URL = "https://github.com/AngelFreak/Nimrag-Connect"

private val IntervalOptions = listOf(0, 15, 30, 60, 120, 240)

@Composable
fun SettingsScreen(
    state: AppState,
    controller: WatchController,
    contentPadding: PaddingValues,
    onOpenLicences: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Column(
        modifier = modifier
            .fillMaxSize()
            .padding(contentPadding)
            .consumeWindowInsets(contentPadding)
            .imePadding()
            .verticalScroll(rememberScrollState())
            .padding(horizontal = 16.dp, vertical = 8.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        ServerCard(state.settings, controller)
        SyncIntervalCard(state.settings.autoSyncMinutes, controller::setAutoSyncMinutes)
        GarminConnectModeCard(state.settings.garminConnectMode, controller::setGarminConnectMode)
        AdvancedCard(state.settings.capabilitiesHex, controller::setCapabilitiesHex)
        AboutCard(onOpenLicences)
    }
}

// ---------------------------------------------------------------- server

private val UrlPattern = Regex("^https?://\\S+$", RegexOption.IGNORE_CASE)

@Composable
internal fun ServerCard(settings: Settings, controller: WatchController, modifier: Modifier = Modifier) {
    var url by rememberSaveable(settings.serverUrl) { mutableStateOf(settings.serverUrl) }
    // The token is deliberately not saveable: it must not end up in the saved instance state.
    var token by remember { mutableStateOf("") }
    var allowMobile by rememberSaveable(settings.allowMobileData) { mutableStateOf(settings.allowMobileData) }
    var justSaved by remember { mutableStateOf(false) }
    var testing by remember { mutableStateOf(false) }
    var testResult by remember { mutableStateOf<ServerTest?>(null) }
    val testErrorText = stringResource(R.string.server_test_error)
    val scope = rememberCoroutineScope()

    val trimmed = url.trim()
    val urlValid = trimmed.isEmpty() || UrlPattern.matches(trimmed)
    val dirty = trimmed != settings.serverUrl || token.isNotEmpty() || allowMobile != settings.allowMobileData

    SectionCard(modifier) {
        CardTitle(stringResource(R.string.settings_server))
        OutlinedTextField(
            value = url,
            onValueChange = {
                url = it
                justSaved = false
            },
            modifier = Modifier.fillMaxWidth(),
            label = { Text(stringResource(R.string.server_url)) },
            placeholder = { Text(stringResource(R.string.server_url_placeholder)) },
            singleLine = true,
            isError = !urlValid,
            supportingText = if (!urlValid) ({ Text(stringResource(R.string.server_url_invalid)) }) else null,
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri, autoCorrectEnabled = false),
        )
        OutlinedTextField(
            value = token,
            onValueChange = {
                token = it
                justSaved = false
            },
            modifier = Modifier.fillMaxWidth(),
            label = { Text(stringResource(R.string.server_token)) },
            placeholder = if (settings.hasToken) ({ Text(stringResource(R.string.server_token_saved)) }) else null,
            supportingText = {
                Text(stringResource(if (settings.hasToken) R.string.server_token_keep else R.string.server_token_none))
            },
            singleLine = true,
            visualTransformation = PasswordVisualTransformation(),
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password, autoCorrectEnabled = false),
        )
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .clickable(role = Role.Switch) {
                    allowMobile = !allowMobile
                    justSaved = false
                },
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            Column(Modifier.weight(1f)) {
                Text(stringResource(R.string.server_mobile_data), style = MaterialTheme.typography.bodyLarge)
                Text(
                    stringResource(R.string.server_mobile_data_hint),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            Switch(checked = allowMobile, onCheckedChange = null)
        }
        Row(horizontalArrangement = Arrangement.spacedBy(12.dp), verticalAlignment = Alignment.CenterVertically) {
            Button(
                onClick = {
                    controller.saveServer(trimmed, token.ifEmpty { null }, allowMobile)
                    token = ""
                    testResult = null
                    justSaved = true
                },
                enabled = dirty && urlValid,
            ) { Text(stringResource(R.string.save)) }
            OutlinedButton(
                onClick = {
                    scope.launch {
                        testing = true
                        testResult = null
                        testResult = try {
                            controller.testServer()
                        } catch (e: CancellationException) {
                            throw e
                        } catch (e: Exception) {
                            ServerTest.Failed(testErrorText)
                        } finally {
                            testing = false
                        }
                    }
                },
                enabled = !testing && !dirty && settings.serverUrl.isNotBlank(),
            ) { Text(stringResource(R.string.server_test)) }
            if (testing) CircularProgressIndicator(Modifier.padding(start = 4.dp), strokeWidth = 3.dp)
        }
        if (dirty) {
            Text(
                stringResource(R.string.server_save_first),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        if (justSaved && !dirty) {
            Text(
                stringResource(R.string.server_saved),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.status.good,
            )
        }
        when (val result = testResult) {
            null -> Unit
            ServerTest.Ok -> Text(
                stringResource(R.string.server_test_ok),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.status.good,
            )
            is ServerTest.Failed -> Text(
                stringResource(R.string.server_test_failed, result.reason),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.error,
            )
        }
        if (settings.hasToken) {
            TextButton(
                onClick = {
                    controller.saveServer(settings.serverUrl, "", settings.allowMobileData)
                    testResult = null
                },
            ) { Text(stringResource(R.string.server_token_remove), color = MaterialTheme.colorScheme.error) }
        }
    }
}

// ---------------------------------------------------------------- sync interval

@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun SyncIntervalCard(minutes: Int, onSelect: (Int) -> Unit, modifier: Modifier = Modifier) {
    SectionCard(modifier) {
        CardTitle(stringResource(R.string.settings_sync))
        Text(stringResource(R.string.auto_sync_label), style = MaterialTheme.typography.bodyLarge)
        FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            // Keep a value the engine holds that is not in the list visible, instead of hiding it.
            val options = if (minutes in IntervalOptions) IntervalOptions else (IntervalOptions + minutes).sorted()
            options.forEach { option ->
                FilterChip(
                    selected = option == minutes,
                    onClick = { onSelect(option) },
                    label = {
                        Text(if (option == 0) stringResource(R.string.interval_off) else stringResource(R.string.interval_minutes, option))
                    },
                )
            }
        }
        Text(
            stringResource(if (minutes == 0) R.string.auto_sync_off_hint else R.string.auto_sync_hint),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

// ---------------------------------------------------------------- Garmin Connect

@Composable
internal fun GarminConnectModeCard(
    mode: GarminConnectMode,
    onSelect: (GarminConnectMode) -> Unit,
    modifier: Modifier = Modifier,
) {
    SectionCard(modifier) {
        CardTitle(stringResource(R.string.settings_garmin_connect))
        Column(Modifier.selectableGroup(), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            ModeOption(
                selected = mode == GarminConnectMode.Off,
                title = stringResource(R.string.garmin_mode_off),
                hint = stringResource(R.string.garmin_mode_off_hint),
                onClick = { onSelect(GarminConnectMode.Off) },
            )
            ModeOption(
                selected = mode == GarminConnectMode.Alongside,
                title = stringResource(R.string.garmin_mode_alongside),
                hint = stringResource(R.string.garmin_mode_alongside_hint),
                onClick = { onSelect(GarminConnectMode.Alongside) },
            )
        }
        Text(
            stringResource(R.string.garmin_mode_note),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        if (mode == GarminConnectMode.Alongside) {
            Text(
                stringResource(R.string.garmin_mode_alongside_note),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

@Composable
private fun ModeOption(selected: Boolean, title: String, hint: String, onClick: () -> Unit) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .selectable(selected = selected, onClick = onClick, role = Role.RadioButton)
            .padding(vertical = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        RadioButton(selected = selected, onClick = null)
        Column(Modifier.weight(1f)) {
            Text(title, style = MaterialTheme.typography.bodyLarge)
            Text(
                hint,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

// ---------------------------------------------------------------- advanced

@Composable
internal fun AdvancedCard(capabilitiesHex: String, onSave: (String) -> Boolean, modifier: Modifier = Modifier) {
    var expanded by rememberSaveable { mutableStateOf(false) }
    var hex by rememberSaveable(capabilitiesHex) { mutableStateOf(capabilitiesHex) }
    var error by remember { mutableStateOf(false) }
    var saved by remember { mutableStateOf(false) }

    SectionCard(modifier) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .clickable(role = Role.Button) { expanded = !expanded },
            verticalAlignment = Alignment.CenterVertically,
        ) {
            CardTitle(stringResource(R.string.settings_advanced), Modifier.weight(1f))
            Icon(
                if (expanded) Icons.Filled.KeyboardArrowUp else Icons.Filled.KeyboardArrowDown,
                contentDescription = stringResource(if (expanded) R.string.collapse else R.string.expand),
            )
        }
        AnimatedVisibility(expanded) {
            Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                OutlinedTextField(
                    value = hex,
                    onValueChange = {
                        hex = it
                        error = false
                        saved = false
                    },
                    modifier = Modifier.fillMaxWidth(),
                    label = { Text(stringResource(R.string.capabilities_label)) },
                    singleLine = true,
                    isError = error,
                    textStyle = MaterialTheme.typography.bodyLarge.copy(fontFamily = FontFamily.Monospace),
                    supportingText = {
                        Text(stringResource(if (error) R.string.capabilities_invalid else R.string.capabilities_hint))
                    },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Ascii, autoCorrectEnabled = false),
                )
                Row(horizontalArrangement = Arrangement.spacedBy(12.dp), verticalAlignment = Alignment.CenterVertically) {
                    Button(
                        onClick = {
                            val ok = onSave(hex.trim())
                            error = !ok
                            saved = ok
                        },
                        enabled = hex.trim() != capabilitiesHex,
                    ) { Text(stringResource(R.string.save)) }
                    if (saved) {
                        Text(
                            stringResource(R.string.capabilities_saved),
                            style = MaterialTheme.typography.bodyMedium,
                            color = MaterialTheme.status.good,
                        )
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------- about

@Composable
internal fun AboutCard(onOpenLicences: () -> Unit, modifier: Modifier = Modifier) {
    val context = LocalContext.current
    val preview = LocalInspectionMode.current
    val uriHandler = LocalUriHandler.current
    val version = remember {
        if (preview) {
            "0.1.0 (1)"
        } else {
            runCatching {
                // The flags overload needs API 33; the plain one works everywhere (minSdk is 31).
                @Suppress("DEPRECATION")
                val info = context.packageManager.getPackageInfo(context.packageName, 0)
                "${info.versionName} (${info.longVersionCode})"
            }.getOrDefault("?")
        }
    }
    // The native library cannot load in a preview; anywhere else a failure here is worth showing.
    val coreVersion = remember { if (preview) "preview" else runCatching { ffiVersion() }.getOrElse { "unavailable" } }

    SectionCard(modifier) {
        CardTitle(stringResource(R.string.settings_about))
        DetailRow(stringResource(R.string.app_name), version)
        DetailRow(stringResource(R.string.about_core), coreVersion)
        DetailRow(stringResource(R.string.about_licence), "AGPL-3.0-or-later")
        Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
            TextButton(onClick = { runCatching { uriHandler.openUri(SOURCE_URL) } }) {
                Text(stringResource(R.string.about_source))
            }
        }
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .clickable(role = Role.Button, onClick = onOpenLicences)
                .padding(vertical = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                stringResource(R.string.about_open_source),
                style = MaterialTheme.typography.bodyLarge,
                modifier = Modifier.weight(1f),
            )
            Icon(Icons.AutoMirrored.Filled.KeyboardArrowRight, contentDescription = null)
        }
    }
}
