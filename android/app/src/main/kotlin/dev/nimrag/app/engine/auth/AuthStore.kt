package dev.nimrag.app.engine.auth

import android.content.SharedPreferences

/**
 * The non-secret facts about this phone's account: who is signed in (shown on the unlock screen,
 * sent with the password to `/api/app/unlock`) and the id of this phone's device token on the
 * server (so sign-out can revoke it). The secrets live in [SecretVault] and the upload token in
 * `EngineSettings`.
 */
class AuthStore(private val prefs: SharedPreferences) {
    val username: String?
        get() = prefs.getString(USERNAME, null)?.takeIf { it.isNotEmpty() }

    val deviceId: Long?
        get() = if (prefs.contains(DEVICE_ID)) prefs.getLong(DEVICE_ID, 0L) else null

    fun save(username: String, deviceId: Long) {
        prefs.edit().putString(USERNAME, username).putLong(DEVICE_ID, deviceId).apply()
    }

    fun clear() {
        prefs.edit().remove(USERNAME).remove(DEVICE_ID).apply()
    }

    private companion object {
        const val USERNAME = "username"
        const val DEVICE_ID = "device_id"
    }
}
