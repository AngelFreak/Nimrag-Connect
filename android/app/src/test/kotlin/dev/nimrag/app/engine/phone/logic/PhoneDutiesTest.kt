package dev.nimrag.app.engine.phone.logic

import dev.nimrag.app.engine.logic.CapabilityHex
import dev.nimrag.app.model.PhoneAccess
import dev.nimrag.app.model.PhoneFeatures
import org.junit.jupiter.api.Assertions.assertArrayEquals
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertSame
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

class PhoneDutiesTest {
    private val granted = PhoneAccess(notificationAccess = true)
    private val denied = PhoneAccess(notificationAccess = false)

    @Test
    fun `everything is off by default`() {
        val features = PhoneFeatures()
        assertFalse(features.notifications)
        assertFalse(features.music)
        assertFalse(features.findPhone)
        assertTrue(features.excludedApps.isEmpty())
        assertFalse(PhoneDuties.effective(features, granted).any)
    }

    @Test
    fun `notifications and music need notification access, finding the phone does not`() {
        val all = PhoneFeatures(notifications = true, music = true, findPhone = true)
        assertEquals(DutyMask(notifications = true, music = true, findPhone = true), PhoneDuties.effective(all, granted))
        assertEquals(DutyMask(notifications = false, music = false, findPhone = true), PhoneDuties.effective(all, denied))
    }

    @Test
    fun `a switch that is off stays off even with access`() {
        val onlyMusic = PhoneFeatures(music = true)
        assertEquals(DutyMask(music = true), PhoneDuties.effective(onlyMusic, granted))
    }

    @Test
    fun `no duty claims no bit and returns the mask unchanged`() {
        val base = CapabilityHex.parse(CapabilityHex.DEFAULT)!!
        assertSame(base, PhoneDuties.claim(base, DutyMask()))
    }

    @Test
    fun `claiming sets the bits of the duty on the default mask`() {
        val base = CapabilityHex.parse(CapabilityHex.DEFAULT)!!
        // default: 39 00 00 20 00 00 00 00 80 10 00 00 00
        val notes = PhoneDuties.claim(base, DutyMask(notifications = true))
        assertEquals(0x79, notes[0].toInt() and 0xFF) // bit 6 added to 0x39
        assertArrayEquals(base.copyOfRange(1, base.size), notes.copyOfRange(1, notes.size))

        val music = PhoneDuties.claim(base, DutyMask(music = true))
        assertEquals(0xB9, music[0].toInt() and 0xFF) // bit 7
        assertEquals(0x10, music[6].toInt() and 0xFF) // bit 52 = byte 6, 1 shl 4

        val find = PhoneDuties.claim(base, DutyMask(findPhone = true))
        assertEquals(0x03, find[1].toInt() and 0xFF) // bits 8 and 9
        assertEquals(0x39, find[0].toInt() and 0xFF)
    }

    @Test
    fun `claiming never clears a bit and never changes the input`() {
        val base = CapabilityHex.parse(CapabilityHex.DEFAULT)!!
        val copy = base.copyOf()
        val out = PhoneDuties.claim(base, DutyMask(true, true, true))
        assertArrayEquals(copy, base)
        for (i in base.indices) {
            assertEquals(base[i].toInt() and 0xFF, (base[i].toInt() and out[i].toInt()) and 0xFF, "byte $i lost a bit")
        }
    }

    @Test
    fun `a short mask grows with zero bytes up to the highest claimed bit`() {
        val out = PhoneDuties.claim(byteArrayOf(0x01), DutyMask(music = true))
        assertEquals(7, out.size)
        assertEquals(0x81, out[0].toInt() and 0xFF)
        assertEquals(0x10, out[6].toInt() and 0xFF)
        assertEquals(0, out[3].toInt())
        // notifications alone fit in the first byte
        assertEquals(1, PhoneDuties.claim(byteArrayOf(0x00), DutyMask(notifications = true)).size)
    }

    @Test
    fun `claiming twice is the same as claiming once`() {
        val base = CapabilityHex.parse(CapabilityHex.DEFAULT)!!
        val once = PhoneDuties.claim(base, DutyMask(true, true, true))
        assertArrayEquals(once, PhoneDuties.claim(once, DutyMask(true, true, true)))
    }

