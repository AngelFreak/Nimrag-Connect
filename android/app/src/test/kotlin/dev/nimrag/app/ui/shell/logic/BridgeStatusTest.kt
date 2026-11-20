package dev.nimrag.app.ui.shell.logic

import dev.nimrag.app.model.AppState
import dev.nimrag.app.model.LinkState
import dev.nimrag.app.model.Permissions
import dev.nimrag.app.model.SyncResult
import dev.nimrag.app.model.SyncState
import dev.nimrag.app.model.WatchState
import java.time.Instant
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

class BridgeStatusTest {
    private val ok = Permissions(bluetooth = true, notifications = true, bluetoothEnabled = true)

    private fun paired(link: LinkState, unsupported: String? = null) =
        WatchState.Paired("AA:BB", "Instinct 2", "Instinct 2", link, unsupportedReason = unsupported)

    private fun state(watch: WatchState, permissions: Permissions = ok, lastSyncMs: Long? = null) = AppState(
        permissions = permissions,
        watch = watch,
        sync = SyncState(lastResult = lastSyncMs?.let { SyncResult(it, downloaded = 1, archived = 1, failed = 0) }),
    )

    @Test
    fun `no watch is unknown`() {
        assertEquals(BridgeLink.Unknown, BridgeStatus.linkOf(state(WatchState.NotPaired)))
    }

    @Test
    fun `pairing is waiting`() {
        assertEquals(BridgeLink.Waiting, BridgeStatus.linkOf(state(WatchState.Pairing("AA", "Instinct 2"))))
    }

    @Test
    fun `each link state maps as documented`() {
        val expected = mapOf(
            LinkState.Ready to BridgeLink.Connected,
            LinkState.Waiting to BridgeLink.Waiting,
            LinkState.Connecting to BridgeLink.Waiting,
            LinkState.Offline to BridgeLink.Problem,
            LinkState.BondLost to BridgeLink.Problem,
        )
        assertEquals(LinkState.entries.toSet(), expected.keys, "every LinkState is mapped")
        for ((link, bridge) in expected) assertEquals(bridge, BridgeStatus.linkOf(state(paired(link))), "$link")
    }

    @Test
    fun `missing bluetooth permission or adapter off is a problem even when the link says ready`() {
        assertEquals(BridgeLink.Problem, BridgeStatus.linkOf(state(paired(LinkState.Ready), ok.copy(bluetooth = false))))
        assertEquals(BridgeLink.Problem, BridgeStatus.linkOf(state(paired(LinkState.Waiting), ok.copy(bluetoothEnabled = false))))
    }

    @Test
    fun `a model that cannot sync is a problem`() {
        assertEquals(BridgeLink.Problem, BridgeStatus.linkOf(state(paired(LinkState.Ready, unsupported = "not verified"))))
    }

    @Test
    fun `json carries the wire name and an ISO-8601 UTC time truncated to seconds`() {
        val ms = Instant.parse("2026-11-19T10:00:00.789Z").toEpochMilli()
        assertEquals(
            "{\"link\":\"connected\",\"lastSyncAt\":\"2026-11-19T10:00:00Z\"}",
            BridgeStatus(BridgeLink.Connected, ms).toJson(),
        )
    }

    @Test
    fun `json has null when there was no sync`() {
        assertEquals("{\"link\":\"unknown\",\"lastSyncAt\":null}", BridgeStatus.Unknown.toJson())
        assertEquals("{\"link\":\"waiting\",\"lastSyncAt\":null}", BridgeStatus(BridgeLink.Waiting, 0L).toJson())
    }

    @Test
    fun `status takes the last finished sync from the state`() {
        val ms = Instant.parse("2026-11-19T08:30:15Z").toEpochMilli()
        val s = BridgeStatus.of(state(paired(LinkState.Ready), lastSyncMs = ms))
        assertEquals(BridgeStatus(BridgeLink.Connected, ms), s)
        assertTrue(s.toJson().contains("\"lastSyncAt\":\"2026-11-19T08:30:15Z\""))
    }

    @Test
    fun `equal statuses compare equal so the shell dispatches only on change`() {
        val a = BridgeStatus.of(state(paired(LinkState.Ready), lastSyncMs = 5_000))
        val same = BridgeStatus.of(state(paired(LinkState.Ready), lastSyncMs = 5_000).copy(log = emptyList()))
        val other = BridgeStatus.of(state(paired(LinkState.Ready), lastSyncMs = 6_000))
        assertEquals(a, same)
        assertFalse(a == other)
    }

    @Test
    fun `the bridge script defines the web contract`() {
        val js = BridgeScripts.DEFINE_BRIDGE
        for (needle in listOf("window.NimragApp", "native: true", "syncNow", "status", "openWatch", "onRoute", "NimragAppNative")) {
            assertTrue(js.contains(needle), needle)
        }
        assertEquals("NimragAppNative", BridgeScripts.INTERFACE_NAME)
    }

    @Test
    fun `events are dispatched on window with the contract names`() {
        assertTrue(BridgeScripts.statusEvent("{\"link\":\"unknown\",\"lastSyncAt\":null}").contains("'nimrag:status'"))
        val nav = BridgeScripts.navigateEvent("/activities")
        assertTrue(nav.contains("'nimrag:navigate'"))
        assertTrue(nav.contains("detail:{path:\"/activities\"}"))
        assertEquals("window.dispatchEvent(new CustomEvent('nimrag:data'));", BridgeScripts.DATA_EVENT_SCRIPT)
    }

    @Test
    fun `a path is quoted as a JavaScript string`() {
        assertEquals("\"/a\\\"b\"", BridgeScripts.jsString("/a\"b"))
        assertEquals("\"\\\\\"", BridgeScripts.jsString("\\"))
        assertEquals("\"\\u003c/script>\"", BridgeScripts.jsString("</script>"))
        assertEquals("\"a\\nb\"", BridgeScripts.jsString("a\nb"))
    }
}
