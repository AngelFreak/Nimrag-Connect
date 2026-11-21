package dev.nimrag.app.engine.phone

import android.content.ComponentName
import android.media.AudioManager
import android.media.MediaMetadata
import android.media.session.MediaController
import android.media.session.MediaSessionManager
import android.media.session.PlaybackState
import android.os.SystemClock
import android.view.KeyEvent
import dev.nimrag.app.engine.Hub
import dev.nimrag.app.engine.phone.logic.MusicPlayback
import dev.nimrag.app.engine.phone.logic.MusicRules
import dev.nimrag.app.engine.phone.logic.MusicTrack
import dev.nimrag.app.engine.phone.logic.SessionInfo
import dev.nimrag.app.engine.phone.logic.TransportAction
import dev.nimrag.app.engine.phone.logic.WatchMusicCommand
import uniffi.nimrag_ffi.Input
import uniffi.nimrag_ffi.MusicInfo
import uniffi.nimrag_ffi.MusicState

/**
 * Music control: follows the phone's active media session, tells the watch what plays and how
 * (title, artist, state, position, volume), and turns the watch's buttons into transport
 * controls. Needs "Notification access": Android only shows other apps' media sessions to an
 * enabled notification listener ([NimragNotificationListener]).
 *
 * Runs on the engine's HandlerThread (the callbacks are registered with its handler). The
 * track is never logged. Thin on purpose: the choices (which session, what a button does, the
 * position arithmetic) are in `MusicRules` and unit-tested.
 */
class MusicBridge(private val hub: Hub, private val send: (Input) -> Unit) {
    private val ctx = hub.app
    private val handler = hub.handler
    private val manager = ctx.getSystemService(MediaSessionManager::class.java)
    private val audio = ctx.getSystemService(AudioManager::class.java)
    private val listener = ComponentName(ctx, NimragNotificationListener::class.java)

    private var running = false
    private var controller: MediaController? = null
    private var lastTrack: MusicTrack? = null

    private val sessionsChanged = MediaSessionManager.OnActiveSessionsChangedListener { list ->
        if (running) follow(list.orEmpty())
    }

    private val callback = object : MediaController.Callback() {
        override fun onMetadataChanged(metadata: MediaMetadata?) = pushTrack(force = false)
        override fun onPlaybackStateChanged(state: PlaybackState?) = pushState()
        override fun onAudioInfoChanged(info: MediaController.PlaybackInfo) = pushVolume()
        override fun onSessionDestroyed() {
            if (running) follow(activeSessions())
        }
    }

    fun start() {
        if (running || manager == null) return
        try {
            manager.addOnActiveSessionsChangedListener(sessionsChanged, listener, handler)
            running = true
            follow(manager.getActiveSessions(listener))
            hub.log.info("Music control on: following the phone's player")
        } catch (_: SecurityException) {
            hub.log.warn("Music control needs notification access")
        }
    }

    fun stop() {
        if (!running) return
        running = false
        try {
            manager?.removeOnActiveSessionsChangedListener(sessionsChanged)
        } catch (_: Exception) {
            // Already gone.
        }
        controller?.unregisterCallback(callback)
        controller = null
        lastTrack = null
    }

    private fun activeSessions(): List<MediaController> = try {
        manager?.getActiveSessions(listener).orEmpty()
    } catch (_: SecurityException) {
        emptyList()
    }

    private fun follow(sessions: List<MediaController>) {
        val infos = sessions.map { SessionInfo(MusicRules.isPlaying(it.playbackState?.state ?: 0), it.metadata != null) }
        val pick = MusicRules.pickSession(infos)?.let { sessions[it] }
        if (pick?.sessionToken == controller?.sessionToken && controller != null) return
        controller?.unregisterCallback(callback)
        controller = pick
        pick?.registerCallback(callback, handler)
        pushAll()
    }

    /** The watch asked for everything again, or a new session was picked. */
    fun pushAll() {
        if (!running) return
        pushTrack(force = true)
        pushState()
        pushVolume()
    }

