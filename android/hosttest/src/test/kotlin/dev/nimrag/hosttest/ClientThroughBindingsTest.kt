package dev.nimrag.hosttest

import org.junit.jupiter.api.Test
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertTrue
import uniffi.nimrag_ffi.ClientConfig
import uniffi.nimrag_ffi.DstPair
import uniffi.nimrag_ffi.Event
import uniffi.nimrag_ffi.Input
import uniffi.nimrag_ffi.NimragClient
import uniffi.nimrag_ffi.Now
import uniffi.nimrag_ffi.Phase
import uniffi.nimrag_ffi.ScriptedWatch
import uniffi.nimrag_ffi.Duties
import uniffi.nimrag_ffi.ffiVersion
import uniffi.nimrag_ffi.fitProfileCompiledIn

private const val CLIENT_ID: ULong = 0x1234_5678_9ABC_DEF0UL

private fun config() = ClientConfig(
    clientId = CLIENT_ID,
    connectionEpoch = 1u,
    hostName = "TestPhone",
    hostManufacturer = "TestCo",
    hostModel = "T-1",
    appVersion = 1234u,
    capabilities = byteArrayOf(0x11, 0x22),
    firstConnect = false,
    archive = true,
    skip = emptyList(),
    duties = Duties(notifications = false, music = false, findPhone = false, weather = false, gpsAssist = false),
)

private fun now(monoMs: ULong) = Now(
    monoMs = monoMs,
    unixS = 1_800_000_000L,
    utcOffsetS = 3600,
    dst = DstPair(startUnixS = 1_802_000_000L, endUnixS = 1_817_000_000L),
)

/** Close-all request for [CLIENT_ID]: 00 05, client id (u64 LE), 00 00 (core/src/multilink.rs). */
private fun expectedCloseAll(): ByteArray {
    val id = ByteArray(8) { i -> (CLIENT_ID shr (8 * i)).toByte() }
    return byteArrayOf(0, 5) + id + byteArrayOf(0, 0)
}

class ClientThroughBindingsTest {
    @Test
    fun libraryLoadsAndReportsVersionWithoutFitProfile() {
        assertTrue(ffiVersion().isNotEmpty())
        assertFalse(fitProfileCompiledIn(), "the FIT profile must never be compiled in")
    }

    @Test
    fun connectedWritesCloseAllFirst() {
        val client = NimragClient(config())
        assertEquals(Phase.IDLE, client.phase())
        val step = client.handle(now(0u), Input.Connected(23u.toUShort()))
        assertEquals(Phase.BRING_UP, client.phase())
        assertTrue(step.writes.isNotEmpty())
        assertTrue(
            step.writes[0].contentEquals(expectedCloseAll()),
            "first write was ${step.writes[0].joinToString { "%02x".format(it) }}",
        )
    }

    @Test
    fun scriptedWatchReachesHandshakeDone() {
        val client = NimragClient(config())
        val watch = ScriptedWatch(23u.toUShort())
        val events = mutableListOf<Event>()
        var t = 0uL
        var writes = listOf<ByteArray>()
        var infoSent = false

        val first = client.handle(now(t), Input.Connected(23u.toUShort()))
        events += first.events
        writes = first.writes
        while (true) {
            val inbox = ArrayDeque<ByteArray>()
            for (w in writes) inbox.addAll(watch.onWrite(t, w))
            if (!infoSent) {
                val info = watch.deviceInformation(t)
                if (info.isNotEmpty()) {
                    infoSent = true
                    inbox.addAll(info)
                }
            }
            if (inbox.isEmpty()) break
            val next = mutableListOf<ByteArray>()
            for (value in inbox) {
                t += 1u
                val step = client.handle(now(t), Input.Notification(value))
                events += step.events
                next += step.writes
            }
            writes = next
        }

        assertEquals(Phase.READY, client.phase(), "events: $events")
        val done = events.filterIsInstance<Event.HandshakeDone>()
        assertEquals(1, done.size)
        assertEquals("Watch One", done[0].device.modelName)
        assertEquals(1, done[0].fileTypes.size)
        assertTrue(watch.phoneMessageIds().contains(5000.toUShort()), "the phone ACKed DeviceInformation")
    }

    @Test
    fun staleFileStoredGivesStaleConfirmation() {
        val client = NimragClient(config())
        client.handle(now(0u), Input.Connected(23u.toUShort()))
        // Top bit set and above 2^53: must survive u64 -> ULong -> u64 intact.
        val id = 0xFEDC_BA98_7654_3210UL
        val step = client.handle(now(1u), Input.FileStored(id))
        assertEquals(listOf<Event>(Event.StaleConfirmation(id)), step.events)
    }
}
