package dev.nimrag.app.engine.auth

import android.app.Activity
import android.hardware.biometrics.BiometricManager
import android.hardware.biometrics.BiometricPrompt
import android.os.CancellationSignal
import java.util.concurrent.atomic.AtomicBoolean
import javax.crypto.Cipher

/**
 * The system fingerprint prompt (platform `BiometricPrompt`, API 30+ for the authenticator
 * flags; the app's minSdk is 31) with a Keystore-backed [Cipher]: the cipher only works when the
 * prompt succeeds, which is what releases the device secret ([SecretVault.biometricCipher]).
 * Only `BIOMETRIC_STRONG` counts; there is no device-credential fallback (the password unlock is
 * the other way in).
 */
object BiometricGate {
    sealed interface Outcome {
        /** The prompt succeeded; [cipher] can decrypt now. */
        class Success(val cipher: Cipher) : Outcome

        /** The person backed out (back, the "Use password" button, a tap outside). */
        data object Cancelled : Outcome

        /** Too many wrong tries, a lock-out, no sensor, a changed fingerprint set. */
        data object Failed : Outcome
    }

    fun authenticate(
        activity: Activity,
        cipher: Cipher,
        title: String,
        negativeText: String,
        onOutcome: (Outcome) -> Unit,
    ) {
        val executor = activity.mainExecutor
        // The negative button also arrives as an error: report only the first outcome.
        val reported = AtomicBoolean(false)
        val finish = { outcome: Outcome -> if (reported.compareAndSet(false, true)) onOutcome(outcome) }
        val prompt = BiometricPrompt.Builder(activity)
            .setTitle(title)
            .setAllowedAuthenticators(BiometricManager.Authenticators.BIOMETRIC_STRONG)
            .setNegativeButton(negativeText, executor) { _, _ -> finish(Outcome.Cancelled) }
            .build()
        val callback = object : BiometricPrompt.AuthenticationCallback() {
            override fun onAuthenticationSucceeded(result: BiometricPrompt.AuthenticationResult) {
                val unlocked = result.cryptoObject?.cipher
                finish(if (unlocked != null) Outcome.Success(unlocked) else Outcome.Failed)
            }

            override fun onAuthenticationError(errorCode: Int, errString: CharSequence?) {
                val cancelled = errorCode == BiometricPrompt.BIOMETRIC_ERROR_USER_CANCELED ||
                    errorCode == BiometricPrompt.BIOMETRIC_ERROR_CANCELED
                finish(if (cancelled) Outcome.Cancelled else Outcome.Failed)
            }
        }
        try {
            prompt.authenticate(BiometricPrompt.CryptoObject(cipher), CancellationSignal(), executor, callback)
        } catch (_: Exception) {
            finish(Outcome.Failed)
        }
    }
}
