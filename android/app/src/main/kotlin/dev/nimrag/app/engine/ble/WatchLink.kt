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
import dev.nimrag.app.engine.Hub
import dev.nimrag.app.engine.RememberedWatch
import dev.nimrag.app.engine.WatchService
import dev.nimrag.app.engine.logic.Backoff
import dev.nimrag.app.engine.logic.CapabilityHex
import dev.nimrag.app.engine.logic.DstRule
import dev.nimrag.app.engine.logic.FileKind
import dev.nimrag.app.engine.logic.GattPair
import dev.nimrag.app.engine.logic.LinkRules
import dev.nimrag.app.engine.logic.WriteQueue
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
import uniffi.nimrag_ffi.Event
import uniffi.nimrag_ffi.FileFailure
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

    /** The current connection carries `first_connect`; it is charged on `HandshakeDone`. */
    private var connectionHasFirst = false
    private val backoff = Backoff()
    private var pairing: Pairing? = null

    private data class Pairing(val address: String, val name: String, val model: String, val reason: String?)

    private val writes = WriteQueue(::sendOne)

    // ---- runnables (all posted on the handler) ----
    private val tick = Runnable { feed(Input.Tick) }
    private val reconnect = Runnable { connect() }
    private val pumpAgain = Runnable { handlePump(writes.pump()) }
    private val setupWatchdog = Runnable { linkFailed("The link did not come up in time") }
    private val pairTimeout = Runnable { failPairing("Pairing timed out") }
    private val autoSync = Runnable {
        if (ready) {
            requestBatteryAndSync()
            scheduleAutoSync()
        }
    }

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
            // Idempotent: makes sure the system still reports the watch coming into range.
            hub.companion.startObserving(watch.address)
            connect()
        }
        return true
    }

    /** The service stopped: drop the link, keep the watch remembered. */
    fun stop() {
        running = false
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
        pairing = Pairing(address, name, model, unsupportedReason)
        bondLost = false
        hub.update { it.copy(watch = WatchState.Pairing(address, name)) }
        // Started now, while the UI is in the foreground; the service keeps the process alive.
        running = true
        WatchService.start(ctx)
        handler.postDelayed(pairTimeout, PAIR_TIMEOUT_MS)
        // First the companion association (the system's own dialog, shown by MainActivity),
        // then the bond. Without the association the pairing still works, but the link will
        // not come back by itself after Android killed the process.
        hub.companion.associate(address, ::onAssociation)
    }

    private fun onAssociation(outcome: CompanionAssociation.Outcome) {
        val pending = pairing ?: return
        when (outcome) {
            CompanionAssociation.Outcome.Created ->
                log.info("Pairing path: companion association in place, the system can restart the link")
            CompanionAssociation.Outcome.Unavailable ->
                log.warn("Pairing path: no companion association, after a process death the app must be opened to reconnect")
            CompanionAssociation.Outcome.Declined -> return failPairing("The companion association was declined")
        }
        startBonding(pending.address)
    }

    private fun startBonding(address: String) {
        try {
            val device = adapter()?.getRemoteDevice(address) ?: return failPairing("Bluetooth is not available")
            when (device.bondState) {
                BluetoothDevice.BOND_BONDED -> completeBond()
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
            removeBond(watch.address)
            hub.companion.disassociate(watch.address)
        }
        backoff.reset()
        forcedNotifyId = null
        avoidNotifyId = null
        channelRetries = 0
        bondLost = false
        settings.watch = null
        settings.firstConnect = false
        settings.firstConnectTries = 0
        settings.lastSync = null
        settings.lastConnectedAtMs = 0L
        running = false
        hub.update { it.copy(watch = WatchState.NotPaired, sync = SyncState(), scan = ScanState()) }
        log.info("Watch forgotten")
        WatchService.stop(ctx)
    }

    fun syncNow() {
        if (!ready) {
            log.warn("Sync now: the watch is not connected")
            return
        }
        log.info("Sync requested")
        requestBatteryAndSync()
    }

    /** The auto-sync interval changed. */
    fun onAutoSyncChanged() {
        handler.removeCallbacks(autoSync)
        if (ready) scheduleAutoSync()
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
        settings.watch = RememberedWatch(pending.address, pending.name, pending.model, pending.reason)
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
            removeBondIfPresent(pending.address)
            if (settings.watch == null) hub.companion.disassociate(pending.address)
        }
        hub.update { it.copy(watch = WatchState.NotPaired) }
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
        hub.refreshPermissions()
        if (pairing != null) failPairing("Bluetooth was switched off")
        scanner.stop()
        handler.removeCallbacks(reconnect)
        teardown()
        setLink(LinkState.Offline)
        log.info("Bluetooth switched off")
    }

    private fun onBluetoothOn() {
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
            // autoConnect keeps waiting in the background until the watch is in range.
            val g = device?.connectGatt(
                ctx,
                true,
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
            setLink(LinkState.Waiting)
        } catch (_: SecurityException) {
            permissionLost()
        } catch (_: IllegalArgumentException) {
            log.error("The saved watch address is not valid")
        }
    }

    private val callback = object : BluetoothGattCallback() {
        override fun onConnectionStateChange(g: BluetoothGatt, status: Int, newState: Int) {
            if (g !== gatt) {
                safeClose(g)
                return
            }
            if (newState == BluetoothProfile.STATE_CONNECTED && status == BluetoothGatt.GATT_SUCCESS) {
                setLink(LinkState.Connecting)
                log.info("Connected, setting up the link")
                handler.removeCallbacks(setupWatchdog)
                handler.postDelayed(setupWatchdog, SETUP_TIMEOUT_MS)
                val asked = guarded(false) { g.requestMtu(REQUESTED_MTU) }
                if (!asked) guarded(false) { g.discoverServices() }
            } else {
                log.info("Link lost (status $status)")
                linkLost()
            }
        }

        override fun onMtuChanged(g: BluetoothGatt, mtu: Int, status: Int) {
            if (g !== gatt) return
            attMtu = if (status == BluetoothGatt.GATT_SUCCESS) mtu else MIN_MTU
            log.info("MTU $attMtu")
            if (!guarded(false) { g.discoverServices() }) linkFailed("Service discovery could not start")
        }

        override fun onServicesDiscovered(g: BluetoothGatt, status: Int) {
            if (g !== gatt) return
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
            val enabled = guarded(false) { g.setCharacteristicNotification(notify, true) } &&
                guarded(false) { writeCccd(g, cccd) }
            if (!enabled) linkFailed("Could not enable notifications")
        }

        override fun onDescriptorWrite(g: BluetoothGatt, descriptor: BluetoothGattDescriptor, status: Int) {
            if (g !== gatt || descriptor.uuid != CCCD) return
            if (status != BluetoothGatt.GATT_SUCCESS) return linkFailed("Enabling notifications failed (status $status)")
            handler.removeCallbacks(setupWatchdog)
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
        val first = settings.firstConnect && LinkRules.firstConnectFor(settings.firstConnectTries)
        connectionHasFirst = first
        epoch += 1
        val capabilities = CapabilityHex.parse(settings.capabilitiesHex) ?: CapabilityHex.parse(CapabilityHex.DEFAULT)!!
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
            ),
        )
        ready = false
        writes.clear()
        busyRetries = 0
        feed(Input.Connected(attMtu.toUShort()))
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
                )
                settings.lastSync = result
                log.info("Sync finished: ${result.downloaded} downloaded, ${result.archived} archived, ${result.failed} failed")
                hub.update { it.copy(sync = it.sync.copy(running = false, currentBytes = 0, currentSize = 0, lastResult = result)) }
            }
            is Event.Unsupported -> log.info("The watch asked for something we do not do (message ${event.messageId})")
            is Event.Battery -> {
                updateWatch { it.copy(batteryPercent = event.levelPercent.toInt(), batteryAtMs = System.currentTimeMillis()) }
                log.info("Battery ${event.levelPercent}% (${if (event.report is BatteryReport.Status) "pushed" else "asked"})")
            }
            is Event.Anomaly -> log.warn("Anomaly: ${describe(event.kind)} (message ${event.messageId})")
            is Event.Closed -> onClientClosed(event.reason)
        }
    }

    private fun onHandshakeDone(event: Event.HandshakeDone) {
        ready = true
        if (connectionHasFirst && settings.firstConnect) {
            val budget = LinkRules.afterHandshake(settings.firstConnectTries)
            settings.firstConnectTries = budget.tries
            if (!budget.stillSet) settings.firstConnect = false
        }
        backoff.reset()
        channelRetries = 0
        forcedNotifyId = null
        avoidNotifyId = null
        val nowMs = System.currentTimeMillis()
        settings.lastConnectedAtMs = nowMs
        val version = LinkRules.formatSoftwareVersion(event.device.softwareVersion.toInt())
        updateWatch { it.copy(link = LinkState.Ready, softwareVersion = version, lastConnectedAtMs = nowMs) }
        log.info("Handshake done, watch software $version, ${event.fileTypes.size} file types")
        feed(Input.RequestBattery)
        if (LinkRules.shouldStartSync(settings.lastSync?.finishedAtMs, nowMs, settings.autoSyncMinutes)) {
            feed(Input.StartSync)
        }
        scheduleAutoSync()
    }

    /** Store first (durable on the phone), only then `FileStored`; any failure is `FileStoreFailed`. */
    private fun onFileDownloaded(event: Event.FileDownloaded) {
        val entry = event.entry
        val kind = kindOf(entry)
        hub.update { it.copy(sync = it.sync.copy(currentBytes = event.data.size.toLong(), currentSize = entry.size.toLong())) }
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
            log.info(
                if (saved.alreadyStored) "Stored a $kind file (${event.data.size} bytes), already on the phone"
                else "Stored a $kind file (${event.data.size} bytes)",
            )
            true
        } catch (e: Exception) {
            log.error("Could not store a $kind file: ${e.javaClass.simpleName}${e.message?.let { ": ${it.take(100)}" } ?: ""}")
            false
        }
        if (stored) hub.uploader.wake()
        feed(if (stored) Input.FileStored(event.transferId) else Input.FileStoreFailed(event.transferId))
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
        teardown()
        scheduleReconnect(backoff.next())
    }

    /** Drop the client and the GATT object; the next connection starts from scratch. */
    private fun teardown() {
        handler.removeCallbacks(tick)
        handler.removeCallbacks(setupWatchdog)
        handler.removeCallbacks(autoSync)
        handler.removeCallbacks(pumpAgain)
        writes.clear()
        client?.let { runCatching { it.close() } }
        client = null
        ready = false
        writeChar = null
        currentNotifyId = null
        connectionHasFirst = false
        attMtu = MIN_MTU
        gatt?.let(::safeClose)
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
        feed(Input.StartSync)
    }

    private fun scheduleAutoSync() {
        handler.removeCallbacks(autoSync)
        val minutes = settings.autoSyncMinutes
        if (minutes > 0) handler.postDelayed(autoSync, minutes * 60_000L)
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
        const val SETUP_TIMEOUT_MS = 20_000L
        const val PAIR_TIMEOUT_MS = 120_000L
        const val BUSY_RETRY_MS = 25L
        const val MAX_BUSY_RETRIES = 200
        const val CHANNEL_RETRY_MS = 500L
        const val MAX_CHANNEL_RETRIES = 5
    }
}
