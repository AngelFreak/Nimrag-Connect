package dev.nimrag.app.engine

import android.app.AlarmManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.os.SystemClock
import dev.nimrag.app.NimragApplication

/**
 * The wake-up of the automatic sync. A `Handler` delay counts uptime, which stops while the phone
 * sleeps deeply, so an interval of an hour could stretch to many. An inexact alarm that is allowed
 * while idle (`setAndAllowWhileIdle`, no exact-alarm permission) counts elapsed real time and wakes
 * the phone; [AutoSyncReceiver] hands it to the link. One alarm at most: arming replaces the
 * pending one. It is a `PendingIntent`, so it also reaches a process that was killed meanwhile.
 */
class AutoSyncAlarm(private val context: Context) {
    private val alarms = context.getSystemService(AlarmManager::class.java)

    /** Wakes the app in about [delayMs]. False when the system refused to schedule it. */
    fun arm(delayMs: Long): Boolean = try {
        alarms.setAndAllowWhileIdle(AlarmManager.ELAPSED_REALTIME_WAKEUP, SystemClock.elapsedRealtime() + delayMs, pending())
        true
    } catch (_: RuntimeException) {
        false
    }

    fun cancel() {
        alarms.cancel(pending())
    }

    private fun pending(): PendingIntent = PendingIntent.getBroadcast(
        context,
        0,
        Intent(context, AutoSyncReceiver::class.java).setAction(ACTION),
        PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
    )

    companion object {
        const val ACTION = "dev.nimrag.app.AUTO_SYNC"
    }
}

/**
 * Receives the [AutoSyncAlarm]. The work runs on the engine's HandlerThread; the broadcast stays
 * open (`goAsync`, so the phone keeps awake) until the link has looked at it.
 */
class AutoSyncReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != AutoSyncAlarm.ACTION) return
        val engine = (context.applicationContext as? NimragApplication)?.engine ?: return
        val pending = goAsync()
        engine.onAutoSyncAlarm { pending.finish() }
    }
}
