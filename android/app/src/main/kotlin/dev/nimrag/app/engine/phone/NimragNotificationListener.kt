package dev.nimrag.app.engine.phone

import android.app.Notification
import android.content.pm.ApplicationInfo
import android.os.Build
import android.service.notification.NotificationListenerService
import android.service.notification.StatusBarNotification
import dev.nimrag.app.NimragApplication
import dev.nimrag.app.engine.phone.logic.NotificationSnapshot

/**
 * The system's way to read the phone's notifications. The owner gives it in system settings
 * ("Notification access"); until then, and whenever the notification duty is off, nothing here
 * reads a notification: [PhoneBridge.forwarding] is checked before any text is touched. What is
 * read is handed to the bridge in memory and never logged or stored.
 *
 * Music control reuses the same access: the media sessions of other apps are only visible to an
 * enabled listener ([MusicBridge]).
 */
class NimragNotificationListener : NotificationListenerService() {
    private val bridge: PhoneBridge? get() = (application as? NimragApplication)?.engine?.phoneBridge

    override fun onListenerConnected() {
        instance = this
        // The owner may just have granted the access: tell the engine so the duties start.
        (application as? NimragApplication)?.engine?.onNotificationAccessChanged()
    }

    override fun onListenerDisconnected() {
        if (instance === this) instance = null
        (application as? NimragApplication)?.engine?.onNotificationAccessChanged()
    }

    override fun onNotificationPosted(sbn: StatusBarNotification?) {
        val bridge = bridge ?: return
        if (sbn == null || !bridge.forwarding) return
        val snapshot = try {
            snapshotOf(sbn)
        } catch (_: Exception) {
            null
        } ?: return
        bridge.onPosted(snapshot)
    }

    override fun onNotificationRemoved(sbn: StatusBarNotification?) {
        val bridge = bridge ?: return
        if (sbn == null || !bridge.forwarding) return
        bridge.onRemoved(sbn.key)
    }

    private fun snapshotOf(sbn: StatusBarNotification): NotificationSnapshot? {
        val n = sbn.notification ?: return null
        val extras = n.extras
        fun text(key: String): String = extras?.getCharSequence(key)?.toString().orEmpty()
        return NotificationSnapshot(
            key = sbn.key,
            packageName = sbn.packageName,
            appLabel = labelOf(sbn.packageName, n),
            category = n.category,
            title = text(Notification.EXTRA_TITLE),
            text = text(Notification.EXTRA_TEXT),
            bigText = text(Notification.EXTRA_BIG_TEXT),
            subText = text(Notification.EXTRA_SUB_TEXT),
            postTimeMs = sbn.postTime,
            ongoing = (n.flags and Notification.FLAG_ONGOING_EVENT) != 0,
            groupSummary = (n.flags and Notification.FLAG_GROUP_SUMMARY) != 0,
        )
    }

    /**
     * The app's name. Other apps are only visible to us through the notification's own copy of
     * their application info; when that is missing the package name stands in.
     */
    private fun labelOf(packageName: String, n: Notification): String {
        val info: ApplicationInfo? = try {
            val extras = n.extras
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                extras?.getParcelable(EXTRA_APP_INFO, ApplicationInfo::class.java)
            } else {
                @Suppress("DEPRECATION")
                extras?.getParcelable(EXTRA_APP_INFO)
            }
        } catch (_: Exception) {
            null
        }
        return try {
            (info ?: packageManager.getApplicationInfo(packageName, 0)).loadLabel(packageManager).toString()
        } catch (_: Exception) {
            packageName
        }
    }

    companion object {
        /** `Notification.EXTRA_BUILDER_APPLICATION_INFO` (hidden API constant, a stable string). */
        private const val EXTRA_APP_INFO = "android.appInfo"

        @Volatile
        private var instance: NimragNotificationListener? = null

        /** Dismiss a notification on the phone ("Dismiss" on the watch). Quietly does nothing without the listener. */
        fun cancel(key: String) {
            try {
                instance?.cancelNotification(key)
            } catch (_: Exception) {
                // The listener was disconnected meanwhile.
            }
        }
    }
}
