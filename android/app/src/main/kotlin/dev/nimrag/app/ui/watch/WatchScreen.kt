package dev.nimrag.app.ui.watch

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
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
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import dev.nimrag.app.R
import dev.nimrag.app.engine.auth.logic.AuthPhase
import dev.nimrag.app.model.AccountState
import dev.nimrag.app.model.AppState
import dev.nimrag.app.model.WatchController
import dev.nimrag.app.model.WatchState
import dev.nimrag.app.ui.components.BellIcon
import dev.nimrag.app.ui.components.ConfirmDialog
import dev.nimrag.app.ui.components.InfoIcon
import dev.nimrag.app.ui.components.LinkIcon
import dev.nimrag.app.ui.components.ListIcon
import dev.nimrag.app.ui.components.PageColumn
import dev.nimrag.app.ui.components.PhoneIcon
import dev.nimrag.app.ui.components.RowTone
import dev.nimrag.app.ui.components.RowTrailing
import dev.nimrag.app.ui.components.ServerIcon
import dev.nimrag.app.ui.components.SettingsGroup
import dev.nimrag.app.ui.components.SettingsPage
import dev.nimrag.app.ui.components.SettingsRow
import dev.nimrag.app.ui.components.ShieldIcon
import dev.nimrag.app.ui.components.SignalIcon
import dev.nimrag.app.ui.components.StatusBanner
import dev.nimrag.app.ui.components.SyncIcon
import dev.nimrag.app.ui.components.WatchIcon
import dev.nimrag.app.ui.watch.logic.ServerHost
import dev.nimrag.app.ui.watch.logic.WatchPage
import dev.nimrag.app.ui.watch.logic.WatchProblem
import dev.nimrag.app.ui.watch.logic.WatchProblems

/**
 * The Watch page: a banner when something blocks sync, the watch (a status card, or the setup
 * checklist when none is paired), then the settings in three groups of one-line rows. Every
 * explanation lives one tap further, on the page a row opens.
 */
