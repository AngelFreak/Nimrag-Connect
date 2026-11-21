package dev.nimrag.app.engine.phone

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.graphics.drawable.Icon
import android.media.AudioAttributes
import android.media.AudioManager
import android.media.Ringtone
import android.media.RingtoneManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.os.VibrationAttributes
import android.os.VibrationEffect
import android.os.VibratorManager
import dev.nimrag.app.R
import dev.nimrag.app.engine.phone.logic.FindPhoneRules
import dev.nimrag.app.engine.phone.logic.FindPhoneSession

/**
 * Find my phone: a loud ring on the alarm stream (even when the ringer is silent), a repeating
 * vibration and a heads-up notification that also asks for the full screen, with one big
 * "Found it" button. It stops on "Found it", when the watch cancels, or by itself after the time
 * the watch asked for (10 s to 2 min). The alarm volume is raised for the ring and put back.
 *
 * Any thread may call [start] and [stop]; the work runs on the main thread. Nothing here can
 * throw out of it: a missing sound or notification must never take the app down.
 */
class FindPhoneAlarm(private val ctx: Context, private val onRinging: (Boolean) -> Unit) {
    private val main = Handler(Looper.getMainLooper())
    private val session = FindPhoneSession()
    private var ringtone: Ringtone? = null
    private var savedAlarmVolume: Int? = null
    private val timeout = Runnable { stopOnMain() }

    fun start(seconds: Int) {
        main.post { startOnMain(seconds) }
    }

    fun stop() {
        main.post { stopOnMain() }
    }

    private fun startOnMain(seconds: Int) {
        val first = session.start(SystemClock.elapsedRealtime(), seconds)
        main.removeCallbacks(timeout)
        main.postDelayed(timeout, seconds * 1000L)
        if (!first) return // already ringing: only the end moved
        onRinging(true)
        raiseVolume()
        playSound()
        vibrate()
        showNotification(seconds)
    }

    private fun stopOnMain() {
        if (!session.stop()) return
        main.removeCallbacks(timeout)
        try {
            ringtone?.stop()
        } catch (_: Exception) {
            // Already stopped.
        }
        ringtone = null
        try {
            ctx.getSystemService(VibratorManager::class.java)?.defaultVibrator?.cancel()
        } catch (_: Exception) {
            // No vibrator.
        }
        restoreVolume()
        try {
            ctx.getSystemService(NotificationManager::class.java)?.cancel(NOTIFICATION_ID)
        } catch (_: Exception) {
            // Nothing to cancel.
        }
        onRinging(false)
    }

    private fun raiseVolume() {
        try {
            val audio = ctx.getSystemService(AudioManager::class.java) ?: return
            savedAlarmVolume = audio.getStreamVolume(AudioManager.STREAM_ALARM)
            audio.setStreamVolume(AudioManager.STREAM_ALARM, audio.getStreamMaxVolume(AudioManager.STREAM_ALARM), 0)
        } catch (_: Exception) {
            // Do Not Disturb policy or no audio service: ring at the volume the phone has.
        }
    }

    private fun restoreVolume() {
        val saved = savedAlarmVolume ?: return
        savedAlarmVolume = null
        try {
            ctx.getSystemService(AudioManager::class.java)?.setStreamVolume(AudioManager.STREAM_ALARM, saved, 0)
        } catch (_: Exception) {
            // Leave the volume as it is.
        }
    }

    private fun playSound() {
        try {
            val uri = RingtoneManager.getActualDefaultRingtoneUri(ctx, RingtoneManager.TYPE_ALARM)
                ?: RingtoneManager.getDefaultUri(RingtoneManager.TYPE_ALARM)
                ?: RingtoneManager.getDefaultUri(RingtoneManager.TYPE_RINGTONE)
            val tone = RingtoneManager.getRingtone(ctx, uri) ?: return
            tone.audioAttributes = AudioAttributes.Builder()
                .setUsage(AudioAttributes.USAGE_ALARM)
                .setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION)
                .build()
            tone.isLooping = true
            tone.play()
            ringtone = tone
        } catch (_: Exception) {
            // No sound: the vibration and the notification still tell.
        }
    }

    @Suppress("DEPRECATION")
    private fun vibrate() {
        try {
            val vibrator = ctx.getSystemService(VibratorManager::class.java)?.defaultVibrator ?: return
            val effect = VibrationEffect.createWaveform(FindPhoneRules.VIBRATION_PATTERN, FindPhoneRules.VIBRATION_REPEAT_INDEX)
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                vibrator.vibrate(effect, VibrationAttributes.createForUsage(VibrationAttributes.USAGE_ALARM))
            } else {
                vibrator.vibrate(
                    effect,
                    AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_ALARM).build(),
                )
            }
        } catch (_: Exception) {
            // No vibrator.
        }
    }

    /** Heads-up with "Found it"; on a locked screen it asks for the full screen (see [FindPhoneActivity]). */
    private fun showNotification(seconds: Int) {
        try {
            val manager = ctx.getSystemService(NotificationManager::class.java) ?: return
            manager.createNotificationChannel(
                NotificationChannel(CHANNEL_ID, ctx.getString(R.string.find_phone_channel_name), NotificationManager.IMPORTANCE_HIGH).apply {
                    description = ctx.getString(R.string.find_phone_channel_description)
                    // The alarm plays the sound itself, on the alarm stream.
                    setSound(null, null)
                    enableVibration(false)
                    lockscreenVisibility = Notification.VISIBILITY_PUBLIC
                },
            )
            val flags = PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
            val stop = PendingIntent.getBroadcast(
                ctx,
                REQUEST_STOP,
                Intent(ctx, FindPhoneReceiver::class.java).setAction(FindPhoneReceiver.ACTION_STOP),
                flags,
            )
            val screen = PendingIntent.getActivity(
                ctx,
                REQUEST_SCREEN,
                Intent(ctx, FindPhoneActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
                flags,
            )
            val found = ctx.getString(R.string.find_phone_found)
            manager.notify(
                NOTIFICATION_ID,
                Notification.Builder(ctx, CHANNEL_ID)
                    .setSmallIcon(R.drawable.ic_stat_watch)
                    .setContentTitle(ctx.getString(R.string.find_phone_title))
                    .setContentText(ctx.getString(R.string.find_phone_text))
                    .setCategory(Notification.CATEGORY_ALARM)
                    .setOngoing(true)
                    .setTimeoutAfter(seconds * 1000L)
                    .setContentIntent(screen)
                    .setFullScreenIntent(screen, true)
                    .addAction(Notification.Action.Builder(Icon.createWithResource(ctx, R.drawable.ic_stat_watch), found, stop).build())
                    .build(),
            )
        } catch (_: Exception) {
            // Without the notification permission the phone still rings; the watch or the timeout ends it.
        }
    }

    private companion object {
        const val CHANNEL_ID = "find_phone"
        const val NOTIFICATION_ID = 3
        const val REQUEST_STOP = 3
        const val REQUEST_SCREEN = 4
    }
}
