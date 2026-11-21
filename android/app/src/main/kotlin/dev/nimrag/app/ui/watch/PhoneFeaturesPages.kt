package dev.nimrag.app.ui.watch

import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Info
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import dev.nimrag.app.R
import dev.nimrag.app.model.AppChoice
import dev.nimrag.app.model.AppState
import dev.nimrag.app.model.FindWatchUi
import dev.nimrag.app.model.LinkState
import dev.nimrag.app.model.WatchController
import dev.nimrag.app.model.WatchState
import dev.nimrag.app.model.WeatherLast
import dev.nimrag.app.model.WeatherStatus
import dev.nimrag.app.ui.components.BellIcon
import dev.nimrag.app.ui.components.CloudIcon
import dev.nimrag.app.ui.components.Footnote
import dev.nimrag.app.ui.components.MessageCard
import dev.nimrag.app.ui.components.MusicIcon
import dev.nimrag.app.ui.components.NimragButton
import dev.nimrag.app.ui.components.NimragOutlinedButton
import dev.nimrag.app.ui.components.PageColumn
import dev.nimrag.app.ui.components.PhoneIcon
import dev.nimrag.app.ui.components.relativeTime
import dev.nimrag.app.ui.components.RowTrailing
import dev.nimrag.app.ui.components.SettingsGroup
import dev.nimrag.app.ui.components.SettingsPage
import dev.nimrag.app.ui.components.SettingsRow
import dev.nimrag.app.ui.components.ShieldIcon
import dev.nimrag.app.ui.components.WatchIcon

/** "All off" or "2 of 4 on": the Watch page's one-line summary of the phone features. */
@Composable
internal fun phoneFeaturesSummary(state: AppState): String {
    val phone = state.settings.phone
    val on = listOf(phone.notifications, phone.music, phone.findPhone, phone.weather).count { it }
    return if (on == 0) stringResource(R.string.phone_features_value_off) else stringResource(R.string.phone_features_value_count, on)
}

/**
 * What the phone does for the watch besides syncing: notifications on the watch, music control,
 * find my phone (three switches, all off until the owner turns them on) and "Find my watch".
 * Each switch has its plain-English explanation underneath. Notifications and music need the
 * system's Notification access, which only system settings can give: the card here opens it.
 */
@Composable
internal fun PhoneFeaturesPage(
    state: AppState,
    controller: WatchController,
    actions: WatchActions,
    onOpenApps: () -> Unit,
    onBack: () -> Unit,
) {
    val phone = state.settings.phone
    val access = state.phoneAccess
    val waiting = !access.notificationAccess
    val needsAccess = (phone.notifications || phone.music) && waiting
    val paired = state.watch as? WatchState.Paired

    SettingsPage(title = stringResource(R.string.phone_features_title), onBack = onBack) {
        PageColumn {
            Footnote(stringResource(R.string.phone_features_intro), Modifier.fillMaxWidth())

            if (needsAccess) {
                MessageCard(
                    icon = Icons.Filled.Info,
                    title = stringResource(R.string.phone_access_title),
                    containerColor = MaterialTheme.colorScheme.secondaryContainer,
                    contentColor = MaterialTheme.colorScheme.onSecondaryContainer,
                    actions = {
                        NimragButton(onClick = actions.openNotificationAccess) { Text(stringResource(R.string.phone_access_open)) }
                        NimragOutlinedButton(onClick = actions.openOwnAppInfo) { Text(stringResource(R.string.phone_access_app_info)) }
                    },
                ) {
                    Text(stringResource(R.string.phone_access_body), style = MaterialTheme.typography.bodyMedium)
                    Text(stringResource(R.string.phone_access_restricted), style = MaterialTheme.typography.bodySmall)
                }
            }

            SettingsGroup(heading = stringResource(R.string.phone_group_features), footer = stringResource(R.string.phone_features_footer)) {
                item {
                    SettingsRow(
                        title = stringResource(R.string.phone_notifications_title),
                        value = stringResource(
                            when {
                                !phone.notifications -> R.string.phone_notifications_off
                                waiting -> R.string.phone_notifications_waiting
                                else -> R.string.phone_notifications_on
                            },
                        ),
                        icon = BellIcon,
                        trailing = RowTrailing.Toggle(phone.notifications) { on ->
                            controller.setPhoneNotifications(on)
                            if (on && waiting) actions.openNotificationAccess()
                        },
                    )
                }
                item {
                    SettingsRow(
                        title = stringResource(R.string.phone_music_title),
                        value = stringResource(
                            when {
                                !phone.music -> R.string.phone_music_off
                                waiting -> R.string.phone_music_waiting
                                else -> R.string.phone_music_on
                            },
                        ),
                        icon = MusicIcon,
                        trailing = RowTrailing.Toggle(phone.music) { on ->
                            controller.setPhoneMusic(on)
                            if (on && waiting) actions.openNotificationAccess()
                        },
                    )
                }
                item {
                    SettingsRow(
                        title = stringResource(R.string.phone_find_title),
                        value = stringResource(if (phone.findPhone) R.string.phone_find_on else R.string.phone_find_off),
                        icon = PhoneIcon,
                        trailing = RowTrailing.Toggle(phone.findPhone, controller::setPhoneFindPhone),
                    )
                }
                item {
                    SettingsRow(
                        title = stringResource(R.string.phone_weather_title),
                        value = weatherValue(phone.weather, state.phone.weather),
                        icon = CloudIcon,
                        trailing = RowTrailing.Toggle(phone.weather, controller::setPhoneWeather),
                    )
                }
            }

            if (phone.notifications) {
                SettingsGroup(heading = stringResource(R.string.phone_group_apps), footer = stringResource(R.string.phone_apps_footer)) {
                    item {
                        val count = phone.excludedApps.size
                        SettingsRow(
                            title = stringResource(R.string.phone_apps_title),
                            value = if (count == 0) {
                                stringResource(R.string.phone_apps_none)
                            } else {
                                pluralStringResource(R.plurals.phone_apps_count, count, count)
                            },
                            icon = BellIcon,
                            trailing = RowTrailing.Chevron,
                            onClick = onOpenApps,
                        )
                    }
                }
            }

            SettingsGroup(heading = stringResource(R.string.phone_access_manage)) {
                item {
                    SettingsRow(
                        title = stringResource(R.string.phone_access_manage),
                        value = stringResource(if (access.notificationAccess) R.string.phone_access_manage_on else R.string.phone_access_title),
                        icon = ShieldIcon,
                        trailing = RowTrailing.Chevron,
                        onClick = actions.openNotificationAccess,
                    )
                }
            }
            if (phone.findPhone && !access.fullScreenAlerts) Footnote(stringResource(R.string.phone_fullscreen_note))

            SettingsGroup(heading = stringResource(R.string.phone_group_find_watch), footer = stringResource(R.string.phone_find_watch_footer)) {
                item {
                    val connected = paired?.link == LinkState.Ready
                    val find = state.phone.findWatch
                    val ringing = find == FindWatchUi.Ringing
                    SettingsRow(
                        title = stringResource(R.string.phone_find_watch_title),
                        value = stringResource(
                            when {
                                find == FindWatchUi.Asking -> R.string.phone_find_watch_asking
                                ringing -> R.string.phone_find_watch_ringing
                                find == FindWatchUi.NotSupported -> R.string.phone_find_watch_unsupported
                                find == FindWatchUi.Failed -> R.string.phone_find_watch_failed
                                !connected -> R.string.phone_find_watch_not_connected
                                else -> R.string.phone_find_watch_idle
                            },
                        ),
                        icon = WatchIcon,
                        onClick = if ((connected || ringing) && find != FindWatchUi.Asking && find != FindWatchUi.NotSupported) {
                            { controller.findMyWatch(!ringing) }
                        } else {
                            null
                        },
                    )
                }
            }

            Footnote(stringResource(R.string.phone_notifications_explain), Modifier.fillMaxWidth())
            Footnote(stringResource(R.string.phone_music_explain), Modifier.fillMaxWidth())
            Footnote(stringResource(R.string.phone_find_explain), Modifier.fillMaxWidth())
            Footnote(stringResource(R.string.phone_weather_explain), Modifier.fillMaxWidth())
        }
    }
}