@Composable
internal fun WatchScreen(
    state: AppState,
    account: AccountState,
    controller: WatchController,
    actions: WatchActions,
    onOpen: (WatchPage) -> Unit,
    onSignIn: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val watch = state.watch
    val paired = watch as? WatchState.Paired
    var confirmForget by rememberSaveable { mutableStateOf(false) }
    val settings = state.settings

    SettingsPage(
        title = stringResource(if (paired != null) R.string.watch_title else R.string.setup_title),
        subtitle = if (paired == null) stringResource(R.string.setup_subtitle) else null,
        modifier = modifier,
    ) {
        PageColumn {
            WatchProblems.top(state)?.let { ProblemBanner(it, state, actions, controller, onOpen) }

            when (watch) {
                WatchState.NotPaired -> SetupSection(state, controller, actions)
                is WatchState.Pairing -> PairingCard(watch.name)
                is WatchState.Paired -> StatusCard(
                    watch = watch,
                    sync = state.sync,
                    onSync = controller::syncNow,
                    onPairAgain = controller::forget,
                )
            }

            SettingsGroup(heading = stringResource(R.string.group_sync)) {
                item {
                    SettingsRow(
                        title = stringResource(R.string.auto_sync_title),
                        value = intervalLabel(settings.autoSyncMinutes),
                        icon = SyncIcon,
                        trailing = RowTrailing.Chevron,
                        onClick = { onOpen(WatchPage.AutoSync) },
                    )
                }
                item {
                    SettingsRow(
                        title = stringResource(R.string.server_mobile_data),
                        value = stringResource(if (settings.allowMobileData) R.string.mobile_data_on else R.string.mobile_data_off),
                        icon = SignalIcon,
                        trailing = RowTrailing.Toggle(settings.allowMobileData, controller::setAllowMobileData),
                    )
                }
                item {
                    SettingsRow(
                        title = stringResource(R.string.settings_garmin_connect),
                        value = stringResource(R.string.garmin_value),
                        icon = LinkIcon,
                        trailing = RowTrailing.Chevron,
                        onClick = { onOpen(WatchPage.GarminConnect) },
                    )
                }
            }

            SettingsGroup(heading = stringResource(R.string.group_data_retention)) {
                item {
                    SettingsRow(
                        title = stringResource(R.string.history_retention_title),
                        value = historyRetentionLabel(settings.historyRetention),
                        icon = ListIcon,
                        trailing = RowTrailing.Chevron,
                        onClick = { onOpen(WatchPage.DataRetention) },
                    )
                }
            }

            SettingsGroup(heading = stringResource(R.string.group_watch)) {
                if (paired != null) {
                    item {
                        SettingsRow(
                            title = stringResource(R.string.watch_details_title),
                            value = stringResource(R.string.watch_details_value, paired.softwareVersion ?: stringResource(R.string.unknown)),
                            icon = WatchIcon,
                            trailing = RowTrailing.Chevron,
                            onClick = { onOpen(WatchPage.WatchDetails) },
                        )
                    }
                }
                item {
                    SettingsRow(
                        title = stringResource(R.string.phone_features_title),
                        value = phoneFeaturesSummary(state),
                        icon = PhoneIcon,
                        trailing = RowTrailing.Chevron,
                        onClick = { onOpen(WatchPage.PhoneFeatures) },
                    )
                }
                item {
                    val files = state.uploads.storedTotal
                    val edits = state.uploads.pendingEdits
                    val filesValue = when {
                        files == 0 -> stringResource(R.string.history_value_none)
                        state.uploads.pending > 0 -> stringResource(
                            R.string.history_value_waiting,
                            pluralStringResource(R.plurals.files, files, files),
                            pluralStringResource(R.plurals.files_waiting, state.uploads.pending, state.uploads.pending),
                        )
                        else -> stringResource(R.string.history_value_uploaded, pluralStringResource(R.plurals.files, files, files))
                    }
                    // Changes made offline on the phone's copy wait for the server like files do.
                    val editsValue = pluralStringResource(R.plurals.edits_waiting, edits, edits)
                    SettingsRow(
                        title = stringResource(R.string.history_title),
                        value = when {
                            edits == 0 -> filesValue
                            files == 0 -> editsValue
                            else -> stringResource(R.string.history_value_edits_waiting, filesValue, editsValue)
                        },
                        icon = ListIcon,
                        trailing = RowTrailing.Chevron,
                        onClick = { onOpen(WatchPage.SyncHistory) },
                    )
                }
            }

            val signedIn = account.phase == AuthPhase.SignedIn || account.phase == AuthPhase.Locked
            SettingsGroup(heading = stringResource(R.string.group_phone)) {
                item {
                    SettingsRow(
                        title = stringResource(R.string.settings_server),
                        value = settings.serverUrl.takeIf { it.isNotBlank() }?.let(ServerHost::of) ?: stringResource(R.string.server_not_set_up),
                        icon = ServerIcon,
                        trailing = RowTrailing.Chevron,
                        onClick = { onOpen(WatchPage.Server) },
                    )
                }
                item {
                    if (signedIn) {
                        SettingsRow(
                            title = account.username?.let { stringResource(R.string.account_signed_in_as, it) } ?: stringResource(R.string.account_signed_in),
                            value = when {
                                account.phase == AuthPhase.Locked -> stringResource(R.string.account_value_locked)
                                account.biometricReady -> stringResource(R.string.account_value_fingerprint)
                                else -> null
                            },
                            icon = ShieldIcon,
                            trailing = RowTrailing.Chevron,
                            onClick = { onOpen(WatchPage.Account) },
                        )
                    } else {
                        SettingsRow(
                            title = stringResource(R.string.account_sign_in),
                            value = stringResource(R.string.account_value_signed_out),
                            icon = ShieldIcon,
                            trailing = RowTrailing.Chevron,
                            onClick = onSignIn,
                        )
                    }
                }
                if (!state.permissions.notifications) {
                    item {
                        SettingsRow(
                            title = stringResource(R.string.notifications_title),
                            value = stringResource(R.string.notifications_value_off),
                            icon = BellIcon,
                            trailing = RowTrailing.Chevron,
                            onClick = actions.allowNotifications,
                        )
                    }
                }
                item {
                    SettingsRow(
                        title = stringResource(R.string.settings_about_app),
                        value = stringResource(R.string.about_value, appVersionName()),
                        icon = InfoIcon,
                        trailing = RowTrailing.Chevron,
                        onClick = { onOpen(WatchPage.About) },
                    )
                }
            }

            if (paired != null) {
                SettingsGroup {
                    item {
                        SettingsRow(
                            title = stringResource(R.string.forget_watch),
                            tone = RowTone.Danger,
                            onClick = { confirmForget = true },
                        )
                    }
                }
            }

            Box(Modifier.fillMaxWidth(), contentAlignment = Alignment.Center) {
                TextButton(onClick = { onOpen(WatchPage.Help) }, modifier = Modifier.heightIn(min = 48.dp)) {
                    Text(stringResource(R.string.help_link), style = MaterialTheme.typography.labelLarge)
                }
            }
            Spacer(Modifier.height(8.dp))
        }
    }

    if (confirmForget && paired != null) {
        ConfirmDialog(
            title = stringResource(R.string.forget_title, paired.name),
            body = stringResource(R.string.forget_body),
            confirmLabel = stringResource(R.string.forget_confirm),
            destructive = true,
            onConfirm = {
                confirmForget = false
                controller.forget()
            },
            onDismiss = { confirmForget = false },
        )
    }
}

/** The one problem, as a line and (when there is a fix) one button. */
@Composable
private fun ProblemBanner(
    problem: WatchProblem,
    state: AppState,
    actions: WatchActions,
    controller: WatchController,
    onOpen: (WatchPage) -> Unit,
) {
    when (problem) {
        WatchProblem.BluetoothPermission -> StatusBanner(
            message = stringResource(R.string.banner_bluetooth_permission),
            actionLabel = stringResource(R.string.banner_allow),
            onAction = actions.allowBluetooth,
        )
        WatchProblem.BluetoothOff -> StatusBanner(
            message = stringResource(R.string.banner_bluetooth_off),
            actionLabel = stringResource(R.string.banner_turn_on),
            onAction = actions.turnOnBluetooth,
        )
        WatchProblem.UnsupportedWatch -> StatusBanner(
            message = stringResource(R.string.cannot_sync_yet, (state.watch as? WatchState.Paired)?.unsupportedReason.orEmpty()),
        )
        WatchProblem.GarminConnect -> StatusBanner(
            message = stringResource(R.string.banner_garmin_connect),
            actionLabel = stringResource(R.string.banner_how_to_fix),
            onAction = { onOpen(WatchPage.GarminHelp) },
        )
        WatchProblem.UploadFailing -> StatusBanner(
            message = stringResource(R.string.banner_upload_failing),
            actionLabel = stringResource(R.string.banner_retry),
            onAction = controller::uploadNow,
        )
    }
}
