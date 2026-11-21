package dev.nimrag.app.engine.phone.logic

/*
 * The capability bit numbers (GNCS 6, ADVANCED_MUSIC 7, FIND_MY_PHONE 8, FIND_MY_WATCH 9,
 * CONNECTIQ_HTTP 10, WEATHER_CONDITIONS 26, BASIC_MUSIC 52, HTTP_RAW_RESOURCE_REQUEST 88) are taken from Gadgetbridge commit 7257742b (AGPL-3.0-or-later),
 * app/src/main/java/nodomain/freeyourgadget/gadgetbridge/service/devices/garmin/GarminCapability.java
 * (no copyright line in the file; Gadgetbridge contributors), and which of them a duty claims from
 * GarminSupport.java. WEATHER_CONDITIONS (26) is from the same file; WEATHER_ALERTS (27) is not
 * claimed. The rest of the file is our own. Inferred (Gadgetbridge) until the stage 2f
 * live test confirms that the watch asks for a duty when its bit is claimed. See docs/PORTED.md.
 */

import dev.nimrag.app.model.PhoneAccess
import dev.nimrag.app.model.PhoneFeatures

/** Which companion duties a connection is built with (the protocol client's `Duties`). */
data class DutyMask(
    val notifications: Boolean = false,
    val music: Boolean = false,
    val findPhone: Boolean = false,
    val weather: Boolean = false,
    /** The watch's GPS-assist download is answered (satellite data from the server). Needs no system access. */
    val gpsAssist: Boolean = false,
) {
    val any: Boolean get() = notifications || music || findPhone || weather || gpsAssist
}

/**
 * Turns the owner's switches and what the system allows into the duties of a connection, and
 * claims the matching capability bits of our Configuration (5050) mask. The watch only asks for
 * a duty when we claim it, so a duty that is off is never claimed and the watch behaves as it
 * did before stage 2f. Bit numbers are *inferred (Gadgetbridge)* (`GarminCapability`), checked
 * on a watch only in the stage 2f live test.
 */
object PhoneDuties {
    const val BIT_GNCS = 6
    const val BIT_ADVANCED_MUSIC = 7
    const val BIT_FIND_MY_PHONE = 8
    const val BIT_FIND_MY_WATCH = 9
    const val BIT_BASIC_MUSIC = 52

    /** WEATHER_CONDITIONS; WEATHER_ALERTS (27) is never claimed. */
    const val BIT_WEATHER_CONDITIONS = 26

    /** CONNECTIQ_HTTP and HTTP_RAW_RESOURCE_REQUEST: claimed only with GPS assist on (inferred, Gadgetbridge). */
    const val BIT_CONNECTIQ_HTTP = 10
    const val BIT_HTTP_RAW_RESOURCE_REQUEST = 88

    /**
     * A duty runs only when it is switched on and the system lets us do it: notifications and
     * music both need "Notification access" (the notification listener and the media sessions);
     * finding the phone needs nothing from the system.
     */
    fun effective(features: PhoneFeatures, access: PhoneAccess): DutyMask = DutyMask(
        notifications = features.notifications && access.notificationAccess,
        music = features.music && access.notificationAccess,
        findPhone = features.findPhone,
        weather = features.weather,
        gpsAssist = features.gpsAssist,
    )

    /** The bits a duty set claims. */
    fun bits(duties: DutyMask): List<Int> = buildList {
        if (duties.notifications) add(BIT_GNCS)
        if (duties.music) {
            add(BIT_ADVANCED_MUSIC)
            add(BIT_BASIC_MUSIC)
        }
        if (duties.findPhone) {
            add(BIT_FIND_MY_PHONE)
            add(BIT_FIND_MY_WATCH)
        }
        if (duties.weather) add(BIT_WEATHER_CONDITIONS)
        if (duties.gpsAssist) {
            add(BIT_CONNECTIQ_HTTP)
            add(BIT_HTTP_RAW_RESOURCE_REQUEST)
        }
    }

    /**
     * [base] with the bits of [duties] set (bit i is byte i/8, value 1 shl (i%8)); the mask grows
     * with zero bytes when a bit lies beyond its end. Never clears a bit, never changes [base].
     */
    fun claim(base: ByteArray, duties: DutyMask): ByteArray {
        val bits = bits(duties)
        if (bits.isEmpty()) return base
        val needed = bits.maxOf { it / 8 + 1 }
        val out = if (base.size >= needed) base.copyOf() else base.copyOf(needed)
        for (bit in bits) {
            val index = bit / 8
            out[index] = (out[index].toInt() or (1 shl (bit % 8))).toByte()
        }
        return out
    }
}
