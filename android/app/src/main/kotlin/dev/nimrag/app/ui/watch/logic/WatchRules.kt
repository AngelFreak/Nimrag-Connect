package dev.nimrag.app.ui.watch.logic

import dev.nimrag.app.model.AppState
import dev.nimrag.app.model.GarminConnect
import dev.nimrag.app.model.LinkState
import dev.nimrag.app.model.Permissions
import dev.nimrag.app.model.WatchState
import dev.nimrag.app.ui.shell.logic.Origin
import dev.nimrag.app.ui.shell.logic.ServerOrigin

/*
 * Pure rules for the Watch page (no Android types, so the unit tests run on the JVM): which
 * problem the banner shows, what the status chip says, what the one pairing button does next.
 */

/** Something that blocks or hampers syncing. The page shows only the most important one. */
enum class WatchProblem {
    BluetoothPermission,
    BluetoothOff,
    UnsupportedWatch,
    GarminConnect,
    UploadFailing,
}

object WatchProblems {
    /**
     * The one problem to show, most important first, or null. Bluetooth trouble is only a banner
     * once a watch is paired: while no watch is paired, the setup checklist asks for it as its first step.
     */
    fun top(state: AppState): WatchProblem? {
        val paired = state.watch as? WatchState.Paired
        return when {
            paired != null && !state.permissions.bluetooth -> WatchProblem.BluetoothPermission
            paired != null && !state.permissions.bluetoothEnabled -> WatchProblem.BluetoothOff
            paired?.unsupportedReason != null -> WatchProblem.UnsupportedWatch
            garminConnectInTheWay(state) -> WatchProblem.GarminConnect
            state.settings.serverUrl.isNotBlank() && state.uploads.lastError != null -> WatchProblem.UploadFailing
            else -> null
        }
    }

    /** Garmin Connect holds the watch's session whenever it runs, so Nimrag cannot connect. */
    fun garminConnectInTheWay(state: AppState): Boolean {
        val ready = (state.watch as? WatchState.Paired)?.link == LinkState.Ready
        return state.garminConnect == GarminConnect.HasBluetooth && !ready
    }

    /** The dot on the Watch tab: a banner is showing, or Bluetooth is not usable yet. */
    fun needsAttention(state: AppState): Boolean =
        top(state) != null || !state.permissions.bluetooth || !state.permissions.bluetoothEnabled
}

/** The single word on the status card. */
enum class WatchStatus { Connected, Connecting, NotConnected, Syncing, SyncFailed, PairingLost }

object WatchStatuses {
    fun of(watch: WatchState.Paired, syncRunning: Boolean, lastFailed: Int): WatchStatus = when {
        watch.link == LinkState.BondLost -> WatchStatus.PairingLost
        syncRunning -> WatchStatus.Syncing
        watch.link == LinkState.Connecting -> WatchStatus.Connecting
        watch.link == LinkState.Ready -> if (lastFailed > 0) WatchStatus.SyncFailed else WatchStatus.Connected
        else -> WatchStatus.NotConnected
    }
}

/** What the single button of the setup checklist does now. */
enum class SetupAction { AllowBluetooth, TurnOnBluetooth, Scan, Scanning }

object PairingSetup {
    fun action(permissions: Permissions, scanning: Boolean): SetupAction = when {
        !permissions.bluetooth -> SetupAction.AllowBluetooth
        !permissions.bluetoothEnabled -> SetupAction.TurnOnBluetooth
        scanning -> SetupAction.Scanning
        else -> SetupAction.Scan
    }

    /** 1: Bluetooth, 2: put the watch in pairing mode (then scan), 3: find it and confirm. */
    fun currentStep(permissions: Permissions, scanning: Boolean, foundAny: Boolean): Int = when {
        !permissions.bluetooth || !permissions.bluetoothEnabled -> 1
        scanning || foundAny -> 3
        else -> 2
    }
}

object SyncIntervals {
    val Options = listOf(0, 15, 30, 60, 120, 240)

    /** The choices, keeping a value the engine holds that is not in the list visible instead of hiding it. */
    fun options(current: Int): List<Int> = if (current in Options) Options else (Options + current).sorted()
}

object ServerHost {
    /** The address as people say it: host (and a non-default port), never the scheme or a trailing slash. */
    fun of(raw: String): String {
        val origin: Origin? = ServerOrigin.originOf(raw)
        if (origin != null) {
            val shown = origin.host
            return if (origin.port == Origin.defaultPort(origin.scheme)) shown else "$shown:${origin.port}"
        }
        return raw.trim().substringAfter("://").trimEnd('/')
    }
}
