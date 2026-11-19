package dev.nimrag.app.ui.preview

import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.ui.tooling.preview.PreviewLightDark
import androidx.compose.ui.unit.dp
import dev.nimrag.app.model.AppState
import dev.nimrag.app.ui.NimragShell
import dev.nimrag.app.ui.settings.LicencesScreen
import dev.nimrag.app.ui.settings.SettingsScreen
import dev.nimrag.app.ui.sync.SyncScreen
import dev.nimrag.app.ui.theme.NimragTheme
import dev.nimrag.app.ui.watch.WatchScreen

/* Previews for every screen in several states, all on [FakeController] data. */

private val NoPadding = PaddingValues(0.dp)

@Composable
private fun Host(content: @Composable () -> Unit) {
    NimragTheme(dynamicColor = false) {
        Surface(color = androidx.compose.material3.MaterialTheme.colorScheme.background, content = content)
    }
}

// ---- Watch

@Composable
private fun WatchPreview(state: AppState) = Host { WatchScreen(state, FakeController(state), NoPadding) }

@PreviewLightDark @Composable private fun WatchPermissionsMissing() = WatchPreview(PreviewData.permissionsMissing())

@PreviewLightDark @Composable private fun WatchBluetoothOff() = WatchPreview(PreviewData.bluetoothOff())

@PreviewLightDark @Composable private fun WatchNotPaired() = WatchPreview(PreviewData.notPaired())

@PreviewLightDark @Composable private fun WatchScanning() =
    WatchPreview(PreviewData.notPaired(scanning = true, found = PreviewData.foundDevices))

@PreviewLightDark @Composable private fun WatchPairing() = WatchPreview(PreviewData.pairing())

@PreviewLightDark @Composable private fun WatchPairedReady() = WatchPreview(PreviewData.pairedReady())

@PreviewLightDark @Composable private fun WatchPairedWaitingLowBattery() = WatchPreview(PreviewData.pairedWaitingLowBattery())

@PreviewLightDark @Composable private fun WatchPairedUnsupported() = WatchPreview(PreviewData.pairedUnsupported())

@PreviewLightDark @Composable private fun WatchBondLost() = WatchPreview(PreviewData.pairedBondLost())

// ---- Sync

@Composable
private fun SyncPreview(state: AppState) = Host { SyncScreen(state, FakeController(state), NoPadding, onOpenSettings = {}) }

@PreviewLightDark @Composable private fun SyncIdle() = SyncPreview(PreviewData.pairedReady())

@PreviewLightDark @Composable private fun SyncRunning() = SyncPreview(PreviewData.syncing())

@PreviewLightDark @Composable private fun SyncTroubled() = SyncPreview(PreviewData.troubled())

@PreviewLightDark @Composable private fun SyncNoServer() = SyncPreview(PreviewData.noServer())

// ---- Settings

@Composable
private fun SettingsPreview(state: AppState) = Host { SettingsScreen(state, FakeController(state), NoPadding, onOpenLicences = {}) }

@PreviewLightDark @Composable private fun SettingsConfigured() = SettingsPreview(PreviewData.pairedReady())

@PreviewLightDark @Composable private fun SettingsEmpty() = SettingsPreview(PreviewData.noServer())

@PreviewLightDark @Composable private fun Licences() = Host { LicencesScreen(NoPadding) }

// ---- Whole shell

@PreviewLightDark @Composable private fun ShellWatch() = NimragTheme(dynamicColor = false) {
    NimragShell(PreviewData.pairedReady(), FakeController(PreviewData.pairedReady()))
}

@PreviewLightDark @Composable private fun ShellSync() = NimragTheme(dynamicColor = false) {
    NimragShell(PreviewData.troubled(), FakeController(PreviewData.troubled()), initialTab = 1)
}

@PreviewLightDark @Composable private fun ShellSettings() = NimragTheme(dynamicColor = false) {
    NimragShell(PreviewData.pairedReady(), FakeController(PreviewData.pairedReady()), demo = true, initialTab = 2)
}
