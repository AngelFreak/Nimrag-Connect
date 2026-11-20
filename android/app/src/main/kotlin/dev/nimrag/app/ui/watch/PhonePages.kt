package dev.nimrag.app.ui.watch

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalInspectionMode
import androidx.compose.ui.platform.LocalUriHandler
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import dev.nimrag.app.R
import dev.nimrag.app.engine.auth.logic.AuthPhase
import dev.nimrag.app.model.AccountState
import dev.nimrag.app.model.ServerTest
import dev.nimrag.app.model.Settings
import dev.nimrag.app.model.WatchController
import dev.nimrag.app.ui.components.CodeIcon
import dev.nimrag.app.ui.components.ConfirmDialog
import dev.nimrag.app.ui.components.DocumentIcon
import dev.nimrag.app.ui.components.Footnote
import dev.nimrag.app.ui.components.NimragButton
import dev.nimrag.app.ui.components.NimragOutlinedButton
import dev.nimrag.app.ui.components.PageColumn
import dev.nimrag.app.ui.components.RowTone
import dev.nimrag.app.ui.components.RowTrailing
import dev.nimrag.app.ui.components.SectionCard
import dev.nimrag.app.ui.components.ServerIcon
import dev.nimrag.app.ui.components.SettingsGroup
import dev.nimrag.app.ui.components.SettingsPage
import dev.nimrag.app.ui.components.SettingsRow
import dev.nimrag.app.ui.components.ShieldIcon
import dev.nimrag.app.ui.components.SignOutIcon
import dev.nimrag.app.ui.components.SlidersIcon
import dev.nimrag.app.ui.theme.status
import dev.nimrag.app.ui.watch.logic.ServerHost
import dev.nimrag.app.ui.watch.logic.WatchPage
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch
import uniffi.nimrag_ffi.ffiVersion

internal const val SOURCE_URL = "https://github.com/AngelFreak/Nimrag-Connect"

// ---------------------------------------------------------------- server

/**
 * The server this phone uploads to: its address as a host, whether the last test reached it, and
 * the test itself. The only way to connect is signing in (the sign-in screen asks for the address);
 * the phone's upload token is issued then and kept out of sight, so there is nothing to paste here.
 */
@Composable
internal fun ServerPage(
    settings: Settings,
    controller: WatchController,
    onSignIn: () -> Unit,
    onBack: () -> Unit,
) {
    var testing by remember { mutableStateOf(false) }
    var testResult by remember { mutableStateOf<ServerTest?>(null) }
    val testErrorText = stringResource(R.string.server_test_error)
    val scope = rememberCoroutineScope()
    val configured = settings.serverUrl.isNotBlank()

    SettingsPage(title = stringResource(R.string.settings_server), onBack = onBack) {
        PageColumn {
            SettingsGroup {
                item {
                    SettingsRow(
                        title = stringResource(R.string.server_address),
                        value = if (configured) ServerHost.of(settings.serverUrl) else stringResource(R.string.server_not_set_up),
                        icon = ServerIcon,
                    )
                }
                if (configured) {
                    item {
                        val result = testResult
                        SettingsRow(
                            title = stringResource(R.string.server_status),
                            value = when {
                                testing -> stringResource(R.string.server_status_testing)
                                result == null -> stringResource(R.string.server_status_untested)
                                result == ServerTest.Ok -> stringResource(R.string.server_test_ok)
                                result is ServerTest.Failed -> stringResource(R.string.server_test_failed, result.reason)
                                else -> null
                            },
                            valueColor = when (result) {
                                ServerTest.Ok -> MaterialTheme.status.good
                                is ServerTest.Failed -> MaterialTheme.colorScheme.error
                                else -> Color.Unspecified
                            },
                        )
                    }
                }
            }
            if (configured) {
                Row(horizontalArrangement = Arrangement.spacedBy(12.dp), verticalAlignment = Alignment.CenterVertically) {
                    NimragOutlinedButton(
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
                        enabled = !testing && settings.hasToken,
                    ) { Text(stringResource(R.string.server_test)) }
                    if (testing) CircularProgressIndicator(Modifier.padding(start = 4.dp), strokeWidth = 3.dp)
                }
            } else {
                Footnote(stringResource(R.string.server_sign_in_hint))
                NimragButton(onClick = onSignIn, modifier = Modifier.fillMaxWidth()) { Text(stringResource(R.string.account_sign_in)) }
            }
        }
    }
}

// ---------------------------------------------------------------- account

