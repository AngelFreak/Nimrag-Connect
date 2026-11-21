package dev.nimrag.app.engine.ble

import android.annotation.SuppressLint
import android.bluetooth.BluetoothAdapter
import android.bluetooth.BluetoothDevice
import android.bluetooth.BluetoothGatt
import android.bluetooth.BluetoothGattCallback
import android.bluetooth.BluetoothGattCharacteristic
import android.bluetooth.BluetoothGattDescriptor
import android.bluetooth.BluetoothManager
import android.bluetooth.BluetoothProfile
import android.bluetooth.BluetoothStatusCodes
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.os.Build
import android.os.SystemClock
import dev.nimrag.app.engine.AutoSyncAlarm
import dev.nimrag.app.engine.Hub
import dev.nimrag.app.engine.RememberedWatch
import dev.nimrag.app.engine.WatchService
import dev.nimrag.app.engine.logic.AlongsideRules
import dev.nimrag.app.engine.logic.Backoff
import dev.nimrag.app.engine.logic.CapabilityHex
import dev.nimrag.app.engine.logic.DstRule
import dev.nimrag.app.engine.logic.FileKind
import dev.nimrag.app.engine.logic.GattPair
import dev.nimrag.app.engine.logic.LinkRules
import dev.nimrag.app.engine.logic.LinkStep
import dev.nimrag.app.engine.logic.LinkSupervisor
import dev.nimrag.app.engine.logic.StallAction
import dev.nimrag.app.engine.logic.WriteQueue
import dev.nimrag.app.engine.phone.logic.DutyMask
import dev.nimrag.app.engine.phone.logic.PhoneDuties
import dev.nimrag.app.engine.store.CopiedRow
import dev.nimrag.app.model.GarminConnect
import dev.nimrag.app.model.GarminConnectMode
import dev.nimrag.app.model.LinkState
import dev.nimrag.app.model.SyncResult
import dev.nimrag.app.model.SyncState
import dev.nimrag.app.model.ScanState
import dev.nimrag.app.model.WatchState
import uniffi.nimrag_ffi.AnomalyKind
import uniffi.nimrag_ffi.BatteryReport
import uniffi.nimrag_ffi.ClientConfig
import uniffi.nimrag_ffi.CloseReason
import uniffi.nimrag_ffi.DirEntry
import uniffi.nimrag_ffi.DstPair
import uniffi.nimrag_ffi.Duties
import uniffi.nimrag_ffi.Event
import uniffi.nimrag_ffi.FileFailure
import uniffi.nimrag_ffi.FileKey
import uniffi.nimrag_ffi.Input
import uniffi.nimrag_ffi.NimragClient
import uniffi.nimrag_ffi.Now
import uniffi.nimrag_ffi.Step
import uniffi.nimrag_ffi.SyncTrigger
import java.time.Instant
import java.time.ZoneId
import java.util.UUID

/**
 * The link to the watch: bonding, the GATT connection, the write queue, the protocol client
 * ([NimragClient]), its single timer and the events it returns (state, store, log).
 *
 * Threading: every public method must be called on the engine's HandlerThread (`hub.handler`),
 * and so run all GATT callbacks, broadcast receivers, timers and calls into the client. That
 * is what keeps "handle, then perform the writes" in order (core's threading rule). The only
 * thing that may block is the file store write in [Event.FileDownloaded], on purpose: the
 * client must not hear `FileStored` before the bytes are durable.
 */
// Every BLE call runs inside guarded { } (or a try that catches SecurityException), which lint
// cannot see through: a lost permission is reported in AppState instead of crashing.
@SuppressLint("MissingPermission")
class WatchLink(private val hub: Hub, private val scanner: BleScanner) {
    private val ctx: Context = hub.app
    private val handler = hub.handler
    private val settings = hub.settings
    private val log = hub.log

    init {
        hub.phone.attach(this)
        hub.weather.attach(this)
        hub.workouts.attach(this)
    }

    // ---- connection state (HandlerThread only) ----
    private var running = false
    private var gatt: BluetoothGatt? = null
    private var client: NimragClient? = null
    private var writeChar: BluetoothGattCharacteristic? = null
    private var currentNotifyId: Int? = null
    private var attMtu = 23
    private var ready = false
    private var epoch = 0
    private var forcedNotifyId: Int? = null
    private var avoidNotifyId: Int? = null
    private var channelRetries = 0
    private var busyRetries = 0
    private var permissionWarned = false

    /** The bond with the remembered watch is gone: no reconnecting until forget + pair. */
    private var bondLost = false

    /** The adapter is switching off or off (set by the first of the two broadcasts). */
    private var adapterOff = false

    /** The "Garmin Connect may hold the watch" hint was logged since the last good link. */
    private var otherAppHinted = false

    /** The current connection carries `first_connect`; it is charged on `HandshakeDone`. */
    private var connectionHasFirst = false

    /** The mode the current client was built with: files are copied, never archived, in Alongside. */
    private var connectionMode = GarminConnectMode.Off

    /**
     * The phone features the current client was built with (and claimed in its capability mask).
     * A change of the switches or of the system access reconnects, because the watch only
     * learns what we do at the handshake ([onDutiesChanged]).
     */
    var connectionDuties = DutyMask()
        private set
    private val backoff = Backoff()
    private var pairing: Pairing? = null

    /** Per-step deadlines and the escalation after repeated stalls (pure logic, see [LinkSupervisor]). */
    private val supervisor = LinkSupervisor()

    /** The setup step the current connection waits in; null when there is no connection or setup is over. */
    private var step: LinkStep? = null

    /** The MTU exchange got no answer on this connection and was skipped. */
    private var mtuSkipped = false

    /** The current [gatt] reported CONNECTED (so a disconnect is worth waiting for before close). */
    private var gattConnected = false

    /** The old client, disconnecting: it is closed when the stack reports it or after [CLOSE_WAIT_MS]. */
    private var closing: BluetoothGatt? = null

    /** The address the system was last asked to report presence for in this process. */
    private var observing: String? = null

    /** [bondedBefore]: the phone already had a bond (Garmin Connect's, say) that we reuse and must not remove. */
    private data class Pairing(
        val address: String,
        val name: String,
        val model: String,
        val reason: String?,
        val bondedBefore: Boolean = false,
    )

    private val writes = WriteQueue(::sendOne)

    // ---- runnables (all posted on the handler) ----
    private val tick = Runnable { feed(Input.Tick) }
    private val reconnect = Runnable { connect() }
    private val pumpAgain = Runnable { handlePump(writes.pump()) }
    private val stepWatchdog = Runnable { onStepStalled() }
    private val closeFallback = Runnable { finishClose() }
    private val pairTimeout = Runnable { failPairing("Pairing timed out") }

    /** Wakes the link for the auto sync even while the phone sleeps (see [scheduleAutoSync]). */
    private val autoSyncAlarm = AutoSyncAlarm(ctx)

    /** Elapsed-realtime ms when a sync was last asked for; until the watch starts it, asking again only queues a second cycle. */
    private var syncAskedAtMs: Long? = null

