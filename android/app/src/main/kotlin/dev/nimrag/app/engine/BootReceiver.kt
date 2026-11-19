package dev.nimrag.app.engine

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import dev.nimrag.app.NimragApplication

/** Starts the watch service after a reboot or an app update when a watch is remembered. */
class BootReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != Intent.ACTION_BOOT_COMPLETED && intent.action != Intent.ACTION_MY_PACKAGE_REPLACED) return
        (context.applicationContext as? NimragApplication)?.engine?.startServiceIfPaired(StartSource.Boot)
    }
}
