package dev.nimrag.app.engine.logic

/**
 * Our Configuration (5050) capability mask as hex, edited in Settings (advanced).
 * Bit i is byte i/8, value 1 shl (i%8).
 */
object CapabilityHex {
    /**
     * Bits 0 CONNECT_MOBILE_FIT_LINK, 3 SYNC, 4 DEVICE_INITIATES_SYNC,
     * 5 HOST_INITIATED_SYNC_REQUESTS, 29 EXPLICIT_ARCHIVE, 71 CURRENT_TIME_REQUEST_SUPPORT,
     * 76 MULTI_LINK_SERVICE (IMPLEMENTATION_PLAN.md stage 2c). Only claims what the client
     * implements; to be confirmed live.
     */
    const val DEFAULT = "39000020000000008010000000"

    /** The client sends the mask as `u8 length` + bytes. */
    const val MAX_BYTES = 255

    /** Whitespace is ignored. Null for empty, odd-length, non-hex or longer than [MAX_BYTES]. */
    fun parse(hex: String): ByteArray? {
        val clean = hex.filterNot { it.isWhitespace() }
        if (clean.isEmpty() || clean.length % 2 != 0 || clean.length > MAX_BYTES * 2) return null
        val out = ByteArray(clean.length / 2)
        for (i in out.indices) {
            val hi = Character.digit(clean[2 * i], 16)
            val lo = Character.digit(clean[2 * i + 1], 16)
            if (hi < 0 || lo < 0) return null
            out[i] = ((hi shl 4) or lo).toByte()
        }
        return out
    }

    /** Lower-case, no whitespace; null when [parse] rejects it. */
    fun normalize(hex: String): String? =
        parse(hex)?.joinToString("") { "%02x".format(it.toInt() and 0xFF) }
}