    init {
        val filter = IntentFilter().apply {
            addAction(BluetoothAdapter.ACTION_STATE_CHANGED)
            addAction(BluetoothDevice.ACTION_BOND_STATE_CHANGED)
        }
        ctx.registerReceiver(systemReceiver(), filter, null, handler, Context.RECEIVER_EXPORTED)
    }

    // ---- public API ----

    /** The service started. Returns false when there is nothing to keep alive. */
    fun start(): Boolean {
        if (settings.watch == null && pairing == null) {
            running = false
            return false
        }
        running = true
        hub.refreshPermissions()
        val watch = settings.watch
        if (watch != null) {
            // Once per process: makes sure the system still reports the watch coming into range
            // (an app update may drop it). Repeating it on every service start adds nothing.
            if (observing != watch.address && hub.companion.startObserving(watch.address)) observing = watch.address
            // Another start while a connection exists, is closing or waits out a backoff changes
            // nothing: one owner of the GATT client, and the backoff stays in force.
            if (gatt == null && closing == null && !handler.hasCallbacks(reconnect)) connect()
        }
        scheduleAutoSync()
        return true
    }

    /**
     * The system says the watch is in range. While we only wait out a reconnect delay, connect
     * now instead; a connection or a close in progress is left alone.
     */
    fun onWatchPresent() {
        if (!running || gatt != null || bondLost) return
        if (handler.hasCallbacks(reconnect)) {
            handler.removeCallbacks(reconnect)
            connect()
        }
    }

    /** The service stopped: drop the link, keep the watch remembered. */
    fun stop() {
        running = false
        autoSyncAlarm.cancel()
        handler.removeCallbacks(reconnect)
        teardown()
        setLink(LinkState.Offline)
    }

    fun pair(address: String, name: String, model: String, unsupportedReason: String?) {
        if (settings.watch != null) {
            log.warn("A watch is already paired. Forget it first.")
            return
        }
        if (!hub.hasBluetoothPermissions()) {
            hub.refreshPermissions()
            log.warn("Pairing needs the Bluetooth permission")
            return
        }
        if (!hub.bluetoothEnabled()) {
            hub.refreshPermissions()
            log.warn("Bluetooth is off")
            return
        }
        scanner.stop()
        // Decided first, before anything can fail: a bond that already exists (usually Garmin
        // Connect's) is reused and must never be removed by us.
        val bondedBefore = try {
            adapter()?.getRemoteDevice(address)?.bondState == BluetoothDevice.BOND_BONDED
        } catch (_: Exception) {
            false
        }
        pairing = Pairing(address, name, model, unsupportedReason, bondedBefore)
        bondLost = false
        otherAppHinted = false
        hub.update { it.copy(watch = WatchState.Pairing(address, name), pairingError = null) }
        when (hub.garminConnect()) {
            GarminConnect.HasBluetooth -> if (AlongsideRules.hintsOtherApp(settings.garminConnectMode)) {
                log.warn("Garmin Connect has Bluetooth access. While it runs it holds the watch, and Nimrag cannot talk to it at the same time")
            } else {
                log.info("Garmin Connect has Bluetooth access. It shares the watch with Nimrag (Alongside mode)")
            }
            GarminConnect.BluetoothOff -> log.info("Garmin Connect's Bluetooth access is off")
            GarminConnect.Absent -> Unit
        }
        // Started now, while the UI is in the foreground; the service keeps the process alive.
        running = true
        WatchService.start(ctx)
        handler.postDelayed(pairTimeout, PAIR_TIMEOUT_MS)
        // First the companion association (the system's own dialog, shown by MainActivity),
        // then the bond. The association is optional: without it the pairing still works, but
        // the link will not come back by itself after Android killed the process.
        hub.companion.associate(address, ::onAssociation)
    }

    private fun onAssociation(outcome: CompanionAssociation.Outcome) {
        val pending = pairing ?: return
        when (outcome) {
            CompanionAssociation.Outcome.Created ->
                log.info("Pairing path: companion association in place, the system can restart the link")
            // The dialog also ends as "declined" when it found nothing within its 20 s search,
            // so neither outcome stops the pairing.
            CompanionAssociation.Outcome.Unavailable, CompanionAssociation.Outcome.Declined ->
                log.warn("Pairing path: no companion association, after a process death the app must be opened to reconnect")
        }
        startBonding(pending.address)
    }

    private fun startBonding(address: String) {
        try {
            val device = adapter()?.getRemoteDevice(address) ?: return failPairing("Bluetooth is not available")
            when (device.bondState) {
                BluetoothDevice.BOND_BONDED -> {
                    if (pairing?.bondedBefore == true) {
                        log.info("The phone is already paired with this watch at the Bluetooth level; reusing that pairing")
                    }
                    completeBond()
                }
                BluetoothDevice.BOND_BONDING -> log.info("Waiting for the pairing dialog")
                else -> {
                    // createBond(transport) is public API from 37 only.
                    val started = if (Build.VERSION.SDK_INT >= 37) {
                        device.createBond(BluetoothDevice.TRANSPORT_LE)
                    } else {
                        device.createBond()
                    }
                    if (!started) failPairing("Android did not start pairing") else log.info("Pairing started")
                }
            }
        } catch (_: SecurityException) {
            permissionLost()
            failPairing("The Bluetooth permission is missing")
        } catch (_: IllegalArgumentException) {
            failPairing("Invalid device address")
        }
    }

    /** Disconnect, remove the bond, clear remembered state. Stored files stay. */
    fun forget() {
        val watch = settings.watch
        handler.removeCallbacks(pairTimeout)
        pairing = null
        hub.companion.cancel()
        handler.removeCallbacks(reconnect)
        teardown()
        if (watch != null) {
            if (watch.ownBond) {
                removeBond(watch.address)
            } else {
                log.info("Kept the Bluetooth pairing: another app made it")
            }
            hub.companion.disassociate(watch.address)
        }
        backoff.reset()
        supervisor.healthy()
        observing = null
        forcedNotifyId = null
        avoidNotifyId = null
        channelRetries = 0
        bondLost = false
        settings.watch = null
        settings.firstConnect = false
        settings.firstConnectTries = 0
        settings.lastSync = null
        settings.lastConnectedAtMs = 0L
        // Its file keys mean nothing for another watch.
        runCatching { hub.db.clearCopied() }
        running = false
        autoSyncAlarm.cancel()
        hub.update { it.copy(watch = WatchState.NotPaired, sync = SyncState(), scan = ScanState()) }
        log.info("Watch forgotten")
        WatchService.stop(ctx)
    }

    /** The handshake is done and the link is usable. */
    fun isReady(): Boolean = ready && client != null

    /** A phone-feature input (notification, music, find my watch): sent when the link is ready, else dropped. */
    fun sendDuty(input: Input) {
        if (!ready || client == null) return
        feed(input)
    }

    /**
     * Hands a file to the client to be sent to the watch (`Input.UploadFile`; a workout is type 128,
     * sub type 5). False when the link is not ready; otherwise the outcome comes back as
     * `Event.UploadFinished`, exactly once, whatever happens.
     */
    fun sendFile(uploadId: ULong, dataType: UByte, subType: UByte, data: ByteArray): Boolean {
        if (!ready || client == null) return false
        feed(Input.UploadFile(uploadId, dataType, subType, data))
        return true
    }