    private fun pushTrack(force: Boolean) {
        if (!running) return
        val md = controller?.metadata
        val track = if (md == null) {
            MusicTrack("", "", "", 0)
        } else {
            MusicTrack(
                artist = md.getString(MediaMetadata.METADATA_KEY_ARTIST) ?: md.getString(MediaMetadata.METADATA_KEY_ALBUM_ARTIST).orEmpty(),
                album = md.getString(MediaMetadata.METADATA_KEY_ALBUM).orEmpty(),
                title = md.getString(MediaMetadata.METADATA_KEY_TITLE).orEmpty(),
                durationS = md.getLong(MediaMetadata.METADATA_KEY_DURATION).coerceAtLeast(0) / 1000,
            )
        }
        if (!force && track == lastTrack) return
        lastTrack = track
        send(Input.MusicInfo(MusicInfo(track.artist, track.album, track.title, track.durationS.toUInt())))
    }

    private fun pushState() {
        if (!running) return
        val s = controller?.playbackState
        val p = if (s == null) {
            MusicPlayback(playing = false, ratePercent = 100, positionS = 0)
        } else {
            MusicRules.playback(s.state, s.playbackSpeed, s.position, s.lastPositionUpdateTime, SystemClock.elapsedRealtime())
        }
        send(Input.MusicState(MusicState(p.playing, p.ratePercent.toUShort(), p.positionS.toUInt())))
    }

    private fun pushVolume() {
        if (!running) return
        val a = audio ?: return
        val percent = MusicRules.volumePercent(a.getStreamVolume(AudioManager.STREAM_MUSIC), a.getStreamMaxVolume(AudioManager.STREAM_MUSIC))
        send(Input.MusicVolume(percent.toUByte()))
    }

    /** A button on the watch. */
    fun onCommand(command: WatchMusicCommand) {
        if (!running) return
        val c = controller
        val playing = MusicRules.isPlaying(c?.playbackState?.state ?: 0)
        val action = MusicRules.actionFor(command, playing)
        try {
            if (c == null) fallback(action) else transport(c, action)
        } catch (_: Exception) {
            hub.log.warn("A music button from the watch could not be carried out")
        }
    }

    private fun transport(c: MediaController, action: TransportAction) {
        val t = c.transportControls
        when (action) {
            TransportAction.Play -> t.play()
            TransportAction.Pause -> t.pause()
            TransportAction.Next -> t.skipToNext()
            TransportAction.Previous -> t.skipToPrevious()
            TransportAction.VolumeUp -> c.adjustVolume(AudioManager.ADJUST_RAISE, 0)
            TransportAction.VolumeDown -> c.adjustVolume(AudioManager.ADJUST_LOWER, 0)
            TransportAction.SeekForward, TransportAction.SeekBackward -> {
                val state = c.playbackState ?: return
                val now = MusicRules.playback(state.state, state.playbackSpeed, state.position, state.lastPositionUpdateTime, SystemClock.elapsedRealtime())
                val duration = c.metadata?.getLong(MediaMetadata.METADATA_KEY_DURATION) ?: 0L
                val delta = if (action == TransportAction.SeekForward) MusicRules.SEEK_STEP_MS else -MusicRules.SEEK_STEP_MS
                t.seekTo(MusicRules.seekTarget(now.positionS * 1000, delta, duration))
            }
        }
    }

    /** No player is active: the media keys can still wake the last one. */
    private fun fallback(action: TransportAction) {
        val a = audio ?: return
        when (action) {
            TransportAction.Play -> key(a, KeyEvent.KEYCODE_MEDIA_PLAY)
            TransportAction.Pause -> key(a, KeyEvent.KEYCODE_MEDIA_PAUSE)
            TransportAction.Next -> key(a, KeyEvent.KEYCODE_MEDIA_NEXT)
            TransportAction.Previous -> key(a, KeyEvent.KEYCODE_MEDIA_PREVIOUS)
            TransportAction.VolumeUp -> a.adjustStreamVolume(AudioManager.STREAM_MUSIC, AudioManager.ADJUST_RAISE, 0)
            TransportAction.VolumeDown -> a.adjustStreamVolume(AudioManager.STREAM_MUSIC, AudioManager.ADJUST_LOWER, 0)
            TransportAction.SeekForward, TransportAction.SeekBackward -> Unit
        }
        if (action == TransportAction.VolumeUp || action == TransportAction.VolumeDown) pushVolume()
    }

    private fun key(a: AudioManager, code: Int) {
        a.dispatchMediaKeyEvent(KeyEvent(KeyEvent.ACTION_DOWN, code))
        a.dispatchMediaKeyEvent(KeyEvent(KeyEvent.ACTION_UP, code))
    }
}
