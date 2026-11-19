package dev.nimrag.app.engine.logic

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test
import java.util.UUID
import kotlin.random.Random

class GattPairTest {
    @Test
    fun `service and characteristic uuids have the Garmin layout`() {
        assertEquals(UUID.fromString("6A4E2800-667B-11E3-949A-0800200C9A66"), GattPair.SERVICE)
        assertEquals(UUID.fromString("6A4E2815-667B-11E3-949A-0800200C9A66"), GattPair.uuid(0x2815))
    }

    @Test
    fun `notify 2810 to 2819 pairs with write 2820 to 2829`() {
        for (id in 0x2810..0x2819) {
            assertEquals(GattPair.uuid(id + 0x10), GattPair.writeUuidFor(id))
            assertEquals(id, GattPair.notifyId(GattPair.uuid(id)))
        }
        assertEquals(UUID.fromString("6A4E2820-667B-11E3-949A-0800200C9A66"), GattPair.writeUuidFor(0x2810))
        assertEquals(UUID.fromString("6A4E2829-667B-11E3-949A-0800200C9A66"), GattPair.writeUuidFor(0x2819))
    }

    @Test
    fun `other characteristics are not notify characteristics`() {
        assertNull(GattPair.notifyId(GattPair.uuid(0x2803))) // info characteristic
        assertNull(GattPair.notifyId(GattPair.uuid(0x2820))) // a write characteristic
        assertNull(GattPair.notifyId(GattPair.uuid(0x281A)))
        assertNull(GattPair.notifyId(UUID.fromString("00002902-0000-1000-8000-00805f9b34fb")))
    }

    private fun present(vararg ids: Int) = ids.map { GattPair.uuid(it) }

    @Test
    fun `choose needs both the notify and the write characteristic`() {
        // 2812 has no write partner, 2823 has no notify partner: only 2811 is usable.
        val chars = present(0x2803, 0x2811, 0x2821, 0x2812, 0x2823)
        repeat(20) { assertEquals(0x2811, GattPair.choose(chars)) }
    }

    @Test
    fun `choose picks among the usable pairs`() {
        val chars = present(0x2810, 0x2820, 0x2811, 0x2821, 0x2812, 0x2822)
        val picks = (0 until 200).map { GattPair.choose(chars, random = Random(it)) }.toSet()
        assertEquals(setOf(0x2810, 0x2811, 0x2812), picks)
    }

    @Test
    fun `a forced pair wins when it is usable, otherwise it is ignored`() {
        val chars = present(0x2810, 0x2820, 0x2814, 0x2824)
        repeat(20) { assertEquals(0x2814, GattPair.choose(chars, forced = 0x2814)) }
        val other = GattPair.choose(chars, forced = 0x2817, random = Random(1))
        assertTrue(other == 0x2810 || other == 0x2814)
    }

    @Test
    fun `a pair that was in use is avoided while another exists`() {
        val two = present(0x2810, 0x2820, 0x2811, 0x2821)
        repeat(20) { assertEquals(0x2811, GattPair.choose(two, avoid = 0x2810, random = Random(it))) }
        val one = present(0x2810, 0x2820)
        assertEquals(0x2810, GattPair.choose(one, avoid = 0x2810))
    }

    @Test
    fun `no usable pair gives null`() {
        assertNull(GattPair.choose(emptyList()))
        assertNull(GattPair.choose(present(0x2803, 0x2810)))
    }

    @Test
    fun `ChannelInUse names the characteristic to retry on`() {
        assertEquals(0x2812, GattPair.notifyIdFromRetryChar(0x2812))
        assertEquals(0x2819, GattPair.notifyIdFromRetryChar(0x2819))
        // Leniently: the write id of the same pair.
        assertEquals(0x2812, GattPair.notifyIdFromRetryChar(0x2822))
        assertNull(GattPair.notifyIdFromRetryChar(0x2803))
        assertNull(GattPair.notifyIdFromRetryChar(0x0000))
        assertNull(GattPair.notifyIdFromRetryChar(0x282A))
    }
}
