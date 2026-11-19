package dev.nimrag.app.ui.preview

import dev.nimrag.app.model.AppState
import dev.nimrag.app.model.FoundDevice
import dev.nimrag.app.model.LinkState
import dev.nimrag.app.model.LogLine
import dev.nimrag.app.model.Permissions
import dev.nimrag.app.model.ScanState
import dev.nimrag.app.model.Settings
import dev.nimrag.app.model.StoredFile
import dev.nimrag.app.model.SyncResult
import dev.nimrag.app.model.SyncState
import dev.nimrag.app.model.UploadState
import dev.nimrag.app.model.WatchState

/** Sample [AppState]s for previews and for the demo controller. Times are relative to "now". */
object PreviewData {
    private const val MIN = 60_000L
    private const val HOUR = 60 * MIN

    val allPermissions = Permissions(bluetooth = true, notifications = true, bluetoothEnabled = true)

    val foundDevices = listOf(
        FoundDevice("AA:BB:CC:00:00:01", "Instinct 2", "Instinct 2", rssi = -52),
        FoundDevice("AA:BB:CC:00:00:02", "Forerunner 55", "Forerunner 55", rssi = -71, unsupportedReason = "pairing flow not verified yet"),
        FoundDevice("AA:BB:CC:00:00:03", "vivoactive 4", "vivoactive 4S", rssi = -88),
    )

    fun paired(
        now: Long = System.currentTimeMillis(),
        link: LinkState = LinkState.Ready,
        battery: Int? = 73,
        unsupportedReason: String? = null,
    ) = WatchState.Paired(
        address = "AA:BB:CC:00:00:01",
        name = "Instinct 2",
        model = "Instinct 2 Solar",
        link = link,
        softwareVersion = "19.18",
        batteryPercent = battery,
        batteryAtMs = battery?.let { now - 4 * MIN },
        lastConnectedAtMs = now - 12 * MIN,
        unsupportedReason = unsupportedReason,
    )

    fun storedFiles(now: Long = System.currentTimeMillis()) = listOf(
        StoredFile("a1", 184_320, "FIT 4/4 activity", now - 3 * HOUR, now - 12 * MIN, now - 10 * MIN),
        StoredFile("a2", 22_016, "FIT 32/2 monitoring", now - 2 * HOUR, now - 12 * MIN, null),
        StoredFile("a3", 9_870, "FIT 49/0 sleep", now - 9 * HOUR, now - 12 * MIN, now - 10 * MIN),
        StoredFile("a4", 1_906_688, "FIT 44/4 metrics", null, now - 26 * HOUR, now - 25 * HOUR),
        StoredFile("a5", 512, "FIT 128/4", now - 30 * HOUR, now - 26 * HOUR, now - 25 * HOUR),
    )

    fun log(now: Long = System.currentTimeMillis()) = listOf(
        LogLine(now - 1 * MIN, LogLine.Level.Info, "File 2 of 4: 22016 bytes stored"),
        LogLine(now - 2 * MIN, LogLine.Level.Info, "Listing: 4 files"),
        LogLine(now - 3 * MIN, LogLine.Level.Warning, "Battery report late, asking again"),
        LogLine(now - 12 * MIN, LogLine.Level.Info, "Handshake done, software 19.18"),
        LogLine(now - 13 * MIN, LogLine.Level.Error, "Upload failed: HTTP 502"),
        LogLine(now - 14 * MIN, LogLine.Level.Info, "Connecting"),
    )

    fun uploads(now: Long = System.currentTimeMillis()) = UploadState(
        pending = 1,
        uploadedTotal = 42,
        storedTotal = 43,
        lastUploadAtMs = now - 10 * MIN,
        recent = storedFiles(now),
    )

    val serverSettings = Settings(
        serverUrl = "https://nimrag.example.ts.net",
        hasToken = true,
        allowMobileData = false,
        autoSyncMinutes = 60,
        capabilitiesHex = "0f3a",
    )

    fun notPaired(scanning: Boolean = false, found: List<FoundDevice> = emptyList()) = AppState(
        permissions = allPermissions,
        scan = ScanState(scanning, found),
    )

    fun permissionsMissing() = AppState(permissions = Permissions())

    fun bluetoothOff() = AppState(permissions = Permissions(bluetooth = true, notifications = true, bluetoothEnabled = false))

    fun pairing() = AppState(
        permissions = allPermissions,
        watch = WatchState.Pairing("AA:BB:CC:00:00:01", "Instinct 2"),
    )

    fun pairedReady(now: Long = System.currentTimeMillis()) = AppState(
        permissions = allPermissions,
        watch = paired(now),
        sync = SyncState(lastResult = SyncResult(now - 12 * MIN, downloaded = 4, archived = 4, failed = 0)),
        uploads = uploads(now),
        settings = serverSettings,
        log = log(now),
    )

    fun pairedWaitingLowBattery(now: Long = System.currentTimeMillis()) = pairedReady(now).copy(
        watch = paired(now, LinkState.Waiting, battery = 9),
    )

    fun pairedUnsupported(now: Long = System.currentTimeMillis()) = pairedReady(now).copy(
        watch = paired(now, LinkState.Connecting, unsupportedReason = "pairing flow not verified yet"),
    )

    fun pairedBondLost(now: Long = System.currentTimeMillis()) = pairedReady(now).copy(
        watch = paired(now, LinkState.BondLost),
    )

    fun syncing(now: Long = System.currentTimeMillis()) = pairedReady(now).copy(
        sync = SyncState(
            running = true,
            filesInCycle = 7,
            filesDone = 2,
            currentBytes = 640_000,
            currentSize = 1_906_688,
            lastResult = SyncResult(now - 3 * HOUR, 3, 3, 0),
        ),
    )

    fun troubled(now: Long = System.currentTimeMillis()) = pairedReady(now).copy(
        sync = SyncState(lastResult = SyncResult(now - 40 * MIN, downloaded = 5, archived = 4, failed = 1)),
        uploads = uploads(now).copy(
            pending = 3,
            lastError = "HTTP 401: token rejected",
            waitingForNetwork = true,
        ),
    )

    fun noServer(now: Long = System.currentTimeMillis()) = pairedReady(now).copy(
        settings = Settings(),
        uploads = UploadState(storedTotal = 43, recent = storedFiles(now)),
        log = emptyList(),
    )
}
