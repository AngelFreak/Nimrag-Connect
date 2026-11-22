package dev.nimrag.app.ui.watch

import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import dev.nimrag.app.model.AppState
import dev.nimrag.app.model.GarminConnectMode
import dev.nimrag.app.model.WatchController
import dev.nimrag.app.model.WatchState
import dev.nimrag.app.ui.settings.LicencesScreen
import dev.nimrag.app.ui.components.SettingsPage
import dev.nimrag.app.ui.watch.logic.WatchPage
import androidx.compose.ui.res.stringResource
import dev.nimrag.app.R

/**
 * The Watch tab: [WatchScreen] and the sub-pages its rows open. Which page is on screen is owned
 * by the shell (it survives rotation and answers the system back button); [onPage] moves between
 * pages and a sub-page's back arrow goes to its parent.
 */
@Composable
fun WatchTab(
    state: AppState,
    controller: WatchController,
    page: WatchPage,
    onPage: (WatchPage) -> Unit,
    onSignIn: () -> Unit,
) {
    val actions = rememberWatchActions(controller)
    val account by controller.account.state.collectAsStateWithLifecycle()
    val paired = state.watch as? WatchState.Paired
    val back = { onPage(page.parent ?: WatchPage.Main) }

    // The watch was forgotten while its details were open: nothing left to show there.
    LaunchedEffect(page, paired == null) {
        if (page == WatchPage.WatchDetails && paired == null) onPage(WatchPage.Main)
    }

    when (page) {
        WatchPage.Main -> WatchScreen(state, account, controller, actions, onOpen = onPage, onSignIn = onSignIn)
        WatchPage.AutoSync -> AutoSyncPage(
            minutes = state.settings.autoSyncMinutes,
            alongside = state.settings.garminConnectMode == GarminConnectMode.Alongside,
            onSelect = controller::setAutoSyncMinutes,
            onBack = back,
        )
        WatchPage.GarminConnect -> GarminConnectPage(
            mode = state.settings.garminConnectMode,
            garminConnect = state.garminConnect,
            onSelect = controller::setGarminConnectMode,
            onHelp = { onPage(WatchPage.GarminHelp) },
            onBack = back,
        )
        WatchPage.GarminHelp -> GarminHelpPage(
            mode = state.settings.garminConnectMode,
            garminConnect = state.garminConnect,
            onOpenGarminConnectInfo = actions.openGarminConnectInfo,
            onBack = back,
        )
        WatchPage.WatchDetails -> if (paired != null) WatchDetailsPage(paired, onPairAgain = controller::forget, onBack = back)
        WatchPage.SyncHistory -> SyncHistoryPage(state, controller, onOpenServer = { onPage(WatchPage.Server) }, onBack = back)
        WatchPage.PhoneFeatures -> PhoneFeaturesPage(
            state = state,
            controller = controller,
            actions = actions,
            onOpenApps = { onPage(WatchPage.NotificationApps) },
            onBack = back,
        )
        WatchPage.NotificationApps -> NotificationAppsPage(state.settings.phone.excludedApps, controller, back)
        WatchPage.Server -> ServerPage(state.settings, controller, onSignIn, back)
        WatchPage.Account -> AccountPage(account, controller, onSignIn, back)
        WatchPage.About -> AboutPage(onOpen = onPage, onBack = back)
        WatchPage.Advanced -> AdvancedPage(state.settings.capabilitiesHex, controller::setCapabilitiesHex, back)
        WatchPage.Licences -> SettingsPage(title = stringResource(R.string.about_open_source), onBack = back) { LicencesScreen() }
        WatchPage.Help -> HelpPage(onOpen = onPage, onBack = back)
    }
}
