package dev.nimrag.app.engine.logic

import java.time.Duration
import java.time.Instant
import java.time.zone.ZoneRules

/**
 * The daylight-saving pair of the watch's time reply (5052), PROTOCOL.md §3 step 6:
 * `dst_start` is the next transition into daylight saving time, `dst_end` the next
 * transition out of it, so in summer time `dst_start` is the later of the two.
 * Both are 0 when the zone has no further transition (the caller sends `None`).
 */
data class DstSeconds(val startUnixS: Long, val endUnixS: Long)

object DstRule {
    /**
     * Garmin Connect's rule: t1 = the next zone transition after [now]. If t1 goes into
     * DST, start = t1 and end = the next transition after t1 + 1 day; otherwise
     * start = the next transition after t1 + 1 day and end = t1. Null when the zone has
     * no transition after [now]. If only one of the two is known (the zone drops DST for
     * good) the other is 0.
     */
    fun compute(rules: ZoneRules, now: Instant): DstSeconds? {
        val t1 = rules.nextTransition(now) ?: return null
        val t2 = rules.nextTransition(t1.instant.plus(Duration.ofDays(1)))?.instant?.epochSecond ?: 0L
        val first = t1.instant.epochSecond
        // At the transition instant the new offset is already in force.
        return if (rules.isDaylightSavings(t1.instant)) {
            DstSeconds(startUnixS = first, endUnixS = t2)
        } else {
            DstSeconds(startUnixS = t2, endUnixS = first)
        }
    }
}
