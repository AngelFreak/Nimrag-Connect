package dev.nimrag.app.engine.phone.logic

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

class FindPhoneRulesTest {
    @Test
    fun `the ring time is clamped to ten seconds up to two minutes, zero means the default`() {
        assertEquals(30, FindPhoneRules.ringSeconds(0))
        assertEquals(30, FindPhoneRules.ringSeconds(-4))
        assertEquals(10, FindPhoneRules.ringSeconds(1))
        assertEquals(10, FindPhoneRules.ringSeconds(10))
        assertEquals(45, FindPhoneRules.ringSeconds(45))
        assertEquals(120, FindPhoneRules.ringSeconds(120))
        assertEquals(120, FindPhoneRules.ringSeconds(255))
    }

    @Test
    fun `the vibration repeats from the buzz, not from the leading pause`() {
        assertEquals(1, FindPhoneRules.VIBRATION_REPEAT_INDEX)
        assertEquals(0L, FindPhoneRules.VIBRATION_PATTERN[0])
        assertTrue(FindPhoneRules.VIBRATION_PATTERN.size > FindPhoneRules.VIBRATION_REPEAT_INDEX)
    }

    @Test
    fun `a session starts once, a second request only moves the end`() {
        val s = FindPhoneSession()
        assertFalse(s.ringing)
        assertTrue(s.start(nowMs = 1_000, seconds = 30))
        assertTrue(s.ringing)
        assertEquals(30_000L, s.remainingMs(1_000))
        assertEquals(20_000L, s.remainingMs(11_000))
        assertFalse(s.start(nowMs = 11_000, seconds = 30))
        assertEquals(30_000L, s.remainingMs(11_000))
    }

    @Test
    fun `stop reports whether anything rang, and a stop with nothing ringing is harmless`() {
        val s = FindPhoneSession()
        assertFalse(s.stop())
        s.start(0, 30)
        assertTrue(s.stop())
        assertFalse(s.ringing)
        assertFalse(s.stop())
        assertEquals(0L, s.remainingMs(5))
    }

    @Test
    fun `nothing is left once the time is over`() {
        val s = FindPhoneSession()
        s.start(0, 10)
        assertEquals(0L, s.remainingMs(10_000))
        assertEquals(0L, s.remainingMs(99_000))
    }
}

class MusicRulesTest {
    @Test
    fun `toggle plays when paused and pauses when playing`() {
        assertEquals(TransportAction.Play, MusicRules.actionFor(WatchMusicCommand.TogglePlayPause, playing = false))
        assertEquals(TransportAction.Pause, MusicRules.actionFor(WatchMusicCommand.TogglePlayPause, playing = true))
    }

    @Test
    fun `every watch command has an action`() {
        for (c in WatchMusicCommand.entries) MusicRules.actionFor(c, true)
        assertEquals(TransportAction.Play, MusicRules.actionFor(WatchMusicCommand.Play, true))
        assertEquals(TransportAction.Pause, MusicRules.actionFor(WatchMusicCommand.Pause, false))
        assertEquals(TransportAction.Next, MusicRules.actionFor(WatchMusicCommand.Next, false))
        assertEquals(TransportAction.Previous, MusicRules.actionFor(WatchMusicCommand.Previous, false))
        assertEquals(TransportAction.VolumeUp, MusicRules.actionFor(WatchMusicCommand.VolumeUp, false))
        assertEquals(TransportAction.VolumeDown, MusicRules.actionFor(WatchMusicCommand.VolumeDown, false))
        assertEquals(TransportAction.SeekForward, MusicRules.actionFor(WatchMusicCommand.SkipForward, false))
        assertEquals(TransportAction.SeekBackward, MusicRules.actionFor(WatchMusicCommand.SkipBackward, false))
    }

    @Test
    fun `playing and buffering count as playing`() {
        assertTrue(MusicRules.isPlaying(3))
        assertTrue(MusicRules.isPlaying(6))
        assertFalse(MusicRules.isPlaying(2)) // paused
        assertFalse(MusicRules.isPlaying(1)) // stopped
        assertFalse(MusicRules.isPlaying(0))
    }