    /**
     * The phone-feature switches or the system's notification access changed. The watch learns
     * what we claim at the handshake, so a connected watch is reconnected once when what is
     * effective now differs from what this connection was built with.
     */
    fun onDutiesChanged() {
        val want = PhoneDuties.effective(settings.phoneFeatures(), hub.phoneAccess())
        hub.phone.onDutiesChanged(want)
        hub.weather.onDutiesChanged(want)
        if (client == null || want == connectionDuties) return
        log.info("Phone features changed, reconnecting to apply them")
        teardown()
        scheduleReconnect(MODE_RECONNECT_MS)
    }

    fun syncNow() {
        if (!ready) {
            log.warn("Sync now: the watch is not connected")
            return
        }
        log.info("Sync requested")
        requestBatteryAndSync()
    }

    /**
     * The Garmin Connect mode was saved ([mode] is the new one). The client carries the mode it
     * was built with, so a connected watch is reconnected to apply it at once.
     */
    fun onGarminConnectModeChanged(mode: GarminConnectMode) {
        otherAppHinted = false
        if (client == null) {
            log.info("Garmin Connect mode: ${mode.name}, used from the next connection")
        } else {
            log.info("Garmin Connect mode: ${mode.name}, reconnecting to apply it")
            teardown()
            scheduleReconnect(MODE_RECONNECT_MS)
        }
        refreshLeftForGarminConnect()
    }

    /** The auto-sync interval changed: the alarm is set again (or gone for Off), and a sync that is now due starts. */
    fun onAutoSyncChanged() {
        scheduleAutoSync()
        maybeAutoSync()
    }

    /** The auto-sync alarm went off (while the phone slept, perhaps): sync when due, hurry a waiting reconnect, set the next alarm. */
    fun onAutoSyncAlarm() {
        if (!running || settings.watch == null) return
        if (ready) {
            if (autoSyncDue()) {
                log.info("Sync requested by the auto-sync timer")
                requestBatteryAndSync()
            }
        } else {
            // The reconnect delay is a Handler delay and stands still while the phone sleeps.
            onWatchPresent()
        }
        scheduleAutoSync()
    }

    /** The app came to the foreground: sync when the last one is older than 5 minutes. */
    fun onAppOpened() {
        val due = LinkRules.shouldSyncOnOpen(ready, syncBusy(), settings.lastSync?.finishedAtMs, System.currentTimeMillis(), settings.autoSyncMinutes)
        if (due) {
            log.info("Sync requested: the app was opened")
            requestBatteryAndSync()
        }
    }

    /** Shows the remembered watch in `AppState` before any connection (Application start). */
    fun restore() {
        val watch = settings.watch
        hub.update {
            it.copy(
                watch = if (watch == null) WatchState.NotPaired else pairedState(watch, LinkState.Offline),
                sync = it.sync.copy(lastResult = settings.lastSync),
            )
        }
    }

    /** Re-counts the files copied in the last 60 days (a database read: HandlerThread only). */
    fun refreshLeftForGarminConnect() {
        val left = runCatching { hub.db.copiedCountSince(AlongsideRules.skipSince(System.currentTimeMillis())) }.getOrDefault(0)
        hub.update { it.copy(sync = it.sync.copy(leftForGarminConnect = left)) }
    }

    // ---- bonding and adapter broadcasts ----

