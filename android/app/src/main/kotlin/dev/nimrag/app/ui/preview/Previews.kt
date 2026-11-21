package dev.nimrag.app.ui.preview

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import dev.nimrag.app.model.AppState
import dev.nimrag.app.ui.NimragShell
import dev.nimrag.app.engine.auth.logic.AuthFailure
import dev.nimrag.app.engine.auth.logic.AuthPhase
import dev.nimrag.app.model.AccountState
import dev.nimrag.app.ui.shell.CheckingScreen
import dev.nimrag.app.ui.shell.SignInScreen
import dev.nimrag.app.ui.shell.UnlockScreen
import dev.nimrag.app.ui.shell.ServerUnreachableScreen
import dev.nimrag.app.ui.theme.NimragTheme
import dev.nimrag.app.ui.components.RowTone
import dev.nimrag.app.ui.components.RowTrailing
import dev.nimrag.app.ui.components.SettingsGroup
import dev.nimrag.app.ui.components.SettingsRow
import dev.nimrag.app.ui.components.SignalIcon
import dev.nimrag.app.ui.components.StatusBanner
import dev.nimrag.app.ui.components.SyncIcon
import dev.nimrag.app.ui.watch.WatchTab
import dev.nimrag.app.ui.watch.logic.WatchPage

/* Previews for every screen in several states, all on [FakeController] data. */

@Composable
private fun Host(content: @Composable () -> Unit) {
    NimragTheme {
        Surface(color = androidx.compose.material3.MaterialTheme.colorScheme.background, content = content)
    }
}

// ---- Watch page

@Composable
private fun WatchPreview(state: AppState, page: WatchPage = WatchPage.Main) =
    Host { WatchTab(state, FakeController(state), page, onPage = {}, onSignIn = {}) }

@Preview(showBackground = true) @Composable private fun WatchPairedReady() = WatchPreview(PreviewData.pairedReady())

@Preview(showBackground = true, fontScale = 1.3f) @Composable private fun WatchPairedReadyLargeText() = WatchPreview(PreviewData.pairedReady())

@Preview(showBackground = true, widthDp = 800, heightDp = 360) @Composable private fun WatchPairedReadyLandscape() = WatchPreview(PreviewData.pairedReady())

@Preview(showBackground = true) @Composable private fun WatchSyncing() = WatchPreview(PreviewData.syncing())

@Preview(showBackground = true) @Composable private fun WatchBannerBluetoothOff() = WatchPreview(PreviewData.pairedBluetoothOff())

@Preview(showBackground = true, fontScale = 1.3f) @Composable private fun WatchBannerBluetoothOffLargeText() = WatchPreview(PreviewData.pairedBluetoothOff())

@Preview(showBackground = true) @Composable private fun WatchBannerGarminConnect() = WatchPreview(PreviewData.pairedGarminHolding())

@Preview(showBackground = true) @Composable private fun WatchBannerUploadFailing() = WatchPreview(PreviewData.uploadFailing())

@Preview(showBackground = true) @Composable private fun WatchBannerUnsupported() = WatchPreview(PreviewData.pairedUnsupported())

@Preview(showBackground = true) @Composable private fun WatchWaitingLowBattery() = WatchPreview(PreviewData.pairedWaitingLowBattery())

@Preview(showBackground = true) @Composable private fun WatchBondLost() = WatchPreview(PreviewData.pairedBondLost())

@Preview(showBackground = true) @Composable private fun WatchNotificationsOff() = WatchPreview(PreviewData.notificationsOff())

// ---- Adding a watch

@Preview(showBackground = true) @Composable private fun SetupAllowBluetooth() = WatchPreview(PreviewData.permissionsMissing())

@Preview(showBackground = true) @Composable private fun SetupTurnOnBluetooth() = WatchPreview(PreviewData.bluetoothOff())

@Preview(showBackground = true) @Composable private fun SetupReadyToScan() = WatchPreview(PreviewData.notPaired())

@Preview(showBackground = true, fontScale = 1.3f) @Composable private fun SetupReadyToScanLargeText() = WatchPreview(PreviewData.notPaired())

@Preview(showBackground = true) @Composable private fun SetupScanning() =
    WatchPreview(PreviewData.notPaired(scanning = true, found = PreviewData.foundDevices))

@Preview(showBackground = true) @Composable private fun SetupPairing() = WatchPreview(PreviewData.pairing())

@Preview(showBackground = true) @Composable private fun SetupPairingFailed() =
    WatchPreview(PreviewData.notPaired().copy(pairingError = "The watch did not answer the pairing request."))

// ---- Sub-pages

@Preview(showBackground = true) @Composable private fun PageAutoSync() = WatchPreview(PreviewData.pairedReady(), WatchPage.AutoSync)

@Preview(showBackground = true) @Composable private fun PageGarminConnect() = WatchPreview(PreviewData.alongside(), WatchPage.GarminConnect)

@Preview(showBackground = true) @Composable private fun PageGarminHelp() = WatchPreview(PreviewData.pairedGarminHolding(), WatchPage.GarminHelp)

