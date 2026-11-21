package dev.nimrag.app.engine.phone.logic

/*
 * The list of watch commands in WatchMusicCommand follows GarminMusicControlCommand in Gadgetbridge
 * commit 7257742b (AGPL-3.0-or-later),
 * app/src/main/java/nodomain/freeyourgadget/gadgetbridge/service/devices/garmin/messages/MusicControlMessage.java
 * (no copyright line in the file; Gadgetbridge contributors), and what each one does to the phone's
 * player follows GarminSupport.java. The session choice, position and volume arithmetic are our own.
 * Inferred (Gadgetbridge) until the stage 2f live test. See docs/PORTED.md.
 */

/** What a button on the watch asks for (the protocol client's `MusicCommand`). */
enum class WatchMusicCommand {
    TogglePlayPause,
    Next,
    Previous,
    VolumeUp,
    VolumeDown,
    Play,
    Pause,
    SkipForward,
    SkipBackward,
}

/** What the phone does about it. */
enum class TransportAction { Play, Pause, Next, Previous, VolumeUp, VolumeDown, SeekForward, SeekBackward }

/** The track the watch shows. Private: [toString] hides it. */
data class MusicTrack(
    val artist: String,
    val album: String,
    val title: String,
    val durationS: Long,
) {
    override fun toString() = "MusicTrack(durationS=$durationS)"
}

/** Where the phone's player stands, in the watch's terms. */
data class MusicPlayback(val playing: Boolean, val ratePercent: Int, val positionS: Long)

/** A media session the phone has, as far as choosing one needs it. */
data class SessionInfo(val playing: Boolean, val hasMetadata: Boolean)

/** The rules between Android's media sessions and the watch's music screen. */
object MusicRules {
    /** Fast forward and rewind buttons on the watch move the position by this much. */
    const val SEEK_STEP_MS = 10_000L

    // PlaybackState.STATE_* values (android.media.session.PlaybackState); pure code cannot import it.
    const val STATE_PLAYING = 3
    const val STATE_BUFFERING = 6

    fun actionFor(command: WatchMusicCommand, playing: Boolean): TransportAction = when (command) {
        WatchMusicCommand.TogglePlayPause -> if (playing) TransportAction.Pause else TransportAction.Play
        WatchMusicCommand.Play -> TransportAction.Play
        WatchMusicCommand.Pause -> TransportAction.Pause
        WatchMusicCommand.Next -> TransportAction.Next
        WatchMusicCommand.Previous -> TransportAction.Previous
        WatchMusicCommand.VolumeUp -> TransportAction.VolumeUp
        WatchMusicCommand.VolumeDown -> TransportAction.VolumeDown
        WatchMusicCommand.SkipForward -> TransportAction.SeekForward
        WatchMusicCommand.SkipBackward -> TransportAction.SeekBackward
    }

    fun isPlaying(playbackState: Int): Boolean = playbackState == STATE_PLAYING || playbackState == STATE_BUFFERING

    /**
     * The state for the watch. A player reports the position it had at `updateTimeMs`; while it
     * plays, the position now is that plus the time since, times the speed.
     */
    fun playback(
        playbackState: Int,
        speed: Float,
        positionMs: Long,
        updateTimeMs: Long,
        nowMs: Long,
    ): MusicPlayback {
        val playing = isPlaying(playbackState)
        val rate = if (speed > 0f && speed.isFinite()) (speed * 100).toInt().coerceIn(1, 400) else 100
        val known = positionMs >= 0 // PlaybackState.PLAYBACK_POSITION_UNKNOWN is -1
        val moved = if (playing && known && updateTimeMs > 0 && nowMs >= updateTimeMs) {
            ((nowMs - updateTimeMs) * rate / 100)
        } else {
            0L
        }
        val position = if (known) positionMs + moved else 0L
        return MusicPlayback(playing, rate, position.coerceAtLeast(0) / 1000)
    }

    /** Where a skip button lands: [deltaMs] away, never before the start or past the end (when the length is known). */
    fun seekTarget(positionMs: Long, deltaMs: Long, durationMs: Long): Long {
        val target = (positionMs + deltaMs).coerceAtLeast(0)
        return if (durationMs > 0) target.coerceAtMost(durationMs) else target
    }

    fun volumePercent(current: Int, max: Int): Int =
        if (max <= 0) 0 else (current.coerceIn(0, max) * 100 / max)

    /**
     * Which session the watch follows: the first that plays, else the first with a track, else
     * the first. Android lists sessions most recently active first. Null for none.
     */
    fun pickSession(sessions: List<SessionInfo>): Int? {
        if (sessions.isEmpty()) return null
        sessions.indexOfFirst { it.playing }.takeIf { it >= 0 }?.let { return it }
        sessions.indexOfFirst { it.hasMetadata }.takeIf { it >= 0 }?.let { return it }
        return 0
    }
}
