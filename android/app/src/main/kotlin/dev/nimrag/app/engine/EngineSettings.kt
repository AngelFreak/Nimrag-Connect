package dev.nimrag.app.engine

import android.content.Context
import dev.nimrag.app.engine.logic.CapabilityHex
import dev.nimrag.app.engine.logic.RemovedSettings
import dev.nimrag.app.model.HistoryRetention
import dev.nimrag.app.model.PhoneFeatures
import dev.nimrag.app.model.Settings
import dev.nimrag.app.model.SyncResult
import dev.nimrag.app.model.WebMode
import java.security.SecureRandom

/** The watch this install is paired with. */
data class RememberedWatch(
    val address: String,
    val name: String,
    val model: String,
    val unsupportedReason: String?,
    /**
     * True when this app created the Bluetooth bond. False when the watch was already bonded
     * (by Garmin Connect, say): forgetting it here must not unpair it for the other app.
     */
    val ownBond: Boolean = true,
)

/**
 * Settings, secrets and small remembered facts in private SharedPreferences (the app
 * sandbox is the protection; the token is never logged and never goes into `AppState`).
 */
class EngineSettings(context: Context) {
    private val prefs = context.getSharedPreferences("engine", Context.MODE_PRIVATE)

    var serverUrl: String
        get() = prefs.getString(SERVER_URL, "").orEmpty()
        set(value) = prefs.edit().putString(SERVER_URL, value).apply()

    var token: String?
        get() = prefs.getString(TOKEN, null)?.takeIf { it.isNotEmpty() }
        set(value) {
            val edit = prefs.edit()
            if (value.isNullOrEmpty()) edit.remove(TOKEN) else edit.putString(TOKEN, value)
            edit.apply()
        }

    /** Debug override of which web source the shell uses (stored by name; Auto when unknown). */
    var webMode: WebMode
        get() = runCatching { WebMode.valueOf(prefs.getString(WEB_MODE, null).orEmpty()) }.getOrDefault(WebMode.Auto)
        set(value) = prefs.edit().putString(WEB_MODE, value.name).apply()

    var allowMobileData: Boolean
        get() = prefs.getBoolean(ALLOW_MOBILE, false)
        set(value) = prefs.edit().putBoolean(ALLOW_MOBILE, value).apply()

    /** "Server history on this phone" (Data retention): Off until the person picks a window. */
    var historyRetention: HistoryRetention
        get() = HistoryRetention.fromKey(prefs.getString(HISTORY_RETENTION, null))
        set(value) = prefs.edit().putString(HISTORY_RETENTION, value.key).apply()

    /** The choice the phone's copy of the server history matches (after copying or pruning for it). */
    var historyApplied: HistoryRetention
        get() = HistoryRetention.fromKey(prefs.getString(HISTORY_APPLIED, null))
        set(value) = prefs.edit().putString(HISTORY_APPLIED, value.key).apply()

    /** When the server's list was last walked to its end with nothing left to copy (unix ms); 0 = never. */
    var historyCheckedAtMs: Long
        get() = prefs.getLong(HISTORY_CHECKED, 0L)
        set(value) = prefs.edit().putLong(HISTORY_CHECKED, value).apply()

    var autoSyncMinutes: Int
        get() = prefs.getInt(AUTO_SYNC, DEFAULT_AUTO_SYNC_MINUTES)
        set(value) = prefs.edit().putInt(AUTO_SYNC, value).apply()

    /** The mask in effect: the saved one, else the default. Always valid hex. */
    var capabilitiesHex: String
        get() = prefs.getString(CAPABILITIES, null)?.let { CapabilityHex.normalize(it) } ?: CapabilityHex.DEFAULT
        set(value) = prefs.edit().putString(CAPABILITIES, value).apply()

