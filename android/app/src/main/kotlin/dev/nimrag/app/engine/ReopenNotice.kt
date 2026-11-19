package dev.nimrag.app.engine

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import dev.nimrag.app.R

/**
 * A normal (not foreground) notification for the case Android refuses to start the watch
 * service from the background: tapping it opens the app, which can start the service.
 * Never fails: without the notification permission nothing is shown.
 */
object ReopenNotice {
    private const val CHANNEL_ID = "reopen"
    private const val NOTIFICATION_ID = 2

    fun post(context: Context) {
        try {
            val manager = context.getSystemService(NotificationManager::class.java)
            manager.createNotificationChannel(
                NotificationChannel(
                    CHANNEL_ID,
                    context.getString(R.string.engine_channel_reopen_name),
                    NotificationManager.IMPORTANCE_DEFAULT,
                ).apply { description = context.getString(R.string.engine_channel_reopen_description) },
            )
            val launch = context.packageManager.getLaunchIntentForPackage(context.packageName)
            val tap = launch?.let {
                PendingIntent.getActivity(context, 1, it, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
            }
            manager.notify(
                NOTIFICATION_ID,
                Notification.Builder(context, CHANNEL_ID)
                    .setSmallIcon(R.drawable.ic_stat_watch)
                    .setContentTitle(context.getString(R.string.engine_notif_title))
                    .setContentText(context.getString(R.string.engine_notif_reopen))
                    .setAutoCancel(true)
                    .setContentIntent(tap)
                    .build(),
            )
        } catch (_: Exception) {
            // A missing notification must not take anything down.
        }
    }

    fun cancel(context: Context) {
        try {
            context.getSystemService(NotificationManager::class.java).cancel(NOTIFICATION_ID)
        } catch (_: Exception) {
            // Nothing to cancel.
        }
    }
}
