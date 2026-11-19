package dev.nimrag.app.engine.ble

import android.annotation.SuppressLint
import android.bluetooth.le.ScanFilter
import android.companion.AssociationInfo
import android.companion.AssociationRequest
import android.companion.BluetoothLeDeviceFilter
import android.companion.CompanionDeviceManager
import android.content.IntentSender
import android.content.pm.PackageManager
import android.os.Build
import dev.nimrag.app.engine.Hub
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow

/**
 * CompanionDeviceManager association with the watch. Why: since Android 12 an app that was
 * killed may not start a foreground service from the background. A companion association
 * lets the system tell [dev.nimrag.app.engine.WatchPresenceService] when the watch is in
 * range, and that service may start [dev.nimrag.app.engine.WatchService]. Without an
 * association (a watch paired before this existed) reconnecting after a process death
 * needs the user to open the app; the engine posts a notice for that.
 *
 * The system shows its own confirmation dialog. Its [IntentSender] is published in
 * [chooser] for `MainActivity`, which launches it and reports back through
 * `WatchEngine.onAssociationResult`. All methods except [chooser] run on the engine's
 * HandlerThread.
 */
class CompanionAssociation(private val hub: Hub) {
    private val manager: CompanionDeviceManager? =
        if (hub.app.packageManager.hasSystemFeature(PackageManager.FEATURE_COMPANION_DEVICE_SETUP)) {
            hub.app.getSystemService(CompanionDeviceManager::class.java)
        } else {
            null
        }

    private val chooserFlow = MutableStateFlow<IntentSender?>(null)

    /** The system's confirmation dialog to launch, or null. The UI clears it with [chooserLaunched]. */
    val chooser: StateFlow<IntentSender?> get() = chooserFlow

    private var onDone: ((Outcome) -> Unit)? = null

    enum class Outcome {
        /** The association exists. */
        Created,

        /** Not available or failed: pair without it. */
        Unavailable,

        /** The user declined the system dialog. */
        Declined,
    }

    @Suppress("DEPRECATION") // getAssociations(): the only listing that exists on API 31 and 32
    fun isAssociated(address: String): Boolean = try {
        manager?.associations?.any { it.equals(address, ignoreCase = true) } == true
    } catch (_: Exception) {
        false
    }

    /** Starts the association; [done] is called once, on the HandlerThread. */
    @SuppressLint("MissingPermission")
    fun associate(address: String, done: (Outcome) -> Unit) {
        val cdm = manager
        if (cdm == null) {
            done(Outcome.Unavailable)
            return
        }
        if (isAssociated(address)) {
            done(Outcome.Created)
            return
        }
        onDone = done
        val request = try {
            val filter = BluetoothLeDeviceFilter.Builder()
                .setScanFilter(ScanFilter.Builder().setDeviceAddress(address).build())
                .build()
            AssociationRequest.Builder().addDeviceFilter(filter).setSingleDevice(true).build()
        } catch (_: Exception) {
            finish(Outcome.Unavailable)
            return
        }
        val callback = object : CompanionDeviceManager.Callback() {
            // API 33 and later.
            override fun onAssociationPending(intentSender: IntentSender) {
                hub.handler.post { publish(intentSender) }
            }

            // API 31 and 32 (also called by the default onAssociationPending).
            @Deprecated("Replaced by onAssociationPending, from API 33")
            @Suppress("OVERRIDE_DEPRECATION")
            override fun onDeviceFound(intentSender: IntentSender) {
                hub.handler.post { publish(intentSender) }
            }

            override fun onAssociationCreated(associationInfo: AssociationInfo) {
                hub.handler.post { finish(Outcome.Created) }
            }

            override fun onFailure(error: CharSequence?) {
                hub.handler.post {
                    hub.log.warn("Companion association failed")
                    finish(Outcome.Unavailable)
                }
            }
        }
        try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                cdm.associate(request, { r -> hub.handler.post(r) }, callback)
            } else {
                cdm.associate(request, callback, hub.handler)
            }
        } catch (e: Exception) {
            hub.log.warn("Companion association could not start (${e.javaClass.simpleName})")
            finish(Outcome.Unavailable)
        }
    }

    /** `MainActivity` launched the dialog. */
    fun chooserLaunched() {
        chooserFlow.value = null
    }

    /** The dialog's activity result: OK created the association on the system side. */
    fun onChooserResult(resultOk: Boolean) {
        if (onDone == null) return
        finish(if (resultOk) Outcome.Created else Outcome.Declined)
    }

    /** Gives up waiting (pairing timed out or was cancelled). */
    fun cancel() {
        onDone = null
        chooserFlow.value = null
    }

    @Suppress("DEPRECATION") // startObservingDevicePresence(String): the form that works on API 31 to 37
    fun startObserving(address: String): Boolean {
        val cdm = manager ?: return false
        if (!isAssociated(address)) return false
        return try {
            cdm.startObservingDevicePresence(address)
            true
        } catch (e: Exception) {
            hub.log.warn("Companion presence observing failed (${e.javaClass.simpleName})")
            false
        }
    }

    @Suppress("DEPRECATION")
    fun disassociate(address: String) {
        val cdm = manager ?: return
        if (!isAssociated(address)) return
        try {
            try {
                cdm.stopObservingDevicePresence(address)
            } catch (_: Exception) {
                // Not observed: nothing to stop.
            }
            cdm.disassociate(address)
        } catch (e: Exception) {
            hub.log.warn("Companion association could not be removed (${e.javaClass.simpleName})")
        }
    }

    private fun publish(sender: IntentSender) {
        if (onDone == null || chooserFlow.value != null) return
        chooserFlow.value = sender
    }

    private fun finish(outcome: Outcome) {
        val done = onDone ?: return
        onDone = null
        chooserFlow.value = null
        done(outcome)
    }
}
