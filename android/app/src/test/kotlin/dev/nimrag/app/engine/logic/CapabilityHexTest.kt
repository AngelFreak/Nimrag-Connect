package dev.nimrag.app.engine.logic

import org.junit.jupiter.api.Assertions.assertArrayEquals
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertNotNull
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Test

class CapabilityHexTest {
    private fun bit(mask: ByteArray, i: Int) = (mask[i / 8].toInt() shr (i % 8)) and 1 == 1

    @Test
    fun `valid hex gives the bytes in order`() {
        assertArrayEquals(byteArrayOf(0x39, 0x00, 0x7F, 0xFF.toByte()), CapabilityHex.parse("39007fff"))
    }

    @Test
    fun `upper case and whitespace are accepted`() {
        assertArrayEquals(byteArrayOf(0xAB.toByte(), 0xCD.toByte()), CapabilityHex.parse(" AB cd\n"))
        assertEquals("abcd", CapabilityHex.normalize("AB CD"))
    }

    @Test
    fun `odd length is rejected`() {
        assertNull(CapabilityHex.parse("abc"))
        assertNull(CapabilityHex.parse("3"))
    }

    @Test
    fun `non-hex characters are rejected`() {
        assertNull(CapabilityHex.parse("zz"))
        assertNull(CapabilityHex.parse("0x39"))
        assertNull(CapabilityHex.parse("39 0g"))
        assertNull(CapabilityHex.normalize("12-34"))
    }

    @Test
    fun `empty and blank are rejected`() {
        assertNull(CapabilityHex.parse(""))
        assertNull(CapabilityHex.parse("   "))
    }

    @Test
    fun `the mask is limited to what a length byte can announce`() {
        assertNotNull(CapabilityHex.parse("00".repeat(255)))
        assertNull(CapabilityHex.parse("00".repeat(256)))
    }

    @Test
    fun `the default claims exactly the planned bits`() {
        val mask = CapabilityHex.parse(CapabilityHex.DEFAULT)!!
        assertEquals(13, mask.size)
        val set = (0 until mask.size * 8).filter { bit(mask, it) }
        // 0 CONNECT_MOBILE_FIT_LINK, 3 SYNC, 4 DEVICE_INITIATES_SYNC, 5 HOST_INITIATED_SYNC_REQUESTS,
        // 29 EXPLICIT_ARCHIVE, 71 CURRENT_TIME_REQUEST_SUPPORT, 76 MULTI_LINK_SERVICE
        assertEquals(listOf(0, 3, 4, 5, 29, 71, 76), set)
        assertEquals(CapabilityHex.DEFAULT, CapabilityHex.normalize(CapabilityHex.DEFAULT))
    }
}