/** Who is signed in on this phone, and Sign out (which asks first and says what it does). */
@Composable
internal fun AccountPage(
    account: AccountState,
    controller: WatchController,
    onSignIn: () -> Unit,
    onBack: () -> Unit,
) {
    var confirmSignOut by rememberSaveable { mutableStateOf(false) }
    val signedIn = account.phase == AuthPhase.SignedIn || account.phase == AuthPhase.Locked

    SettingsPage(title = stringResource(R.string.account_title), onBack = onBack) {
        PageColumn {
            if (signedIn) {
                SettingsGroup {
                    item {
                        SettingsRow(
                            title = account.username?.let { stringResource(R.string.account_signed_in_as, it) }
                                ?: stringResource(R.string.account_signed_in),
                            value = when {
                                account.phase == AuthPhase.Locked -> stringResource(R.string.account_value_locked)
                                account.biometricReady -> stringResource(R.string.account_value_fingerprint)
                                else -> stringResource(R.string.account_value_uploading)
                            },
                            icon = ShieldIcon,
                        )
                    }
                }
                SettingsGroup {
                    item {
                        SettingsRow(
                            title = stringResource(R.string.account_sign_out),
                            icon = SignOutIcon,
                            tone = RowTone.Danger,
                            // While the server call runs a second tap would do nothing useful.
                            onClick = if (account.busy) null else ({ confirmSignOut = true }),
                        )
                    }
                }
            } else {
                Footnote(stringResource(R.string.account_state_out))
                NimragButton(onClick = onSignIn, modifier = Modifier.fillMaxWidth()) { Text(stringResource(R.string.account_sign_in)) }
            }
        }
    }

    if (confirmSignOut) {
        ConfirmDialog(
            title = stringResource(R.string.account_sign_out_confirm),
            body = stringResource(R.string.account_sign_out_hint),
            confirmLabel = stringResource(R.string.account_sign_out_yes),
            destructive = true,
            onConfirm = {
                confirmSignOut = false
                controller.account.signOut()
            },
            onDismiss = { confirmSignOut = false },
        )
    }
}

// ---------------------------------------------------------------- about

/** Version, licence and links; the builder's Advanced page hangs off it. */
@Composable
internal fun AboutPage(onOpen: (WatchPage) -> Unit, onBack: () -> Unit) {
    val preview = LocalInspectionMode.current
    val uriHandler = LocalUriHandler.current
    val version = appVersionFull()
    // The native library cannot load in a preview; anywhere else a failure here is worth showing.
    val coreVersion = remember { if (preview) "preview" else runCatching { ffiVersion() }.getOrElse { "unavailable" } }

    SettingsPage(title = stringResource(R.string.settings_about_app), onBack = onBack) {
        PageColumn {
            SettingsGroup {
                item { SettingsRow(title = stringResource(R.string.app_name), value = version) }
                item { SettingsRow(title = stringResource(R.string.about_core), value = coreVersion) }
                item { SettingsRow(title = stringResource(R.string.about_licence), value = "AGPL-3.0-or-later") }
            }
            SettingsGroup {
                item {
                    SettingsRow(
                        title = stringResource(R.string.about_source),
                        icon = CodeIcon,
                        onClick = { runCatching { uriHandler.openUri(SOURCE_URL) } },
                    )
                }
                item {
                    SettingsRow(
                        title = stringResource(R.string.about_open_source),
                        icon = DocumentIcon,
                        trailing = RowTrailing.Chevron,
                        onClick = { onOpen(WatchPage.Licences) },
                    )
                }
                item {
                    SettingsRow(
                        title = stringResource(R.string.settings_advanced),
                        value = stringResource(R.string.advanced_value),
                        icon = SlidersIcon,
                        trailing = RowTrailing.Chevron,
                        onClick = { onOpen(WatchPage.Advanced) },
                    )
                }
            }
        }
    }
}

/** The builder's capability-mask editor (the old Advanced card), on its own page. */
@Composable
internal fun AdvancedPage(capabilitiesHex: String, onSave: (String) -> Boolean, onBack: () -> Unit) {
    var hex by rememberSaveable(capabilitiesHex) { mutableStateOf(capabilitiesHex) }
    var error by remember { mutableStateOf(false) }
    var saved by remember { mutableStateOf(false) }

    SettingsPage(title = stringResource(R.string.settings_advanced), onBack = onBack) {
        PageColumn {
            SectionCard {
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
                    NimragButton(
                        onClick = {
                            val ok = onSave(hex.trim())
                            error = !ok
                            saved = ok
                        },
                        enabled = hex.trim() != capabilitiesHex,
                    ) { Text(stringResource(R.string.save)) }
                    AnimatedVisibility(saved) {
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

// ---------------------------------------------------------------- help

/** "Trouble connecting?": the usual causes in four short lines, with the way to the two places that help. */
@Composable
internal fun HelpPage(onOpen: (WatchPage) -> Unit, onBack: () -> Unit) {
    SettingsPage(title = stringResource(R.string.help_title), onBack = onBack) {
        PageColumn {
            SectionCard {
                listOf(R.string.help_step_close, R.string.help_step_bluetooth, R.string.help_step_pairing_mode, R.string.help_step_one_app)
                    .forEachIndexed { index, text ->
                        Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                            Text(
                                "${index + 1}.",
                                style = MaterialTheme.typography.bodyMedium,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                            Text(stringResource(text), Modifier.weight(1f), style = MaterialTheme.typography.bodyMedium)
                        }
                    }
            }
            SettingsGroup {
                item {
                    SettingsRow(
                        title = stringResource(R.string.garmin_help_title),
                        trailing = RowTrailing.Chevron,
                        onClick = { onOpen(WatchPage.GarminHelp) },
                    )
                }
                item {
                    SettingsRow(
                        title = stringResource(R.string.help_log_row),
                        value = stringResource(R.string.help_log_value),
                        trailing = RowTrailing.Chevron,
                        onClick = { onOpen(WatchPage.SyncHistory) },
                    )
                }
            }
        }
    }
}