    /**
     * 0.8.0 removed the Garmin Connect mode: whatever was stored is ignored (everyone is
     * "Nimrag only") and the key is cleared. True when it held Alongside, so the caller can log it once.
     */
    fun dropRemovedGarminConnectMode(): Boolean {
        val key = RemovedSettings.GARMIN_CONNECT_MODE_KEY
        if (!prefs.contains(key)) return false
        val wasAlongside = RemovedSettings.alongsideWasStored(prefs.getString(key, null))
        prefs.edit().remove(key).apply()
        return wasAlongside
    }

    /** Phone features (stage 2f): every one is off until the owner switches it on. */
    var phoneNotifications: Boolean
        get() = prefs.getBoolean(PHONE_NOTIFICATIONS, false)
        set(value) = prefs.edit().putBoolean(PHONE_NOTIFICATIONS, value).apply()

    var phoneMusic: Boolean
        get() = prefs.getBoolean(PHONE_MUSIC, false)
        set(value) = prefs.edit().putBoolean(PHONE_MUSIC, value).apply()

    var phoneFindPhone: Boolean
        get() = prefs.getBoolean(PHONE_FIND, false)
        set(value) = prefs.edit().putBoolean(PHONE_FIND, value).apply()

    var phoneWeather: Boolean
        get() = prefs.getBoolean(PHONE_WEATHER, false)
        set(value) = prefs.edit().putBoolean(PHONE_WEATHER, value).apply()

    var phoneGpsAssist: Boolean
        get() = prefs.getBoolean(PHONE_GPS_ASSIST, false)
        set(value) = prefs.edit().putBoolean(PHONE_GPS_ASSIST, value).apply()

    /** When satellite data last reached the watch (unix ms); 0 = never. */
    var gpsAssistLastSentMs: Long
        get() = prefs.getLong(GPS_ASSIST_LAST_SENT, 0L)
        set(value) = prefs.edit().putLong(GPS_ASSIST_LAST_SENT, value).apply()

    /** Apps whose notifications stay on the phone (package names). A copy: the stored set is never edited in place. */
    var excludedApps: Set<String>
        get() = prefs.getStringSet(EXCLUDED_APPS, null)?.toSet().orEmpty()
        set(value) = prefs.edit().putStringSet(EXCLUDED_APPS, value.toSet()).apply()

    fun phoneFeatures() = PhoneFeatures(
        notifications = phoneNotifications,
        music = phoneMusic,
        findPhone = phoneFindPhone,
        weather = phoneWeather,
        gpsAssist = phoneGpsAssist,
        gpsAssistLastSentMs = gpsAssistLastSentMs,
        excludedApps = excludedApps,
    )

    /** Multi-link client id: random, chosen once per install. */
    val clientId: Long
        get() {
            val saved = prefs.getLong(CLIENT_ID, 0L)
            if (saved != 0L) return saved
            var fresh = 0L
            while (fresh == 0L) fresh = SecureRandom().nextLong()
            prefs.edit().putLong(CLIENT_ID, fresh).apply()
            return fresh
        }

    var watch: RememberedWatch?
        get() {
            val address = prefs.getString(WATCH_ADDRESS, null) ?: return null
            return RememberedWatch(
                address = address,
                name = prefs.getString(WATCH_NAME, "").orEmpty(),
                model = prefs.getString(WATCH_MODEL, "").orEmpty(),
                unsupportedReason = prefs.getString(WATCH_UNSUPPORTED, null),
                ownBond = prefs.getBoolean(WATCH_OWN_BOND, true),
            )
        }
        set(value) {
            val edit = prefs.edit()
            if (value == null) {
                edit.remove(WATCH_ADDRESS).remove(WATCH_NAME).remove(WATCH_MODEL).remove(WATCH_UNSUPPORTED)
                    .remove(WATCH_OWN_BOND)
            } else {
                edit.putString(WATCH_ADDRESS, value.address)
                    .putString(WATCH_NAME, value.name)
                    .putString(WATCH_MODEL, value.model)
                    .putBoolean(WATCH_OWN_BOND, value.ownBond)
                if (value.unsupportedReason == null) {
                    edit.remove(WATCH_UNSUPPORTED)
                } else {
                    edit.putString(WATCH_UNSUPPORTED, value.unsupportedReason)
                }
            }
            edit.apply()
        }

