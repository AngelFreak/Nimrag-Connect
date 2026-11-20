package dev.nimrag.app.ui.preview

import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import dev.nimrag.app.model.AppState
import dev.nimrag.app.ui.NimragShell
import dev.nimrag.app.ui.settings.LicencesScreen
import dev.nimrag.app.engine.auth.logic.AuthFailure
import dev.nimrag.app.engine.auth.logic.AuthPhase
import dev.nimrag.app.model.AccountState
import dev.nimrag.app.ui.shell.CheckingScreen
import dev.nimrag.app.ui.shell.SignInScreen
import dev.nimrag.app.ui.shell.UnlockScreen
import dev.nimrag.app.ui.shell.ServerUnreachableScreen
import dev.nimrag.app.ui.settings.SettingsScreen
import dev.nimrag.app.ui.sync.SyncScreen
import dev.nimrag.app.ui.theme.NimragTheme
import dev.nimrag.app.ui.watch.WatchScreen

/* Previews for every screen in several states, all on [FakeController] data. */

private val NoPadding = PaddingValues(0.dp)

@Composable
private fun Host(content: @Composable () -> Unit) {
    NimragTheme {
        Surface(color = androidx.compose.material3.MaterialTheme.colorScheme.background, content = content)
    }
}

// ---- Watch

@Composable
private fun WatchPreview(state: AppState) = Host { WatchScreen(state, FakeController(state), NoPadding) }

@Preview @Composable private fun WatchPermissionsMissing() = WatchPreview(PreviewData.permissionsMissing())

@Preview @Composable private fun WatchBluetoothOff() = WatchPreview(PreviewData.bluetoothOff())

@Preview @Composable private fun WatchNotPaired() = WatchPreview(PreviewData.notPaired())

@Preview @Composable private fun WatchScanning() =
    WatchPreview(PreviewData.notPaired(scanning = true, found = PreviewData.foundDevices))

@Preview @Composable private fun WatchPairing() = WatchPreview(PreviewData.pairing())

@Preview @Composable private fun WatchPairedReady() = WatchPreview(PreviewData.pairedReady())

@Preview @Composable private fun WatchPairedWaitingLowBattery() = WatchPreview(PreviewData.pairedWaitingLowBattery())

@Preview @Composable private fun WatchPairedUnsupported() = WatchPreview(PreviewData.pairedUnsupported())

@Preview @Composable private fun WatchBondLost() = WatchPreview(PreviewData.pairedBondLost())

// ---- Sync

@Composable
private fun SyncPreview(state: AppState) = Host { SyncScreen(state, FakeController(state), NoPadding, onOpenSettings = {}) }

@Preview @Composable private fun SyncIdle() = SyncPreview(PreviewData.pairedReady())

@Preview @Composable private fun SyncRunning() = SyncPreview(PreviewData.syncing())

@Preview @Composable private fun SyncTroubled() = SyncPreview(PreviewData.troubled())

@Preview @Composable private fun SyncNoServer() = SyncPreview(PreviewData.noServer())

// ---- Settings

@Composable
private fun SettingsPreview(state: AppState) = Host { SettingsScreen(state, FakeController(state), NoPadding, onOpenLicences = {}) }

@Preview @Composable private fun SettingsConfigured() = SettingsPreview(PreviewData.pairedReady())

@Preview @Composable private fun SettingsEmpty() = SettingsPreview(PreviewData.noServer())

@Preview @Composable private fun Licences() = Host { LicencesScreen(NoPadding) }

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

@Preview @Composable private fun ShellSync() = NimragTheme {
    NimragShell(PreviewData.troubled(), FakeController(PreviewData.troubled()), startInWatch = true, initialSection = 1)
}

@Preview @Composable private fun ShellSettings() = NimragTheme {
    NimragShell(PreviewData.pairedReady(), FakeController(PreviewData.pairedReady()), demo = true, startInWatch = true, initialSection = 2)
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