    @Test
    fun `a playing position moves on with the time since the player reported it`() {
        val p = MusicRules.playback(playbackState = 3, speed = 1f, positionMs = 42_000, updateTimeMs = 1_000, nowMs = 6_500)
        assertTrue(p.playing)
        assertEquals(100, p.ratePercent)
        assertEquals(47L, p.positionS) // 42.0 s + 5.5 s, whole seconds
    }

    @Test
    fun `a paused position stays where it is`() {
        val p = MusicRules.playback(playbackState = 2, speed = 0f, positionMs = 42_900, updateTimeMs = 1_000, nowMs = 60_000)
        assertFalse(p.playing)
        assertEquals(100, p.ratePercent)
        assertEquals(42L, p.positionS)
    }

    @Test
    fun `the speed scales the movement and is bounded`() {
        assertEquals(20L, MusicRules.playback(3, 2f, 10_000, 1, 5_001).positionS)
        assertEquals(200, MusicRules.playback(3, 2f, 0, 1, 2).ratePercent)
        assertEquals(400, MusicRules.playback(3, 99f, 0, 1, 2).ratePercent)
        assertEquals(100, MusicRules.playback(3, Float.NaN, 0, 1, 2).ratePercent)
        assertEquals(100, MusicRules.playback(3, -1f, 0, 1, 2).ratePercent)
    }

    @Test
    fun `an unknown position or a clock that went backwards gives a sane answer`() {
        assertEquals(0L, MusicRules.playback(3, 1f, -1, 1_000, 9_000).positionS)
        assertEquals(30L, MusicRules.playback(3, 1f, 30_000, 9_000, 1_000).positionS)
        assertEquals(30L, MusicRules.playback(3, 1f, 30_000, 0, 1_000).positionS)
    }

    @Test
    fun `a skip stays inside the track`() {
        assertEquals(20_000L, MusicRules.seekTarget(10_000, MusicRules.SEEK_STEP_MS, durationMs = 200_000))
        assertEquals(0L, MusicRules.seekTarget(4_000, -MusicRules.SEEK_STEP_MS, durationMs = 200_000))
        assertEquals(200_000L, MusicRules.seekTarget(195_000, MusicRules.SEEK_STEP_MS, durationMs = 200_000))
        assertEquals(205_000L, MusicRules.seekTarget(195_000, MusicRules.SEEK_STEP_MS, durationMs = 0))
    }

    @Test
    fun `volume is a percent of the maximum`() {
        assertEquals(0, MusicRules.volumePercent(0, 15))
        assertEquals(100, MusicRules.volumePercent(15, 15))
        assertEquals(46, MusicRules.volumePercent(7, 15))
        assertEquals(100, MusicRules.volumePercent(99, 15))
        assertEquals(0, MusicRules.volumePercent(5, 0))
        assertEquals(0, MusicRules.volumePercent(-3, 15))
    }

    @Test
    fun `the watch follows the session that plays, else one with a track, else the first`() {
        assertNull(MusicRules.pickSession(emptyList()))
        val idle = SessionInfo(playing = false, hasMetadata = false)
        val track = SessionInfo(playing = false, hasMetadata = true)
        val playing = SessionInfo(playing = true, hasMetadata = true)
        assertEquals(2, MusicRules.pickSession(listOf(idle, track, playing)))
        assertEquals(1, MusicRules.pickSession(listOf(idle, track, track)))
        assertEquals(0, MusicRules.pickSession(listOf(idle, idle)))
        assertEquals(0, MusicRules.pickSession(listOf(playing, playing)))
    }

    @Test
    fun `printing a track shows none of it`() {
        val t = MusicTrack("Hidden Artist", "Hidden Album", "Hidden Title", 215)
        assertFalse(t.toString().contains("Hidden"))
        assertTrue(t.toString().contains("215"))
        assertEquals(t, MusicTrack("Hidden Artist", "Hidden Album", "Hidden Title", 215))
    }
}