    /** True from bonding until `PairingCompleted` (or the retry budget is used up). */
    var firstConnect: Boolean
        get() = prefs.getBoolean(FIRST_CONNECT, false)
        set(value) = prefs.edit().putBoolean(FIRST_CONNECT, value).apply()

    /** Connections that already carried `first_connect` without finishing it. */
    var firstConnectTries: Int
        get() = prefs.getInt(FIRST_CONNECT_TRIES, 0)
        set(value) = prefs.edit().putInt(FIRST_CONNECT_TRIES, value).apply()

    /** Wall-clock ms of the last finished handshake; 0 = never. */
    var lastConnectedAtMs: Long
        get() = prefs.getLong(LAST_CONNECTED, 0L)
        set(value) = prefs.edit().putLong(LAST_CONNECTED, value).apply()

    var lastSync: SyncResult?
        get() {
            val at = prefs.getLong(SYNC_AT, 0L)
            if (at == 0L) return null
            return SyncResult(
                finishedAtMs = at,
                downloaded = prefs.getInt(SYNC_DOWNLOADED, 0),
                archived = prefs.getInt(SYNC_ARCHIVED, 0),
                failed = prefs.getInt(SYNC_FAILED, 0),
            )
        }
        set(value) {
            val edit = prefs.edit()
            if (value == null) {
                edit.remove(SYNC_AT)
            } else {
                edit.putLong(SYNC_AT, value.finishedAtMs)
                    .putInt(SYNC_DOWNLOADED, value.downloaded)
                    .putInt(SYNC_ARCHIVED, value.archived)
                    .putInt(SYNC_FAILED, value.failed)
            }
            edit.apply()
        }

    /** The public part of the settings, for `AppState`. */
    fun toState() = Settings(
        serverUrl = serverUrl,
        hasToken = token != null,
        allowMobileData = allowMobileData,
        autoSyncMinutes = autoSyncMinutes,
        historyRetention = historyRetention,
        capabilitiesHex = capabilitiesHex,
        phone = phoneFeatures(),
    )

    private companion object {
        const val SERVER_URL = "server_url"
        const val WEB_MODE = "web_mode"
        const val TOKEN = "token"
        const val ALLOW_MOBILE = "allow_mobile_data"
        const val AUTO_SYNC = "auto_sync_minutes"
        const val HISTORY_RETENTION = "history_retention"
        const val HISTORY_APPLIED = "history_applied"
        const val HISTORY_CHECKED = "history_checked_at"

        /** Applies to everyone who never changed the interval (the preference is simply absent). */
        const val DEFAULT_AUTO_SYNC_MINUTES = 15
        const val CAPABILITIES = "capabilities_hex"
        const val CLIENT_ID = "client_id"
        const val PHONE_NOTIFICATIONS = "phone_notifications"
        const val PHONE_MUSIC = "phone_music"
        const val PHONE_FIND = "phone_find_phone"
        const val PHONE_WEATHER = "phone_weather"
        const val PHONE_GPS_ASSIST = "phone_gps_assist"
        const val GPS_ASSIST_LAST_SENT = "gps_assist_last_sent_ms"
        const val EXCLUDED_APPS = "phone_excluded_apps"
        const val WATCH_ADDRESS = "watch_address"
        const val WATCH_NAME = "watch_name"
        const val WATCH_MODEL = "watch_model"
        const val WATCH_UNSUPPORTED = "watch_unsupported"
        const val WATCH_OWN_BOND = "watch_own_bond"
        const val FIRST_CONNECT = "first_connect"
        const val FIRST_CONNECT_TRIES = "first_connect_tries"
        const val LAST_CONNECTED = "last_connected_at"
        const val SYNC_AT = "last_sync_at"
        const val SYNC_DOWNLOADED = "last_sync_downloaded"
        const val SYNC_ARCHIVED = "last_sync_archived"
        const val SYNC_FAILED = "last_sync_failed"
    }
}