    @Test
    fun `bit lists match the protocol client`() {
        assertEquals(listOf(6), PhoneDuties.bits(DutyMask(notifications = true)))
        assertEquals(listOf(7, 52), PhoneDuties.bits(DutyMask(music = true)))
        assertEquals(listOf(8, 9), PhoneDuties.bits(DutyMask(findPhone = true)))
        assertEquals(listOf(26), PhoneDuties.bits(DutyMask(weather = true)))
    }

    @Test
    fun `weather is off by default and needs nothing from the system`() {
        assertFalse(PhoneFeatures().weather)
        assertFalse(PhoneDuties.effective(PhoneFeatures(), granted).weather)
        assertEquals(DutyMask(weather = true), PhoneDuties.effective(PhoneFeatures(weather = true), denied))
        assertTrue(DutyMask(weather = true).any)
        assertFalse(DutyMask().any)
    }

    @Test
    fun `weather claims capability 26 only when it is on, and never the alerts bit 27`() {
        val base = CapabilityHex.parse(CapabilityHex.DEFAULT)!!
        assertEquals(0x20, base[3].toInt() and 0xFF)
        // bit 26 = byte 3, 1 shl 2
        val weather = PhoneDuties.claim(base, DutyMask(weather = true))
        assertEquals(0x24, weather[3].toInt() and 0xFF)
        assertEquals(0, weather[3].toInt() and 0x08) // bit 27 stays clear
        assertArrayEquals(base.copyOfRange(0, 3), weather.copyOfRange(0, 3))
        assertArrayEquals(base.copyOfRange(4, base.size), weather.copyOfRange(4, weather.size))
        // the other duties leave bit 26 alone
        val others = PhoneDuties.claim(base, DutyMask(notifications = true, music = true, findPhone = true))
        assertEquals(0x20, others[3].toInt() and 0xFF)
        // a short mask grows to byte 3
        val grown = PhoneDuties.claim(byteArrayOf(0x01), DutyMask(weather = true))
        assertEquals(4, grown.size)
        assertEquals(0x04, grown[3].toInt() and 0xFF)
    }

    @Test
    fun `GPS assist is off by default, needs no system access and claims the two HTTP bits only when on`() {
        assertFalse(PhoneFeatures().gpsAssist)
        assertEquals(0L, PhoneFeatures().gpsAssistLastSentMs)
        assertFalse(PhoneDuties.effective(PhoneFeatures(), granted).gpsAssist)
        assertEquals(DutyMask(gpsAssist = true), PhoneDuties.effective(PhoneFeatures(gpsAssist = true), denied))
        assertTrue(DutyMask(gpsAssist = true).any)
        assertEquals(listOf(10, 88), PhoneDuties.bits(DutyMask(gpsAssist = true)))
        // 28 (GPS_EPHEMERIS_DOWNLOAD) is not claimed: the EPO messages are not implemented.
        assertFalse(PhoneDuties.bits(DutyMask(true, true, true, weather = true, gpsAssist = true)).contains(28))
        assertEquals(listOf(6, 7, 52, 8, 9, 26, 10, 88), PhoneDuties.bits(DutyMask(true, true, true, weather = true, gpsAssist = true)))
        assertEquals(emptyList<Int>(), PhoneDuties.bits(DutyMask(notifications = false)))
    }

    @Test
    fun `claiming GPS assist sets bit 10 and bit 88 on the default mask and nothing else`() {
        val base = CapabilityHex.parse(CapabilityHex.DEFAULT)!!
        val out = PhoneDuties.claim(base, DutyMask(gpsAssist = true))
        assertEquals(0x04, out[1].toInt() and 0xFF) // bit 10 = byte 1, 1 shl 2
        assertEquals(0x01, out[11].toInt() and 0xFF) // bit 88 = byte 11, 1 shl 0
        for (i in base.indices) {
            if (i != 1 && i != 11) assertEquals(base[i], out[i], "byte $i")
        }
        // Without the duty the mask is returned unchanged.
        assertSame(base, PhoneDuties.claim(base, DutyMask(gpsAssist = false)))
    }
}
