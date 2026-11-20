package dev.nimrag.app.engine.auth

import android.app.Activity
import android.content.Context
import android.os.Build
import dev.nimrag.app.R
import dev.nimrag.app.engine.EngineSettings
import dev.nimrag.app.engine.auth.logic.AuthApi
import dev.nimrag.app.engine.auth.logic.AuthFailure
import dev.nimrag.app.engine.auth.logic.AuthMachine
import dev.nimrag.app.engine.auth.logic.AuthPhase
import dev.nimrag.app.engine.auth.logic.DeviceName
import dev.nimrag.app.engine.auth.logic.HttpUrlTransport
import dev.nimrag.app.engine.auth.logic.LoginForm
import dev.nimrag.app.engine.auth.logic.LoginResult
import dev.nimrag.app.engine.auth.logic.SessionCookie
import dev.nimrag.app.engine.auth.logic.SessionResult
import dev.nimrag.app.engine.logic.UploadPolicy
import dev.nimrag.app.engine.store.EngineLog
import dev.nimrag.app.model.AccountController
import dev.nimrag.app.model.AccountState
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock

/**
 * The app's account: first sign-in, unlock by fingerprint or password, sign out, and the check of
 * whether the session is still good. It owns the phase ([AuthPhase]) and keeps every secret out
 * of [AccountState] and out of the log.
 *
 * What is stored where:
 *  - the server's **upload token** goes to [EngineSettings.token] (the upload worker uses it, so
 *    uploads keep working while the session is locked or gone);
 *  - the **device secret** goes to [SecretVault] (Android Keystore, see there);
 *  - the **session cookie** goes into the web view's cookie jar ([WebSession]) and nowhere else.
 */
