package dev.nimrag.app.engine

import android.companion.AssociationInfo
import android.companion.CompanionDeviceService
import android.os.Build
import androidx.annotation.RequiresApi
import dev.nimrag.app.NimragApplication

/**
 * The system binds this service when the associated watch comes into range (companion
 * device presence). Being bound lets the app start [WatchService] from the background, which
 * is what brings the link back after Android killed the process.
 */
class WatchPresenceService : CompanionDeviceService() {
    // API 31 and 32.
    @Deprecated("Replaced by the AssociationInfo overload, from API 33")
    @Suppress("OVERRIDE_DEPRECATION")
    override fun onDeviceAppeared(address: String) {
        appeared()
    }

    // API 33 and later; deliberately does not call super, which would call the overload above.
    @RequiresApi(Build.VERSION_CODES.TIRAMISU)
    @Suppress("OVERRIDE_DEPRECATION")
    override fun onDeviceAppeared(associationInfo: AssociationInfo) {
        appeared()
    }

    private fun appeared() {
        (application as? NimragApplication)?.engine?.startServiceIfPaired(StartSource.CompanionPresence)
    }
}
