package dev.nimrag.app.ui.watch.logic

import dev.nimrag.app.model.AppState
import dev.nimrag.app.model.GarminConnect
import dev.nimrag.app.model.LinkState
import dev.nimrag.app.model.Permissions
import dev.nimrag.app.model.Settings
import dev.nimrag.app.model.UploadState
import dev.nimrag.app.model.WatchState
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

class WatchRulesTest {
    private val allowed = Permissions(bluetooth = true, notifications = true, bluetoothEnabled = true)

    private fun paired(link: LinkState = LinkState.Ready, unsupported: String? = null) =
        WatchState.Paired("AA", "Instinct 2", "Instinct 2", link, unsupportedReason = unsupported)

    private fun state(
        watch: WatchState = paired(),
        permissions: Permissions = allowed,
        garmin: GarminConnect = GarminConnect.Absent,
        server: String = "https://nimrag.example.ts.net",
        uploadError: String? = null,
    ) = AppState(
        permissions = permissions,
        watch = watch,
        garminConnect = garmin,
        settings = Settings(serverUrl = server),
        uploads = UploadState(lastError = uploadError),
    )

    @Test
    fun `a healthy paired watch has no problem`() {
        assertNull(WatchProblems.top(state()))
        assertFalse(WatchProblems.needsAttention(state()))
    }

    @Test
    fun `bluetooth problems come first and only once a watch is paired`() {
        val noPermission = allowed.copy(bluetooth = false, bluetoothEnabled = false)
        assertEquals(WatchProblem.BluetoothPermission, WatchProblems.top(state(permissions = noPermission)))
        assertEquals(
            WatchProblem.BluetoothOff,
            WatchProblems.top(state(permissions = allowed.copy(bluetoothEnabled = false))),
        )
        // Unpaired: the checklist asks for Bluetooth itself, no banner; the tab still shows its dot.
        val unpaired = state(watch = WatchState.NotPaired, permissions = noPermission)
        assertNull(WatchProblems.top(unpaired))
        assertTrue(WatchProblems.needsAttention(unpaired))
    }

    @Test
    fun `only the most important problem is returned`() {
        val everything = state(
            watch = paired(unsupported = "not verified"),
            permissions = allowed.copy(bluetoothEnabled = false),
            garmin = GarminConnect.HasBluetooth,
            uploadError = "HTTP 502",
        )
        assertEquals(WatchProblem.BluetoothOff, WatchProblems.top(everything))
        assertEquals(
            WatchProblem.UnsupportedWatch,
            WatchProblems.top(everything.copy(permissions = allowed)),
        )
        assertEquals(
            WatchProblem.GarminConnect,
            WatchProblems.top(everything.copy(permissions = allowed, watch = paired(LinkState.Waiting))),
        )
        assertEquals(
            WatchProblem.UploadFailing,
            WatchProblems.top(state(uploadError = "HTTP 502")),
        )
    }

    @Test
    fun `garmin connect is in the way until our link is ready`() {
        assertEquals(WatchProblem.GarminConnect, WatchProblems.top(state(garmin = GarminConnect.HasBluetooth, watch = paired(LinkState.Waiting))))
        assertEquals(WatchProblem.GarminConnect, WatchProblems.top(state(garmin = GarminConnect.HasBluetooth, watch = WatchState.NotPaired)))
        assertNull(WatchProblems.top(state(garmin = GarminConnect.HasBluetooth, watch = paired(LinkState.Ready))))
        // Kept away on purpose: nothing to fix.
        assertNull(WatchProblems.top(state(garmin = GarminConnect.BluetoothOff, watch = paired(LinkState.Waiting))))
    }

    @Test
    fun `an upload error without a server is not shown`() {
        assertNull(WatchProblems.top(state(server = "", uploadError = "HTTP 502")))
    }

    @Test
    fun `status follows the link and the sync`() {
        fun status(link: LinkState, running: Boolean = false, failed: Int = 0) =
            WatchStatuses.of(paired(link), running, failed)
        assertEquals(WatchStatus.Connected, status(LinkState.Ready))
        assertEquals(WatchStatus.Connecting, status(LinkState.Connecting))
        assertEquals(WatchStatus.NotConnected, status(LinkState.Offline))
        assertEquals(WatchStatus.NotConnected, status(LinkState.Waiting))
        assertEquals(WatchStatus.Syncing, status(LinkState.Ready, running = true))
        assertEquals(WatchStatus.SyncFailed, status(LinkState.Ready, failed = 2))
        assertEquals(WatchStatus.PairingLost, status(LinkState.BondLost, running = true, failed = 1))
    }

    @Test
    fun `the pairing button always has a next step`() {
        assertEquals(SetupAction.AllowBluetooth, PairingSetup.action(Permissions(), scanning = false))
        assertEquals(SetupAction.AllowBluetooth, PairingSetup.action(Permissions(), scanning = true))
        assertEquals(SetupAction.TurnOnBluetooth, PairingSetup.action(allowed.copy(bluetoothEnabled = false), scanning = false))
        assertEquals(SetupAction.Scan, PairingSetup.action(allowed, scanning = false))
        assertEquals(SetupAction.Scanning, PairingSetup.action(allowed, scanning = true))
    }

    @Test
    fun `the checklist moves to the step the person is on`() {
        assertEquals(1, PairingSetup.currentStep(Permissions(), scanning = false, foundAny = false))
        assertEquals(1, PairingSetup.currentStep(allowed.copy(bluetoothEnabled = false), scanning = false, foundAny = false))
        assertEquals(2, PairingSetup.currentStep(allowed, scanning = false, foundAny = false))
        assertEquals(3, PairingSetup.currentStep(allowed, scanning = true, foundAny = false))
        assertEquals(3, PairingSetup.currentStep(allowed, scanning = false, foundAny = true))
    }

    @Test
    fun `a stored interval outside the list stays visible`() {
        assertEquals(listOf(0, 15, 30, 60, 120, 240), SyncIntervals.options(60))
        assertEquals(listOf(0, 15, 20, 30, 60, 120, 240), SyncIntervals.options(20))
    }

    @Test
    fun `the server is shown as its host`() {
        assertEquals("nimrag.panga-pirate.ts.net", ServerHost.of("https://nimrag.panga-pirate.ts.net/"))
        assertEquals("nimrag.example.com", ServerHost.of(" HTTPS://Nimrag.Example.com/app "))
        assertEquals("192.168.1.5:18210", ServerHost.of("http://192.168.1.5:18210"))
        assertEquals("nimrag.example.com", ServerHost.of("https://nimrag.example.com:443"))
        assertEquals("not a url", ServerHost.of("not a url"))
        assertEquals("", ServerHost.of(""))
    }

    @Test
    fun `back leads out of every sub-page to the main page`() {
        assertNull(WatchPage.Main.parent)
        for (page in WatchPage.entries) {
            var at = page
            var steps = 0
            while (at != WatchPage.Main) {
                at = at.parent ?: error("$page has no way back")
                assertTrue(++steps < 4, "$page is too deep")
            }
        }
        assertEquals(WatchPage.About, WatchPage.Licences.parent)
        assertEquals(WatchPage.About, WatchPage.Advanced.parent)
    }
}