    private fun systemReceiver() = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            when (intent.action) {
                BluetoothAdapter.ACTION_STATE_CHANGED -> {
                    when (intent.getIntExtra(BluetoothAdapter.EXTRA_STATE, BluetoothAdapter.ERROR)) {
                        BluetoothAdapter.STATE_TURNING_OFF, BluetoothAdapter.STATE_OFF -> onBluetoothOff()
                        BluetoothAdapter.STATE_ON -> onBluetoothOn()
                    }
                }
                BluetoothDevice.ACTION_BOND_STATE_CHANGED -> onBondChanged(intent)
            }
        }
    }

    @Suppress("DEPRECATION")
    private fun onBondChanged(intent: Intent) {
        val device: BluetoothDevice? = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            intent.getParcelableExtra(BluetoothDevice.EXTRA_DEVICE, BluetoothDevice::class.java)
        } else {
            intent.getParcelableExtra(BluetoothDevice.EXTRA_DEVICE)
        }
        val address = device?.address ?: return
        val state = intent.getIntExtra(BluetoothDevice.EXTRA_BOND_STATE, BluetoothDevice.ERROR)
        val previous = intent.getIntExtra(BluetoothDevice.EXTRA_PREVIOUS_BOND_STATE, BluetoothDevice.ERROR)
        val pending = pairing
        if (pending != null && address == pending.address) {
            when {
                state == BluetoothDevice.BOND_BONDED -> completeBond()
                state == BluetoothDevice.BOND_NONE && previous == BluetoothDevice.BOND_BONDING ->
                    failPairing("Pairing was refused or cancelled")
            }
            return
        }
        // The remembered watch lost its bond (removed in system settings, or the watch was reset).
        val watch = settings.watch
        if (watch != null && address.equals(watch.address, ignoreCase = true) && state == BluetoothDevice.BOND_NONE) {
            onBondLost("the bond was removed")
        }
    }

    /** No bond means the watch will not talk to us: stop retrying until the user pairs again. */
    private fun onBondLost(why: String) {
        if (bondLost) return
        bondLost = true
        log.error("Pairing lost ($why). Forget the watch and pair again.")
        handler.removeCallbacks(reconnect)
        teardown()
        setLink(LinkState.BondLost)
    }

    private fun completeBond() {
        val pending = pairing ?: return
        pairing = null
        handler.removeCallbacks(pairTimeout)
        settings.watch = RememberedWatch(pending.address, pending.name, pending.model, pending.reason, ownBond = !pending.bondedBefore)
        settings.firstConnect = true
        settings.firstConnectTries = 0
        bondLost = false
        backoff.reset()
        hub.update { it.copy(watch = pairedState(settings.watch!!, LinkState.Waiting)) }
        log.info("Paired with ${pending.model}")
        if (hub.companion.startObserving(pending.address)) log.info("The system watches for the watch in the background")
        running = true
        connect()
    }

    private fun failPairing(reason: String) {
        val pending = pairing
        handler.removeCallbacks(pairTimeout)
        pairing = null
        hub.companion.cancel()
        log.error(reason)
        if (pending != null) {
            // Leave nothing half-made behind: a bond or an association for a watch we do not keep.
            // A bond that existed before is another app's, so it stays.
            if (!pending.bondedBefore) removeBondIfPresent(pending.address)
            if (settings.watch == null) hub.companion.disassociate(pending.address)
        }
        hub.update { it.copy(watch = WatchState.NotPaired, pairingError = reason) }
        if (settings.watch == null) {
            running = false
            WatchService.stop(ctx)
        }
    }

    private fun removeBondIfPresent(address: String) {
        try {
            val device = adapter()?.getRemoteDevice(address) ?: return
            if (device.bondState != BluetoothDevice.BOND_NONE) removeBond(address)
        } catch (_: SecurityException) {
            // Without the permission there is nothing we can remove.
        } catch (_: IllegalArgumentException) {
            // Invalid address: nothing was bonded.
        }
    }

    private fun onBluetoothOff() {
        // Called for both TURNING_OFF and OFF: act and log once.
        if (adapterOff) return
        adapterOff = true
        hub.refreshPermissions()
        if (pairing != null) failPairing("Bluetooth was switched off")
        scanner.stop()
        handler.removeCallbacks(reconnect)
        teardown()
        setLink(LinkState.Offline)
        log.info("Bluetooth switched off")
    }

    private fun onBluetoothOn() {
        adapterOff = false
        hub.refreshPermissions()
        log.info("Bluetooth switched on")
        if (running && settings.watch != null) {
            handler.removeCallbacks(reconnect)
            handler.postDelayed(reconnect, 500)
        }
    }

    @SuppressLint("MissingPermission")
    private fun removeBond(address: String) {
        try {
            val device = adapter()?.getRemoteDevice(address) ?: return
            val removed = BluetoothDevice::class.java.getMethod("removeBond").invoke(device) as? Boolean
            if (removed != true) {
                log.warn("Android kept the bond: remove the watch in Bluetooth settings before pairing it again")
            }
        } catch (_: Exception) {
            log.warn("Could not remove the bond: remove the watch in Bluetooth settings before pairing it again")
        }
    }

    // ---- GATT ----

    private fun adapter(): BluetoothAdapter? = ctx.getSystemService(BluetoothManager::class.java)?.adapter

    // The Handler overload is the only one with a callback handler on API 31 to 36; API 37 adds
    // a settings-based one that minSdk 31 cannot use.
    @Suppress("DEPRECATION")
    private fun connect() {
        if (!running || gatt != null || client != null) return
        val watch = settings.watch ?: return
        // The previous client is closed before a new one is made, never two at once.
        if (closing != null) {
            handler.removeCallbacks(reconnect)
            handler.postDelayed(reconnect, CLOSE_RETRY_MS)
            return
        }
        if (bondLost) {
            setLink(LinkState.BondLost)
            return
        }
        if (!hub.hasBluetoothPermissions()) {
            permissionLost()
            return
        }
        if (!hub.bluetoothEnabled()) {
            hub.refreshPermissions()
            setLink(LinkState.Offline)
            return
        }
        permissionWarned = false
        try {
            val device = adapter()?.getRemoteDevice(watch.address)
            if (device != null && device.bondState != BluetoothDevice.BOND_BONDED) {
                onBondLost("no bond with the watch")
                return
            }
            attMtu = MIN_MTU
            // autoConnect keeps waiting in the background until the watch is in range. After
            // repeated stalls the supervisor asks for a direct connection instead.
            val direct = supervisor.directConnect
            val g = device?.connectGatt(
                ctx,
                !direct,
                callback,
                BluetoothDevice.TRANSPORT_LE,
                BluetoothDevice.PHY_LE_1M_MASK,
                handler,
            )
            if (g == null) {
                log.error("Android could not start a connection")
                scheduleReconnect(backoff.next())
                return
            }
            gatt = g
            gattConnected = false
            mtuSkipped = false
            setLink(LinkState.Waiting)
            if (direct) log.info("Connecting directly, the earlier attempts stalled")
            enterStep(LinkStep.Connecting)
        } catch (_: SecurityException) {
            permissionLost()
        } catch (_: IllegalArgumentException) {
            log.error("The saved watch address is not valid")
        }
    }

    // ---- setup steps: one deadline each, named in the log when one stalls ----

    private fun enterStep(next: LinkStep) {
        step = next
        handler.removeCallbacks(stepWatchdog)
        val timeout = LinkSupervisor.timeoutMs(next, supervisor.directConnect)
        if (timeout != null) handler.postDelayed(stepWatchdog, timeout)
    }

    private fun leaveSetup() {
        step = null
        handler.removeCallbacks(stepWatchdog)
    }

    private fun onStepStalled() {
        val stalled = step ?: return
        val g = gatt ?: return
        if (stalled == LinkStep.Connecting) {
            // A direct connect that found no watch: not a broken link, go back to the background connection.
            supervisor.connectFailed()
            log.info("No watch found by the direct connection")
            teardown()
            scheduleReconnect(backoff.next())
            return
        }
        when (val action = supervisor.onStall(stalled, mtuSkipped)) {
            StallAction.SkipMtu -> {
                mtuSkipped = true
                attMtu = MIN_MTU
                log.warn("Link stalled at ${stalled.label}, continuing with MTU $MIN_MTU")
                startDiscovery(g)
            }
            is StallAction.Reconnect -> {
                val r = action.recovery
                val next = buildList {
                    if (r.refreshCache) add("refreshing the GATT cache")
                    if (r.directConnect) add("connecting directly")
                }.joinToString(" and ")
                log.warn("Link stalled at ${stalled.label} (attempt ${r.attempt})${if (next.isNotEmpty()) ", next: $next" else ""}")
                hintOtherApp()
                teardown()
                scheduleReconnect(backoff.next())
            }
        }
    }

    private fun requestMtuStep(g: BluetoothGatt) {
        enterStep(LinkStep.MtuExchange)
        val asked = guarded(false) { g.requestMtu(REQUESTED_MTU) }
        if (!asked && step == LinkStep.MtuExchange) {
            mtuSkipped = true
            log.warn("Android would not start the MTU exchange, continuing with MTU $MIN_MTU")
            startDiscovery(g)
        }
    }

    private fun startDiscovery(g: BluetoothGatt) {
        enterStep(LinkStep.ServiceDiscovery)
        if (!guarded(false) { g.discoverServices() } && step == LinkStep.ServiceDiscovery) {
            linkFailed("Service discovery could not start")
        }
    }

    /**
     * Asks the stack to forget its cached services of the watch. `BluetoothGatt.refresh()` is a
     * hidden method: reflection, guarded, and a log line either way.
     */
    @SuppressLint("DiscouragedPrivateApi")
    private fun refreshGattCache(g: BluetoothGatt) {
        supervisor.refreshDone()
        val done = try {
            BluetoothGatt::class.java.getMethod("refresh").invoke(g) as? Boolean
        } catch (_: Exception) {
            null
        }
        log.info(if (done == true) "GATT cache refreshed" else "GATT cache refresh is not available on this Android")
    }

    private val callback = object : BluetoothGattCallback() {
        override fun onConnectionStateChange(g: BluetoothGatt, status: Int, newState: Int) {
            // The old client that is being closed: close it once the stack says it is disconnected.
            if (g === closing) {
                if (newState == BluetoothProfile.STATE_DISCONNECTED) finishClose()
                return
            }
            if (g !== gatt) {
                safeClose(g)
                return
            }
            if (newState == BluetoothProfile.STATE_CONNECTED && status == BluetoothGatt.GATT_SUCCESS) {
                if (step != LinkStep.Connecting) return // a repeated report of the same connection
                gattConnected = true
                setLink(LinkState.Connecting)
                log.info("Connected, setting up the link")
                if (supervisor.refreshCache) refreshGattCache(g)
                requestMtuStep(g)
            } else {
                log.info("Link lost (status $status)")
                gattConnected = false // already disconnected: nothing to wait for before close
                if (step == LinkStep.Connecting) supervisor.connectFailed()
                linkLost()
            }
        }

        override fun onMtuChanged(g: BluetoothGatt, mtu: Int, status: Int) {
            if (g !== gatt || client != null) return
            val answered = if (status == BluetoothGatt.GATT_SUCCESS) mtu else MIN_MTU
            when (step) {
                LinkStep.MtuExchange -> {
                    attMtu = answered
                    log.info("MTU $attMtu")
                    startDiscovery(g)
                }
                // The answer came after we gave up waiting for it: still the real MTU of this link.
                LinkStep.ServiceDiscovery, LinkStep.Notifications -> {
                    attMtu = answered
                    log.info("MTU $attMtu (late answer)")
                }
                else -> Unit
            }
        }

        override fun onServicesDiscovered(g: BluetoothGatt, status: Int) {
            if (g !== gatt || step != LinkStep.ServiceDiscovery) return
            if (status != BluetoothGatt.GATT_SUCCESS) return linkFailed("Service discovery failed (status $status)")
            val service = g.getService(GattPair.SERVICE) ?: return linkFailed("The watch has no multi-link service")
            val id = GattPair.choose(service.characteristics.map { it.uuid }, forcedNotifyId, avoidNotifyId)
                ?: return linkFailed("The watch offers no usable characteristic pair")
            val notify = service.getCharacteristic(GattPair.uuid(id))
            val write = service.getCharacteristic(GattPair.writeUuidFor(id))
            val cccd = notify?.getDescriptor(CCCD)
            if (notify == null || write == null || cccd == null) return linkFailed("Characteristic pair incomplete")
            currentNotifyId = id
            writeChar = write
            enterStep(LinkStep.Notifications)
            val enabled = guarded(false) { g.setCharacteristicNotification(notify, true) } &&
                guarded(false) { writeCccd(g, cccd) }
            if (!enabled) linkFailed("Could not enable notifications")
        }

        override fun onDescriptorWrite(g: BluetoothGatt, descriptor: BluetoothGattDescriptor, status: Int) {
            if (g !== gatt || descriptor.uuid != CCCD || step != LinkStep.Notifications) return
            if (status != BluetoothGatt.GATT_SUCCESS) return linkFailed("Enabling notifications failed (status $status)")
            leaveSetup()
            startClient()
        }

        override fun onCharacteristicWrite(g: BluetoothGatt, characteristic: BluetoothGattCharacteristic, status: Int) {
            if (g !== gatt) return
            handlePump(writes.onWritten(status == BluetoothGatt.GATT_SUCCESS))
        }

        // API 33 and later call this one ...
        override fun onCharacteristicChanged(g: BluetoothGatt, characteristic: BluetoothGattCharacteristic, value: ByteArray) {
            if (g !== gatt) return
            feed(Input.Notification(value))
        }

        // ... older versions this one.
        @Deprecated("Replaced by the overload with the value, from API 33")
        @Suppress("DEPRECATION")
        override fun onCharacteristicChanged(g: BluetoothGatt, characteristic: BluetoothGattCharacteristic) {
            if (g !== gatt || Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) return
            characteristic.value?.let { feed(Input.Notification(it)) }
        }
    }

    @Suppress("DEPRECATION")
    private fun writeCccd(g: BluetoothGatt, cccd: BluetoothGattDescriptor): Boolean =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            g.writeDescriptor(cccd, BluetoothGattDescriptor.ENABLE_NOTIFICATION_VALUE) == BluetoothStatusCodes.SUCCESS
        } else {
            cccd.value = BluetoothGattDescriptor.ENABLE_NOTIFICATION_VALUE
            g.writeDescriptor(cccd)
        }

    /** Notifications are on: a new client for this connection (one per connection). */
    private fun startClient() {
        // Charged against the budget only when the handshake finishes (see onHandshakeDone).
        val mode = settings.garminConnectMode
        connectionMode = mode
        // Alongside sends no first_connect events (Garmin Connect owns the pairing) and leaves the
        // saved flag alone, so switching back to Off still announces the pairing.
        val first = AlongsideRules.firstConnect(mode, settings.firstConnect && LinkRules.firstConnectFor(settings.firstConnectTries))
        connectionHasFirst = first
        epoch += 1
        val base = CapabilityHex.parse(settings.capabilitiesHex) ?: CapabilityHex.parse(CapabilityHex.DEFAULT)!!
        // Stage 2f: a duty is claimed (and switched on in the client) only when the owner turned it
        // on and the system lets us do it; otherwise the watch behaves as before.
        val duties = PhoneDuties.effective(settings.phoneFeatures(), hub.phoneAccess())
        connectionDuties = duties
        val capabilities = PhoneDuties.claim(base, duties)
        client = NimragClient(
            ClientConfig(
                clientId = settings.clientId.toULong(),
                connectionEpoch = epoch.toUInt(),
                hostName = hostName(),
                hostManufacturer = Build.MANUFACTURER,
                hostModel = Build.MODEL,
                appVersion = APP_VERSION.toUShort(),
                capabilities = capabilities,
                firstConnect = first,
                archive = AlongsideRules.archive(mode),
                skip = skipKeys(mode),
                duties = Duties(
                    notifications = duties.notifications,
                    music = duties.music,
                    findPhone = duties.findPhone,
                    weather = duties.weather,
                ),
            ),
        )
        ready = false
        writes.clear()
        busyRetries = 0
        feed(Input.Connected(attMtu.toUShort()))
    }

    /** Alongside: every file copied in the last 60 days stays on the watch for Garmin Connect, so it is not fetched again. */
    private fun skipKeys(mode: GarminConnectMode): List<FileKey> {
        if (mode != GarminConnectMode.Alongside) return emptyList()
        val rows = try {
            val since = AlongsideRules.skipSince(System.currentTimeMillis())
            hub.db.pruneCopied(since)
            hub.db.copiedSince(since)
        } catch (e: Exception) {
            log.warn("Could not read the copied files: ${e.javaClass.simpleName}")
            return emptyList()
        }
        return rows.map {
            FileKey(
                dataType = it.dataType.toUByte(),
                subType = it.subType.toUByte(),
                fileNumber = it.fileNumber.toUShort(),
                size = it.size.toUInt(),
                timestamp = it.timestamp.toUInt(),
            )
        }
    }

    private fun hostName(): String {
        val name = try {
            android.provider.Settings.Global.getString(ctx.contentResolver, "device_name")
        } catch (_: Exception) {
            null
        }
        return name?.takeIf { it.isNotBlank() } ?: Build.MODEL
    }

    // ---- writes ----

    private fun sendOne(value: ByteArray): WriteQueue.SendResult {
        val g = gatt ?: return WriteQueue.SendResult.Failed
        val c = writeChar ?: return WriteQueue.SendResult.Failed
        return try {
            sendValue(g, c, value)
        } catch (_: SecurityException) {
            permissionLost()
            WriteQueue.SendResult.Failed
        }
    }

    @Suppress("DEPRECATION")
    private fun sendValue(g: BluetoothGatt, c: BluetoothGattCharacteristic, value: ByteArray): WriteQueue.SendResult =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            when (g.writeCharacteristic(c, value, BluetoothGattCharacteristic.WRITE_TYPE_NO_RESPONSE)) {
                BluetoothStatusCodes.SUCCESS -> WriteQueue.SendResult.Started
                BluetoothStatusCodes.ERROR_GATT_WRITE_REQUEST_BUSY -> WriteQueue.SendResult.Busy
                else -> WriteQueue.SendResult.Failed
            }
        } else {
            c.writeType = BluetoothGattCharacteristic.WRITE_TYPE_NO_RESPONSE
            c.value = value
            // false means busy or failed; the busy counter turns a persistent failure into a reconnect.
            if (g.writeCharacteristic(c)) WriteQueue.SendResult.Started else WriteQueue.SendResult.Busy
        }

    private fun handlePump(result: WriteQueue.Pump) {
        when (result) {
            WriteQueue.Pump.Sent -> busyRetries = 0
            WriteQueue.Pump.Idle -> Unit
            WriteQueue.Pump.Busy -> {
                if (++busyRetries > MAX_BUSY_RETRIES) {
                    linkFailed("The stack kept refusing writes")
                } else {
                    handler.removeCallbacks(pumpAgain)
                    handler.postDelayed(pumpAgain, BUSY_RETRY_MS)
                }
            }
            WriteQueue.Pump.Failed -> linkFailed("A write to the watch failed")
        }
    }

    // ---- the client ----

    // The DST pair only changes when the zone changes or a transition has passed, so it is
    // not recomputed for every notification (a file is thousands of them).
    private var dstZone: ZoneId? = null
    private var dstPair: DstPair? = null
    private var dstValidUntilS = Long.MIN_VALUE

    private fun now(): Now {
        val instant = Instant.now()
        val zone = ZoneId.systemDefault()
        val rules = zone.rules
        if (zone != dstZone || instant.epochSecond >= dstValidUntilS) {
            val pair = DstRule.compute(rules, instant)
            dstZone = zone
            dstPair = pair?.let { DstPair(it.startUnixS, it.endUnixS) }
            // The earlier of the two non-zero times is the next transition.
            dstValidUntilS = pair?.let { listOf(it.startUnixS, it.endUnixS).filter { t -> t > 0 }.minOrNull() }
                ?: Long.MAX_VALUE
        }
        return Now(
            monoMs = SystemClock.elapsedRealtime().toULong(),
            unixS = instant.epochSecond,
            utcOffsetS = rules.getOffset(instant).totalSeconds,
            dst = dstPair,
        )
    }

    private fun feed(input: Input) {
        val c = client ?: return
        val step = try {
            c.handle(now(), input)
        } catch (e: Exception) {
            log.error("Protocol client error: ${e.javaClass.simpleName}")
            linkFailed("The protocol client failed")
            return
        }
        process(step)
    }

    private fun process(step: Step) {
        if (step.writes.isNotEmpty()) {
            writes.enqueue(step.writes)
            handlePump(writes.pump())
        }
        armTimer(step.wakeAtMonoMs)
        for (event in step.events) handleEvent(event)
        // Backstop for the alarm: whatever the watch tells us, a sync that is due starts now.
        maybeAutoSync()
    }

    /** The one deadline of the client; each step replaces the earlier one. */
    private fun armTimer(wakeAtMonoMs: ULong?) {
        // Handler time (uptime) stops in deep sleep, so a tick can come late. That is fine: the
        // client accepts late ticks (an early one is a no-op), and the watch starts syncs itself.
        handler.removeCallbacks(tick)
        if (wakeAtMonoMs == null || client == null) return
        val delay = wakeAtMonoMs.toLong() - SystemClock.elapsedRealtime()
        handler.postDelayed(tick, delay.coerceAtLeast(0))
    }

    private fun handleEvent(event: Event) {
        when (event) {
            is Event.LinkReady -> Unit
            is Event.HandshakeDone -> onHandshakeDone(event)
            is Event.PairingCompleted -> {
                settings.firstConnect = false
                settings.firstConnectTries = 0
                log.info(
                    if (event.allAcked) "The watch accepted the pairing events" else "The watch refused some pairing events",
                )
            }
            is Event.TimeSent -> log.info("Time sent to the watch")
            is Event.SyncStarted -> {
                log.info("Sync started (${describe(event.trigger)})")
                hub.update {
                    it.copy(sync = it.sync.copy(running = true, filesInCycle = null, filesDone = 0, currentBytes = 0, currentSize = 0))
                }
            }
            is Event.Directory -> {
                log.info("The watch lists ${event.entries.size} files")
                hub.update { it.copy(sync = it.sync.copy(filesInCycle = event.entries.size)) }
            }
            is Event.FileDownloaded -> onFileDownloaded(event)
            is Event.FileFailed -> {
                val kind = kindOf(event.entry)
                log.warn("File not synced ($kind): ${describe(event.reason)}${if (event.willRetry) ", will retry" else ""}")
                hub.update { it.copy(sync = it.sync.copy(filesDone = it.sync.filesDone + 1, currentBytes = 0, currentSize = 0)) }
            }
            // Counted in SyncFinished and summed up in its one log line, not logged per file.
            is Event.FileSkipped -> {
                hub.update { it.copy(sync = it.sync.copy(filesDone = it.sync.filesDone + 1, currentBytes = 0, currentSize = 0)) }
            }
            is Event.FileArchived -> {
                hub.update { it.copy(sync = it.sync.copy(filesDone = it.sync.filesDone + 1, currentBytes = 0, currentSize = 0)) }
            }
            is Event.StaleConfirmation -> log.warn("The client ignored a stale store confirmation")
            is Event.SyncFinished -> {
                val result = SyncResult(
                    finishedAtMs = System.currentTimeMillis(),
                    downloaded = event.downloaded.toInt(),
                    archived = event.archived.toInt(),
                    failed = event.failed.toInt(),
                    skipped = event.skipped.toInt(),
                )
                settings.lastSync = result
                log.info(AlongsideRules.syncSummary(result, connectionMode))
                hub.update { it.copy(sync = it.sync.copy(running = false, currentBytes = 0, currentSize = 0, lastResult = result)) }
            }
            is Event.Unsupported -> log.info("The watch asked for something we do not do (message ${event.messageId})")
            is Event.Battery -> {
                updateWatch { it.copy(batteryPercent = event.levelPercent.toInt(), batteryAtMs = System.currentTimeMillis()) }
                log.info("Battery ${event.levelPercent}% (${if (event.report is BatteryReport.Status) "pushed" else "asked"})")
            }
            // Phone features (stage 2f): the bridge decides; it never logs content.
            is Event.NotificationSubscription,
            is Event.NotificationAction,
            is Event.FindPhoneStart,
            is Event.FindPhoneStop,
            is Event.MusicCommand,
            is Event.MusicRequested,
            is Event.FindWatch,
            -> hub.phone.onWatchEvent(event)
            is Event.WeatherRequest,
            is Event.WeatherSent,
            -> hub.weather.onWatchEvent(event)
            is Event.UploadFinished -> hub.workouts.onUploadFinished(event.uploadId, event.result)
            is Event.Anomaly -> log.warn("Anomaly: ${describe(event.kind)} (message ${event.messageId})")
            is Event.Closed -> onClientClosed(event.reason)
        }
    }

    private fun onHandshakeDone(event: Event.HandshakeDone) {
        ready = true
        otherAppHinted = false
        if (connectionHasFirst && settings.firstConnect) {
            val budget = LinkRules.afterHandshake(settings.firstConnectTries)
            settings.firstConnectTries = budget.tries
            if (!budget.stillSet) settings.firstConnect = false
        }
        backoff.reset()
        supervisor.healthy()
        channelRetries = 0
        forcedNotifyId = null
        avoidNotifyId = null
        val nowMs = System.currentTimeMillis()
        settings.lastConnectedAtMs = nowMs
        val version = LinkRules.formatSoftwareVersion(event.device.softwareVersion.toInt())
        updateWatch { it.copy(link = LinkState.Ready, softwareVersion = version, lastConnectedAtMs = nowMs) }
        log.info("Handshake done, watch software $version, ${event.fileTypes.size} file types")
        feed(Input.RequestBattery)
        hub.phone.onLinkReady(connectionDuties)
        hub.weather.onLinkReady()
        hub.workouts.onLinkReady()
        maybeAutoSync()
        scheduleAutoSync()
    }

    /** Store first (durable on the phone), only then `FileStored`; any failure is `FileStoreFailed`. */
    private fun onFileDownloaded(event: Event.FileDownloaded) {
        val entry = event.entry
        val kind = kindOf(entry)
        hub.update { it.copy(sync = it.sync.copy(currentBytes = event.data.size.toLong(), currentSize = entry.size.toLong())) }
        var sha: String? = null
        val stored = try {
            val saved = hub.store.save(
                dataType = entry.dataType.toInt(),
                subType = entry.subType.toInt(),
                fileIndex = entry.index.toInt(),
                fileNumber = entry.fileNumber.toInt(),
                garminTimestamp = entry.timestamp.toLong(),
                data = event.data,
                nowMs = System.currentTimeMillis(),
            )
            sha = saved.sha256
            log.info(
                if (saved.alreadyStored) "Stored a $kind file (${event.data.size} bytes), already on the phone"
                else "Stored a $kind file (${event.data.size} bytes)",
            )
            true
        } catch (e: Exception) {
            log.error("Could not store a $kind file: ${e.javaClass.simpleName}${e.message?.let { ": ${it.take(100)}" } ?: ""}")
            false
        }
        if (stored) {
            hub.uploader.wake()
            hub.local.onFileStored() // the phone's own copy of the data (personal flavor), off this thread
        }
        if (stored && sha != null && connectionMode == GarminConnectMode.Alongside) rememberCopied(entry, sha)
        feed(if (stored) Input.FileStored(event.transferId) else Input.FileStoreFailed(event.transferId))
    }

    /**
     * Alongside: the file is copied and stays on the watch for Garmin Connect, so no archive
     * follows. The key goes into `copied` so later connections do not fetch it again; a failed
     * write only means one more download later (the store dedupes by hash).
     */
    private fun rememberCopied(entry: DirEntry, sha256: String) {
        try {
            hub.db.insertCopied(
                CopiedRow(
                    dataType = entry.dataType.toInt(),
                    subType = entry.subType.toInt(),
                    fileNumber = entry.fileNumber.toInt(),
                    size = entry.size.toLong(),
                    timestamp = entry.timestamp.toLong(),
                    sha256 = sha256,
                    copiedAt = System.currentTimeMillis(),
                ),
            )
        } catch (e: Exception) {
            log.warn("Could not remember a copied file: ${e.javaClass.simpleName}")
        }
        // No FileArchived follows in this mode, so the file counts as done here.
        hub.update { it.copy(sync = it.sync.copy(filesDone = it.sync.filesDone + 1, currentBytes = 0, currentSize = 0)) }
        refreshLeftForGarminConnect()
    }

    private fun onClientClosed(reason: CloseReason) {
        val inUse = reason as? CloseReason.ChannelInUse
        val notifyId = currentNotifyId
        log.warn("Link closed: ${describe(reason)}")
        teardown()
        if (inUse != null && ++channelRetries <= MAX_CHANNEL_RETRIES) {
            forcedNotifyId = GattPair.notifyIdFromRetryChar(inUse.retryChar.toInt())
            avoidNotifyId = notifyId
            scheduleReconnect(CHANNEL_RETRY_MS)
        } else {
            if (inUse != null) {
                forcedNotifyId = null
                channelRetries = 0
                hintOtherApp()
            }
            scheduleReconnect(backoff.next())
        }
    }

    // ---- teardown and reconnect ----

    /** The GATT link dropped (the watch left, or Android gave up). */
    private fun linkLost() {
        teardown()
        scheduleReconnect(backoff.next())
    }

    private fun linkFailed(reason: String) {
        log.warn(reason)
        // A setup step that fails outright escalates like one that stalls (cache refresh, direct connect).
        step?.takeIf { it != LinkStep.Connecting }?.let { supervisor.onStall(it, mtuSkipped = true) }
        hintOtherApp()
        teardown()
        scheduleReconnect(backoff.next())
    }

    /** A link that will not come up while Garmin Connect is installed is most likely its doing. */
    private fun hintOtherApp() {
        if (otherAppHinted || !AlongsideRules.hintsOtherApp(settings.garminConnectMode)) return
        if (hub.garminConnect() != GarminConnect.HasBluetooth) return
        otherAppHinted = true
        log.warn("Garmin Connect probably holds the watch. Turn off its Nearby devices permission (Watch tab, Garmin Connect card) and Nimrag will reconnect")
    }

    /** Drop the client and the GATT object; the next connection starts from scratch. */
    private fun teardown() {
        handler.removeCallbacks(tick)
        leaveSetup()
        mtuSkipped = false
        syncAskedAtMs = null
        handler.removeCallbacks(pumpAgain)
        writes.clear()
        client?.let { runCatching { it.close() } }
        client = null
        ready = false
        hub.phone.onLinkDown()
        hub.weather.onLinkDown()
        hub.workouts.onLinkDown()
        writeChar = null
        currentNotifyId = null
        connectionHasFirst = false
        attMtu = MIN_MTU
        gatt?.let(::releaseGatt)
        gatt = null
        hub.update { it.copy(sync = it.sync.copy(running = false, currentBytes = 0, currentSize = 0)) }
    }

    private fun scheduleReconnect(delayMs: Long) {
        handler.removeCallbacks(reconnect)
        if (!running || settings.watch == null || bondLost) return
        setLink(if (hub.bluetoothEnabled()) LinkState.Waiting else LinkState.Offline)
        handler.postDelayed(reconnect, delayMs)
    }

    private fun permissionLost() {
        hub.refreshPermissions()
        if (!permissionWarned) {
            permissionWarned = true
            log.error("The Bluetooth permission is missing")
        }
        handler.removeCallbacks(reconnect)
        teardown()
        setLink(LinkState.Offline)
    }

    /**
     * Gives the client up. A connected one is disconnected first and closed when the stack
     * reports the disconnect (or after [CLOSE_WAIT_MS]); closing straight after `disconnect()`
     * can leave the link-layer connection alive, so the next client would meet a half-closed
     * link. [connect] never runs while one is still closing.
     */
    private fun releaseGatt(g: BluetoothGatt) {
        finishClose() // at most one client closing at a time
        val wasConnected = gattConnected
        gattConnected = false
        if (!wasConnected) {
            safeClose(g)
            return
        }
        closing = g
        try {
            g.disconnect()
        } catch (_: SecurityException) {
            // Nothing to disconnect without the permission; closed below.
        }
        handler.postDelayed(closeFallback, CLOSE_WAIT_MS)
    }

    private fun finishClose() {
        handler.removeCallbacks(closeFallback)
        val g = closing ?: return
        closing = null
        try {
            g.close()
        } catch (_: SecurityException) {
            // Nothing left to release without the permission.
        }
    }

    private fun safeClose(g: BluetoothGatt) {
        try {
            g.disconnect()
        } catch (_: SecurityException) {
            // Nothing to disconnect without the permission.
        }
        try {
            g.close()
        } catch (_: SecurityException) {
            // Nothing left to release without the permission.
        }
    }

    private inline fun <T> guarded(default: T, block: () -> T): T = try {
        block()
    } catch (_: SecurityException) {
        permissionLost()
        default
    }

    // ---- auto sync ----

    private fun requestBatteryAndSync() {
        feed(Input.RequestBattery)
        askForSync()
    }

    private fun askForSync() {
        syncAskedAtMs = SystemClock.elapsedRealtime()
        feed(Input.StartSync)
    }

    /** A sync runs, or one was asked for a moment ago and the watch has not started it yet. */
    private fun syncBusy(): Boolean =
        hub.state.value.sync.running || LinkRules.syncAskPending(syncAskedAtMs?.let { SystemClock.elapsedRealtime() - it })

    private fun autoSyncDue(): Boolean =
        LinkRules.shouldStartAutoSync(ready, syncBusy(), settings.lastSync?.finishedAtMs, System.currentTimeMillis(), settings.autoSyncMinutes)

    /** Starts a sync when the interval since the last finished one has run out (after the handshake, on a watch event, on the alarm). */
    private fun maybeAutoSync() {
        if (!autoSyncDue()) return
        log.info("Sync requested: the auto-sync interval has run out")
        askForSync()
    }

    /**
     * Sets the one auto-sync alarm for when the interval since the last finished sync runs out.
     * It stays set while the service runs, also while the link is down, because it is what hurries
     * a reconnect that waits out its delay. A Handler delay counts uptime and would stand still in
     * deep sleep, so this is an alarm ([AutoSyncAlarm]). Off (0), a stopped service and a forgotten
     * watch cancel it.
     */
    private fun scheduleAutoSync() {
        val minutes = settings.autoSyncMinutes
        if (!running || settings.watch == null || minutes <= 0) {
            autoSyncAlarm.cancel()
            return
        }
        val delayMs = LinkRules.nextAutoSyncDelayMs(settings.lastSync?.finishedAtMs, System.currentTimeMillis(), minutes)
        if (!autoSyncAlarm.arm(delayMs)) log.warn("Android refused to set the auto-sync timer")
    }

    // ---- state helpers ----

    private fun pairedState(watch: RememberedWatch, link: LinkState) = WatchState.Paired(
        address = watch.address,
        name = watch.name,
        model = watch.model,
        link = link,
        lastConnectedAtMs = settings.lastConnectedAtMs.takeIf { it > 0 },
        unsupportedReason = watch.unsupportedReason,
    )

    private fun setLink(link: LinkState) = updateWatch { it.copy(link = link) }

    private fun updateWatch(block: (WatchState.Paired) -> WatchState.Paired) {
        hub.update { s ->
            val w = s.watch
            if (w is WatchState.Paired) s.copy(watch = block(w)) else s
        }
    }

    private fun kindOf(entry: DirEntry) = FileKind.label(entry.dataType.toInt(), entry.subType.toInt())

    // ---- log texts ----

    private fun describe(trigger: SyncTrigger): String = when (trigger) {
        is SyncTrigger.Watch -> "asked by the watch"
        is SyncTrigger.Caller -> "asked by the phone"
        is SyncTrigger.FileAvailable -> "new file announced"
    }

    private fun describe(reason: FileFailure): String = when (reason) {
        is FileFailure.Crc -> "checksum or size mismatch"
        is FileFailure.Offset -> "chunk out of order"
        is FileFailure.Refused -> "the watch refused (status ${reason.status})"
        is FileFailure.Skipped -> "not a readable FIT file, skipped"
        is FileFailure.Compressed -> "compressed data is not supported"
        is FileFailure.StoreFailed -> "could not be stored on the phone"
        is FileFailure.ArchiveRefused -> "the watch refused to archive it"
    }

    private fun describe(kind: AnomalyKind): String = when (kind) {
        AnomalyKind.BAD_FRAME -> "bad frame"
        AnomalyKind.UNEXPECTED_RESPONSE -> "unexpected response"
        AnomalyKind.BAD_TRANSPORT -> "bad transport data"
        AnomalyKind.CLOCK_INVALID -> "phone clock invalid, time request not answered"
        AnomalyKind.CHUNKED_PROTOBUF -> "chunked protobuf request"
        AnomalyKind.SYNC_NOT_GRANTED -> "the watch did not start the sync we asked for"
        AnomalyKind.GFDI_CLOSED_BY_WATCH -> "the watch closed the GFDI channel"
        AnomalyKind.BATTERY_REQUEST_REFUSED -> "battery request refused or unanswered"
        AnomalyKind.DUTY_MESSAGE_REFUSED -> "a phone feature message (notification, music or find my watch) was refused or unanswered"
        AnomalyKind.UPLOAD_FAILED -> "an upload to the watch failed or was not answered"
    }

    private fun describe(reason: CloseReason): String = when (reason) {
        is CloseReason.Disconnected -> "disconnected"
        is CloseReason.MtuTooSmall -> "MTU too small"
        is CloseReason.ChannelInUse -> "the characteristic pair is in use, trying another"
        is CloseReason.RegisterRejected -> "the watch rejected service ${reason.service} (status ${reason.status})"
        is CloseReason.AuthRequested -> "the watch asked for authentication"
        is CloseReason.ProtocolTooOld -> "the watch speaks protocol ${reason.version}, too old"
        is CloseReason.Timeout -> "no answer from the watch (${reason.phase})"
        is CloseReason.GfdiClosed -> "the watch closed the channel (${reason.phase})"
        is CloseReason.RequestFailed -> "the watch refused message ${reason.messageId} (status ${reason.status})"
    }

    private companion object {
        val CCCD: UUID = UUID.fromString("00002902-0000-1000-8000-00805f9b34fb")
        const val REQUESTED_MTU = 515
        const val MIN_MTU = 23

        /** Garmin Explore's app version, seen working in CAP-1 [ours, revisit]. */
        const val APP_VERSION = 38215
        const val CLOSE_WAIT_MS = 1_500L
        const val CLOSE_RETRY_MS = 100L
        const val PAIR_TIMEOUT_MS = 120_000L
        const val BUSY_RETRY_MS = 25L
        const val MAX_BUSY_RETRIES = 200
        const val CHANNEL_RETRY_MS = 500L
        const val MODE_RECONNECT_MS = 500L
        const val MAX_CHANNEL_RETRIES = 5
    }
}