/** The Weather row's second line: off, or when the watch last took a forecast and what went wrong last. */
@Composable
private fun weatherValue(on: Boolean, status: WeatherStatus): String {
    if (!on) return stringResource(R.string.phone_weather_off)
    val sentAt = status.sentAtMs
    val base = when {
        sentAt == null -> stringResource(R.string.phone_weather_idle)
        status.last == WeatherLast.Reused -> stringResource(R.string.phone_weather_sent_earlier, relativeTime(sentAt))
        else -> stringResource(R.string.phone_weather_sent, relativeTime(sentAt))
    }
    val issue = when (status.last) {
        WeatherLast.NoForecast -> R.string.phone_weather_issue_no_forecast
        WeatherLast.NoPosition -> R.string.phone_weather_issue_no_position
        WeatherLast.NotTaken -> R.string.phone_weather_issue_not_taken
        else -> null
    }
    return if (issue == null) base else "$base · ${stringResource(issue)}"
}

/** One switch per app: on means the app's notifications stay on the phone. */
@Composable
internal fun NotificationAppsPage(excluded: Set<String>, controller: WatchController, onBack: () -> Unit) {
    var apps by remember { mutableStateOf<List<AppChoice>?>(null) }
    LaunchedEffect(controller) { apps = controller.launchableApps() }

    SettingsPage(title = stringResource(R.string.phone_apps_page_title), onBack = onBack) {
        PageColumn {
            Footnote(stringResource(R.string.phone_apps_page_hint), Modifier.fillMaxWidth())
            val installed = apps
            if (installed == null) {
                Footnote(stringResource(R.string.phone_apps_loading), Modifier.fillMaxWidth())
            } else {
                val known = installed.mapTo(HashSet()) { it.packageName }
                // Apps that were left out and are gone from the phone stay in the list so they can be taken off.
                val gone = excluded.filter { it !in known }.sorted().map { AppChoice(it, it) }
                val all = gone + installed
                if (all.isEmpty()) {
                    Footnote(stringResource(R.string.phone_apps_empty), Modifier.fillMaxWidth())
                } else {
                    SettingsGroup {
                        for (app in all) {
                            item {
                                val left = app.packageName in excluded
                                SettingsRow(
                                    title = app.label,
                                    value = when {
                                        app.packageName !in known -> stringResource(R.string.phone_apps_no_longer_installed)
                                        left -> stringResource(R.string.phone_apps_left_out)
                                        else -> stringResource(R.string.phone_apps_shown)
                                    },
                                    trailing = RowTrailing.Toggle(left) { controller.setAppExcluded(app.packageName, it) },
                                )
                            }
                        }
                    }
                }
            }
        }
    }
}