@Preview(showBackground = true) @Composable private fun PageWatchDetails() = WatchPreview(PreviewData.pairedReady(), WatchPage.WatchDetails)

@Preview(showBackground = true) @Composable private fun PageSyncHistory() = WatchPreview(PreviewData.pairedReady(), WatchPage.SyncHistory)

@Preview(showBackground = true) @Composable private fun PageSyncHistoryTroubled() = WatchPreview(PreviewData.troubled(), WatchPage.SyncHistory)

@Preview(showBackground = true) @Composable private fun PageSyncHistoryEmpty() = WatchPreview(PreviewData.noServer(), WatchPage.SyncHistory)

@Preview(showBackground = true) @Composable private fun PagePhoneFeaturesOff() = WatchPreview(PreviewData.phoneOff(), WatchPage.PhoneFeatures)

@Preview(showBackground = true, fontScale = 1.3f) @Composable private fun PagePhoneFeaturesNeedAccess() = WatchPreview(PreviewData.phoneNeedsAccess(), WatchPage.PhoneFeatures)

@Preview(showBackground = true) @Composable private fun PagePhoneFeaturesOn() = WatchPreview(PreviewData.phoneOn(), WatchPage.PhoneFeatures)

@Preview(showBackground = true) @Composable private fun PageNotificationApps() = WatchPreview(PreviewData.phoneOn(), WatchPage.NotificationApps)

@Preview(showBackground = true) @Composable private fun PageServer() = WatchPreview(PreviewData.pairedReady(), WatchPage.Server)

@Preview(showBackground = true) @Composable private fun PageServerNotSetUp() = WatchPreview(PreviewData.noServer(), WatchPage.Server)

@Preview(showBackground = true) @Composable private fun PageAccount() = WatchPreview(PreviewData.pairedReady(), WatchPage.Account)

@Preview(showBackground = true) @Composable private fun PageAbout() = WatchPreview(PreviewData.pairedReady(), WatchPage.About)

@Preview(showBackground = true) @Composable private fun PageAdvanced() = WatchPreview(PreviewData.pairedReady(), WatchPage.Advanced)

@Preview(showBackground = true) @Composable private fun PageLicences() = WatchPreview(PreviewData.pairedReady(), WatchPage.Licences)

@Preview(showBackground = true) @Composable private fun PageHelp() = WatchPreview(PreviewData.pairedReady(), WatchPage.Help)

// ---- Shared components

@Preview(showBackground = true) @Composable private fun ComponentsRowsAndBanner() = Host {
    Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
        StatusBanner(message = "Bluetooth is off. Sync waits.", actionLabel = "Turn on", onAction = {})
        SettingsGroup(heading = "Heading", footer = "A footer says one thing in one line.") {
            item { SettingsRow(title = "Chevron row", value = "Current value", icon = SyncIcon, trailing = RowTrailing.Chevron, onClick = {}) }
            item { SettingsRow(title = "Switch row", value = "On", icon = SignalIcon, trailing = RowTrailing.Toggle(true) {}) }
            item { SettingsRow(title = "Radio row", value = "Selected", trailing = RowTrailing.Radio(true), onClick = {}) }
            item { SettingsRow(title = "Danger row", tone = RowTone.Danger, onClick = {}) }
        }
    }
}

// ---- Whole shell

@Preview @Composable private fun ShellToday() = NimragTheme {
    NimragShell(PreviewData.pairedReady(), FakeController(PreviewData.pairedReady()))
}

@Preview @Composable private fun ShellNoServer() = NimragTheme {
    NimragShell(PreviewData.noServer(), FakeController(PreviewData.noServer()))
}

@Preview @Composable private fun ShellWatch() = NimragTheme {
    NimragShell(PreviewData.pairedReady(), FakeController(PreviewData.pairedReady()), startInWatch = true)
}

@Preview @Composable private fun ShellWatchSubPage() = NimragTheme {
    NimragShell(PreviewData.pairedReady(), FakeController(PreviewData.pairedReady()), demo = true, startInWatch = true, initialPage = WatchPage.AutoSync)
}

@Preview @Composable private fun SignIn() = Host {
    SignInScreen(AccountState(phase = AuthPhase.SignedOut), savedServer = "https://nimrag.example.ts.net", onSignIn = { _, _, _, _ -> })
}

@Preview @Composable private fun SignInFailed() = Host {
    SignInScreen(AccountState(phase = AuthPhase.SignedOut, failure = AuthFailure.WrongCredentials), savedServer = "", onSignIn = { _, _, _, _ -> })
}

@Preview @Composable private fun Unlock() = Host {
    UnlockScreen(
        AccountState(phase = AuthPhase.Locked, username = "anna", biometricReady = true),
        onFingerprint = {},
        onPassword = {},
        onOtherAccount = {},
    )
}

@Preview @Composable private fun Checking() = Host { CheckingScreen() }

@Preview @Composable private fun ServerUnreachable() = Host { ServerUnreachableScreen(retrying = false, onRetry = {}) }
