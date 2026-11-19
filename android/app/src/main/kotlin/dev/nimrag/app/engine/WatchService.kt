package dev.nimrag.app.engine

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.IBinder
import dev.nimrag.app.NimragApplication
import dev.nimrag.app.R
import dev.nimrag.app.model.AppState
import dev.nimrag.app.model.LinkState
import dev.nimrag.app.model.WatchState
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.launch

/**
 * Foreground service (type `connectedDevice`) that keeps the process alive while a watch is
 * paired. It shows an ongoing notification with the link state and starts and stops the
 * engine's link; the BLE work itself runs on the engine's HandlerThread. `START_STICKY`:
 * Android restarts it after the process was killed.
 */
class WatchService : Service() {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private var collector: Job? = null

    override fun onCreate() {
        super.onCreate()
        val channel = NotificationChannel(
            CHANNEL_ID,
            getString(R.string.engine_channel_name),
            NotificationManager.IMPORTANCE_LOW,
        ).apply {
            description = getString(R.string.engine_channel_description)
            setShowBadge(false)
        }
        getSystemService(NotificationManager::class.java).createNotificationChannel(channel)
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val engine = (application as NimragApplication).engine
        try {
            startForeground(
                NOTIFICATION_ID,
                notificationFor(engine.state.value),
                ServiceInfo.FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE,
            )
        } catch (_: Exception) {
            // Not allowed to run in the foreground now (permission or background start rules).
            stopSelf()
            return START_NOT_STICKY
        }
        if (collector == null) {
            collector = scope.launch {
                engine.state.map { statusText(it) }.distinctUntilChanged().collect {
                    val state = engine.state.value
                    getSystemService(NotificationManager::class.java).notify(NOTIFICATION_ID, notificationFor(state))
                }
            }
        }
        ReopenNotice.cancel(this)
        engine.onServiceStarted { keep -> if (!keep) stopSelf() }
        return START_STICKY
    }

    override fun onDestroy() {
        collector = null
        scope.cancel()
        (application as? NimragApplication)?.engine?.onServiceStopped()
        super.onDestroy()
    }

    override fun onBind(intent: Intent?): IBinder? = null

    private fun notificationFor(state: AppState): Notification {
        val launch = packageManager.getLaunchIntentForPackage(packageName)
        val tap = launch?.let {
            PendingIntent.getActivity(this, 0, it, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
        }
        return Notification.Builder(this, CHANNEL_ID)
            .setSmallIcon(R.drawable.ic_stat_watch)
            .setContentTitle(getString(R.string.engine_notif_title))
            .setContentText(statusText(state))
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .setCategory(Notification.CATEGORY_SERVICE)
            .setForegroundServiceBehavior(Notification.FOREGROUND_SERVICE_IMMEDIATE)
            .setContentIntent(tap)
            .build()
    }

    private fun statusText(state: AppState): String {
        val watch = state.watch
        return when {
            watch is WatchState.Pairing -> getString(R.string.engine_notif_pairing, watch.name)
            watch !is WatchState.Paired -> getString(R.string.engine_notif_offline)
            !state.permissions.bluetooth -> getString(R.string.engine_notif_permission)
            !state.permissions.bluetoothEnabled -> getString(R.string.engine_notif_bluetooth_off)
            else -> when (watch.link) {
                LinkState.Offline -> getString(R.string.engine_notif_offline)
                LinkState.Waiting -> getString(R.string.engine_notif_waiting, watch.model)
                LinkState.Connecting -> getString(R.string.engine_notif_connecting, watch.model)
                LinkState.Ready -> readyText(state, watch)
                LinkState.BondLost -> getString(R.string.engine_notif_bond_lost)
            }
        }
    }

    private fun readyText(state: AppState, watch: WatchState.Paired): String {
        val sync = state.sync
        if (sync.running) {
            val total = sync.filesInCycle
            return if (total != null) {
                getString(R.string.engine_notif_syncing_files, watch.model, sync.filesDone, total)
            } else {
                getString(R.string.engine_notif_syncing, watch.model)
            }
        }
        val battery = watch.batteryPercent
        return if (battery != null) {
            getString(R.string.engine_notif_ready_battery, watch.model, battery)
        } else {
            getString(R.string.engine_notif_ready, watch.model)
        }
    }

    companion object {
        private const val CHANNEL_ID = "watch_link"
        private const val NOTIFICATION_ID = 1

        /** Starts the service in the foreground; false if Android refuses right now. */
        fun start(context: Context): Boolean = try {
            context.startForegroundService(Intent(context, WatchService::class.java))
            true
        } catch (_: Exception) {
            false
        }

        fun stop(context: Context) {
            context.stopService(Intent(context, WatchService::class.java))
        }
    }
}