class AccountManager(
    private val context: Context,
    private val settings: EngineSettings,
    private val store: AuthStore,
    private val vault: SecretVault,
    private val web: WebSession,
    private val log: EngineLog,
    /** The server address, token or sign-in changed: publish settings and wake the uploader. */
    private val onUploadCredentialsChanged: () -> Unit,
    private val api: AuthApi = AuthApi(HttpUrlTransport(), probeHttp = HttpUrlTransport(PROBE_TIMEOUT_MS)),
    private val deviceName: String = DeviceName.of(Build.MODEL),
) : AccountController {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val operation = Mutex()
    private var lastProbeMs = 0L
    private val flow = MutableStateFlow(AccountState(username = store.username))
    override val state: StateFlow<AccountState> get() = flow

    private fun base(): String? = UploadPolicy.normalizeServerUrl(settings.serverUrl)

    private fun hasCredentials() = vault.hasSecret()

    // ---- launch and coming back

    override fun checkSession() {
        scope.launch {
            operation.withLock {
                // Coming back to the app asks again, but not more often than every 30 seconds.
                val now = System.currentTimeMillis()
                if (flow.value.phase != AuthPhase.Starting && now - lastProbeMs < PROBE_MIN_GAP_MS) return@withLock
                lastProbeMs = now
                val base = base()
                if (base == null) {
                    move(AuthMachine.Event.SignedOut)
                    return@withLock
                }
                val result = api.probe(base, web.current(base))
                move(AuthMachine.Event.Probed(result))
                refreshBiometric()
            }
        }
    }

    override fun sessionExpired() {
        scope.launch {
            operation.withLock {
                // Only an end of the session in a signed-in app counts; a 401 while signing in is an answer.
                if (flow.value.phase == AuthPhase.SignedIn) {
                    move(AuthMachine.Event.SessionExpired)
                    refreshBiometric()
                }
            }
        }
    }

    /**
     * An upload was refused (401 or 403): the token on this phone was revoked, or it is an old
     * hand-entered one that no longer works. Same as a revoked device: forget everything and ask
     * for a sign-in. Not part of [AccountController]: only the uploader calls it.
     */
    fun tokenRejected() {
        scope.launch {
            operation.withLock {
                // A sign-in in progress owns the credentials; its result decides.
                if (flow.value.busy) return@withLock
                log.warn("The server no longer accepts this phone's token")
                wipeLocal(AuthFailure.TokenRejected)
            }
        }
    }

    override fun clearFailure() {
        flow.update { it.copy(failure = null) }
    }

    // ---- first sign-in

    override fun signIn(server: String, username: String, password: String, code: String) {
        val form = LoginForm.check(server, username, password, code)
        if (form is LoginForm.Checked.Bad) {
            fail(form.failure)
            return
        }
        form as LoginForm.Checked.Ok
        scope.launch {
            operation.withLock {
                if (flow.value.busy) return@withLock
                flow.update { it.copy(busy = true, failure = null) }
                when (val result = api.login(form.base, form.username, password, form.code, deviceName)) {
                    is LoginResult.Failure -> fail(result.failure)
                    is LoginResult.Success -> {
                        val grant = result.grant
                        // The old web session (a previous person's, or a leftover) goes before the new one comes.
                        web.clear(form.base)
                        web.install(form.base, grant.cookie)
                        settings.serverUrl = form.base
                        settings.token = grant.token
                        store.save(grant.username, grant.deviceId)
                        vault.save(grant.deviceSecret)
                        log.info("Signed in on this phone")
                        onUploadCredentialsChanged()
                        signedIn(AuthMachine.Event.SignedIn)
                    }
                }
            }
        }
    }

    // ---- unlock

    override fun unlockWithPassword(password: String) {
        scope.launch {
            operation.withLock {
                if (flow.value.busy) return@withLock
                val base = base()
                val username = store.username
                val secret = vault.readForPassword()
                if (base == null || username == null || secret == null || password.isEmpty()) {
                    if (base == null || username == null || secret == null) wipeLocal(AuthFailure.NoCredentials) else fail(AuthFailure.Incomplete)
                    return@withLock
                }
                flow.update { it.copy(busy = true, failure = null) }
                when (val result = api.unlock(base, username, password, secret)) {
                    is SessionResult.Failure -> fail(result.failure)
                    is SessionResult.Success -> {
                        web.install(base, result.cookie)
                        // A fingerprint enrolled after sign-in gets its copy now (the secret is at hand).
                        vault.addBiometricCopy(secret)
                        signedIn(AuthMachine.Event.Unlocked)
                    }
                }
            }
        }
    }

    override fun unlockWithFingerprint(activity: Activity) {
        scope.launch {
            val cipher = operation.withLock {
                if (flow.value.busy) return@withLock null
                vault.biometricCipher().also { if (it == null) refreshBiometric() }
            }
            if (cipher == null) return@launch
            BiometricGate.authenticate(
                activity = activity,
                cipher = cipher,
                title = context.getString(R.string.unlock_prompt_title),
                negativeText = context.getString(R.string.unlock_prompt_cancel),
            ) { outcome ->
                when (outcome) {
                    is BiometricGate.Outcome.Success -> scope.launch { resume(outcome.cipher) }
                    BiometricGate.Outcome.Cancelled -> Unit
                    BiometricGate.Outcome.Failed -> fail(AuthFailure.BiometricFailed)
                }
            }
        }
    }

    private suspend fun resume(cipher: javax.crypto.Cipher) = operation.withLock {
        val base = base()
        val secret = vault.readWithBiometric(cipher)
        if (base == null || secret == null) {
            fail(AuthFailure.BiometricFailed)
            return@withLock
        }
        flow.update { it.copy(busy = true, failure = null) }
        when (val result = api.resume(base, secret)) {
            is SessionResult.Failure -> {
                if (result.failure == AuthFailure.SecretRevoked) wipeLocal(AuthFailure.SecretRevoked) else fail(result.failure)
            }
            is SessionResult.Success -> {
                web.install(base, result.cookie)
                signedIn(AuthMachine.Event.Unlocked)
            }
        }
    }

    // ---- sign out

    override fun signOut() {
        scope.launch {
            operation.withLock {
                flow.update { it.copy(busy = true, failure = null) }
                val base = base()
                val cookie = base?.let(web::current)
                // With a live session the phone's token is revoked on the server too (uploads stop
                // for this account); locked or offline, only this phone forgets.
                if (base != null && cookie != null) api.signOutOnServer(base, cookie, store.deviceId)
                log.info("Signed out on this phone")
                wipeLocal(null)
            }
        }
    }

    // ---- state helpers (call under the operation lock)

    /** Forgets every credential and the web session; ends signed out (with [reason] when the server made us). */
    private suspend fun wipeLocal(reason: AuthFailure?) {
        val base = base()
        vault.clear()
        store.clear()
        settings.token = null
        web.clear(base)
        onUploadCredentialsChanged()
        flow.update {
            it.copy(
                phase = AuthMachine.next(it.phase, AuthMachine.Event.SignedOut, hasCredentials = false, hasServer = base != null),
                username = null,
                busy = false,
                failure = reason,
                biometricReady = false,
            )
        }
    }

    private fun move(event: AuthMachine.Event) {
        flow.update {
            val next = AuthMachine.next(it.phase, event, hasCredentials(), hasServer = base() != null)
            it.copy(phase = next, username = if (next == AuthPhase.SignedOut) null else store.username, busy = false)
        }
    }

    private fun signedIn(event: AuthMachine.Event) {
        flow.update {
            it.copy(
                phase = AuthMachine.next(it.phase, event, hasCredentials(), hasServer = true),
                username = store.username,
                busy = false,
                failure = null,
                biometricReady = vault.hasBiometricCopy(),
                sessionEpoch = it.sessionEpoch + 1,
            )
        }
    }

    private fun fail(failure: AuthFailure) {
        flow.update { it.copy(busy = false, failure = failure) }
    }

    private fun refreshBiometric() {
        val ready = hasCredentials() && vault.hasBiometricCopy() && vault.biometricsEnrolled() && vault.biometricCipher() != null
        flow.update { it.copy(biometricReady = ready) }
    }

    private companion object {
        const val PROBE_TIMEOUT_MS = 6_000
        const val PROBE_MIN_GAP_MS = 30_000L
    }
}
