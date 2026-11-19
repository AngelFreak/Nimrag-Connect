package dev.nimrag.app.engine.ble

import android.bluetooth.BluetoothManager
import android.bluetooth.le.ScanCallback
import android.bluetooth.le.ScanResult
import android.bluetooth.le.ScanSettings
import dev.nimrag.app.engine.Hub
import dev.nimrag.app.model.FoundDevice
import dev.nimrag.app.model.ScanState
import uniffi.nimrag_ffi.identifyDevice

/**
 * Scans for Garmin devices while the Watch screen asks for it (no filter, low latency,
 * stops by itself after 30 s). A result counts as Garmin when its manufacturer data carries
 * company id 0x0087 or the device catalogue recognises its name. All methods run on the
 * engine's HandlerThread; the platform's callbacks are forwarded to it.
 */
class BleScanner(private val hub: Hub) {
    private val found = LinkedHashMap<String, FoundDevice>()
    private var callback: ScanCallback? = null

    private val timeout = Runnable { stop("Scan finished") }

    fun start() {
        if (callback != null) return
        if (!hub.hasBluetoothPermissions()) {
            hub.refreshPermissions()
            hub.log.warn("Scanning needs the Bluetooth permission")
            return
        }
        val scanner = try {
            hub.app.getSystemService(BluetoothManager::class.java)?.adapter?.takeIf { it.isEnabled }?.bluetoothLeScanner
        } catch (_: SecurityException) {
            null
        }
        if (scanner == null) {
            hub.refreshPermissions()
            hub.log.warn("Bluetooth is off or has no scanner")
            return
        }
        found.clear()
        val cb = object : ScanCallback() {
            override fun onScanResult(callbackType: Int, result: ScanResult) {
                hub.handler.post { onResult(result) }
            }

            override fun onBatchScanResults(results: MutableList<ScanResult>) {
                hub.handler.post { results.forEach(::onResult) }
            }

            override fun onScanFailed(errorCode: Int) {
                hub.handler.post {
                    callback = null
                    hub.handler.removeCallbacks(timeout)
                    hub.update { it.copy(scan = it.scan.copy(scanning = false)) }
                    hub.log.error("Bluetooth scan failed (error $errorCode)")
                }
            }
        }
        try {
            scanner.startScan(null, ScanSettings.Builder().setScanMode(ScanSettings.SCAN_MODE_LOW_LATENCY).build(), cb)
        } catch (e: SecurityException) {
            hub.refreshPermissions()
            hub.log.warn("Scanning is not allowed: the Bluetooth permission is missing")
            return
        }
        callback = cb
        hub.update { it.copy(scan = ScanState(scanning = true, found = emptyList())) }
        hub.handler.postDelayed(timeout, SCAN_MS)
        hub.log.info("Scanning for Garmin devices")
    }

    fun stop(reason: String? = null) {
        hub.handler.removeCallbacks(timeout)
        val cb = callback ?: return
        callback = null
        try {
            hub.app.getSystemService(BluetoothManager::class.java)?.adapter?.bluetoothLeScanner?.stopScan(cb)
        } catch (_: SecurityException) {
            hub.refreshPermissions()
        } catch (_: IllegalStateException) {
            // Adapter switched off meanwhile: the scan is gone anyway.
        }
        hub.update { it.copy(scan = it.scan.copy(scanning = false)) }
        if (reason != null) hub.log.info("$reason (${found.size} found)")
    }

    private fun onResult(result: ScanResult) {
        if (callback == null) return
        val record = result.scanRecord
        // Manufacturer data without the company id, as identify_device wants it.
        val manufacturer = record?.getManufacturerSpecificData(GARMIN_COMPANY_ID)
        val advertisedName = record?.deviceName
        val info = identifyDevice(advertisedName, manufacturer)
        if (manufacturer == null && info.generic) return
        val address = result.device.address
        val existing = found[address]
        // Avoid republishing for every tiny signal change.
        if (existing != null && Math.abs(existing.rssi - result.rssi) < RSSI_STEP) return
        found[address] = FoundDevice(
            address = address,
            name = advertisedName ?: info.name,
            model = info.name,
            rssi = result.rssi,
            unsupportedReason = info.unsupportedReason,
        )
        hub.update { it.copy(scan = it.scan.copy(found = found.values.sortedByDescending { d -> d.rssi })) }
    }

    private companion object {
        const val GARMIN_COMPANY_ID = 0x0087
        const val SCAN_MS = 30_000L
        const val RSSI_STEP = 6
    }
}
