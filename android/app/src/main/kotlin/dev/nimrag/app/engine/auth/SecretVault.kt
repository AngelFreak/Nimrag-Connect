package dev.nimrag.app.engine.auth

import android.content.Context
import android.content.SharedPreferences
import android.hardware.biometrics.BiometricManager
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyPermanentlyInvalidatedException
import android.security.keystore.KeyProperties
import android.util.Base64
import java.security.KeyFactory
import java.security.KeyPairGenerator
import java.security.KeyStore
import java.security.PrivateKey
import java.security.spec.MGF1ParameterSpec
import java.security.spec.X509EncodedKeySpec
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec
import javax.crypto.spec.OAEPParameterSpec
import javax.crypto.spec.PSource

/**
 * Where the trusted-device secret (`nrd_...`) lives on the phone: only ever encrypted, with keys
 * that never leave the Android Keystore. Two copies of the same secret:
 *
 *  - the **fingerprint copy**: RSA-OAEP under a Keystore key pair whose private key needs a
 *    `BIOMETRIC_STRONG` authentication for every use (`setUserAuthenticationRequired`, timeout 0,
 *    invalidated when a fingerprint is enrolled). The public key encrypts without a prompt, so
 *    nothing has to be touched at sign-in. Only a successful BiometricPrompt releases the secret.
 *  - the **password copy**: AES-GCM under a Keystore key without user authentication. It is read
 *    only to be sent together with the typed password to `/api/app/unlock`, so the password is the
 *    factor there. Without an enrolled fingerprint this is the only copy.
 *
 * The password copy is no defence against someone who can run code as this app (root): they could
 * read it and call `/api/app/resume`. The fingerprint copy only stops a person using the unlocked
 * phone without the fingerprint and without the password.
 *
 * Neither the secret nor the key material is ever logged or shown.
 */
class SecretVault(private val context: Context, private val prefs: SharedPreferences) {
    private val keyStore: KeyStore get() = KeyStore.getInstance(ANDROID_KEYSTORE).apply { load(null) }

    /** True when a fingerprint (strong biometrics) is enrolled and usable right now. */
    fun biometricsEnrolled(): Boolean {
        val manager = context.getSystemService(BiometricManager::class.java) ?: return false
        return manager.canAuthenticate(BiometricManager.Authenticators.BIOMETRIC_STRONG) == BiometricManager.BIOMETRIC_SUCCESS
    }

    /** True when a fingerprint copy exists (its key may still turn out to be invalidated). */
    fun hasBiometricCopy(): Boolean = prefs.contains(BIO_BLOB) && keyStore.containsAlias(BIO_ALIAS)

    fun hasSecret(): Boolean = prefs.contains(PLAIN_BLOB)

    /**
     * Stores [secret]: always the password copy, and the fingerprint copy when a fingerprint is
     * enrolled. Replaces earlier copies. Returns whether the fingerprint copy was made.
     */
    fun save(secret: String): Boolean {
        clear()
        val bytes = secret.toByteArray(Charsets.US_ASCII)
        val plain = encryptPlain(bytes)
        val bio = if (biometricsEnrolled()) runCatching { encryptForBiometric(bytes) }.getOrNull() else null
        val edit = prefs.edit()
        edit.putString(PLAIN_BLOB, plain)
        if (bio != null) edit.putString(BIO_BLOB, bio)
        edit.apply()
        return bio != null
    }

    /** Adds the fingerprint copy (after a password unlock, when a fingerprint was enrolled later). */
    fun addBiometricCopy(secret: String): Boolean {
        if (hasBiometricCopy() || !biometricsEnrolled()) return false
        val bio = runCatching { encryptForBiometric(secret.toByteArray(Charsets.US_ASCII)) }.getOrNull() ?: return false
        prefs.edit().putString(BIO_BLOB, bio).apply()
        return true
    }

    /** The secret for the password unlock; null when none is stored or the key is gone. */
    fun readForPassword(): String? {
        val blob = prefs.getString(PLAIN_BLOB, null) ?: return null
        return try {
            val raw = Base64.decode(blob, Base64.NO_WRAP)
            val key = keyStore.getKey(PLAIN_ALIAS, null) as? SecretKey ?: return null
            val cipher = Cipher.getInstance(AES_GCM)
            cipher.init(Cipher.DECRYPT_MODE, key, GCMParameterSpec(128, raw.copyOfRange(0, IV_LENGTH)))
            String(cipher.doFinal(raw, IV_LENGTH, raw.size - IV_LENGTH), Charsets.US_ASCII)
        } catch (_: Exception) {
            null
        }
    }

    /**
     * A decrypt cipher for the fingerprint copy, to be wrapped in a `BiometricPrompt.CryptoObject`.
     * Null when there is no fingerprint copy, or its key was invalidated (a fingerprint was
     * enrolled or removed): that copy is dropped then and the password unlock stays.
     */
    fun biometricCipher(): Cipher? {
        if (!hasBiometricCopy()) return null
        return try {
            val key = keyStore.getKey(BIO_ALIAS, null) as? PrivateKey ?: return null
            Cipher.getInstance(RSA_OAEP).also { it.init(Cipher.DECRYPT_MODE, key, OAEP) }
        } catch (_: KeyPermanentlyInvalidatedException) {
            dropBiometricCopy()
            null
        } catch (_: Exception) {
            null
        }
    }

    /** The secret from the fingerprint copy, with a cipher the BiometricPrompt has just authorised. */
    fun readWithBiometric(cipher: Cipher): String? {
        val blob = prefs.getString(BIO_BLOB, null) ?: return null
        return try {
            String(cipher.doFinal(Base64.decode(blob, Base64.NO_WRAP)), Charsets.US_ASCII)
        } catch (_: Exception) {
            null
        }
    }

    fun dropBiometricCopy() {
        prefs.edit().remove(BIO_BLOB).apply()
        runCatching { keyStore.deleteEntry(BIO_ALIAS) }
    }

    /** Deletes both copies and both keys. */
    fun clear() {
        prefs.edit().remove(PLAIN_BLOB).remove(BIO_BLOB).apply()
        runCatching { keyStore.deleteEntry(PLAIN_ALIAS) }
        runCatching { keyStore.deleteEntry(BIO_ALIAS) }
    }

    // ---- copies

    private fun encryptPlain(bytes: ByteArray): String {
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, ANDROID_KEYSTORE)
        generator.init(
            KeyGenParameterSpec.Builder(PLAIN_ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .build(),
        )
        val key = generator.generateKey()
        val cipher = Cipher.getInstance(AES_GCM)
        cipher.init(Cipher.ENCRYPT_MODE, key)
        val iv = cipher.iv
        check(iv.size == IV_LENGTH)
        return Base64.encodeToString(iv + cipher.doFinal(bytes), Base64.NO_WRAP)
    }

    private fun encryptForBiometric(bytes: ByteArray): String {
        val generator = KeyPairGenerator.getInstance(KeyProperties.KEY_ALGORITHM_RSA, ANDROID_KEYSTORE)
        generator.initialize(
            KeyGenParameterSpec.Builder(BIO_ALIAS, KeyProperties.PURPOSE_DECRYPT)
                .setKeySize(2048)
                .setDigests(KeyProperties.DIGEST_SHA256, KeyProperties.DIGEST_SHA1)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_RSA_OAEP)
                .setUserAuthenticationRequired(true)
                .setUserAuthenticationParameters(0, KeyProperties.AUTH_BIOMETRIC_STRONG)
                .setInvalidatedByBiometricEnrollment(true)
                .build(),
        )
        val pair = generator.generateKeyPair()
        // The Keystore would ask for a fingerprint to *use* this key pair's public half too on some
        // devices; a plain public key built from the encoded form is not restricted.
        val publicKey = KeyFactory.getInstance(KeyProperties.KEY_ALGORITHM_RSA).generatePublic(X509EncodedKeySpec(pair.public.encoded))
        val cipher = Cipher.getInstance(RSA_OAEP)
        cipher.init(Cipher.ENCRYPT_MODE, publicKey, OAEP)
        return Base64.encodeToString(cipher.doFinal(bytes), Base64.NO_WRAP)
    }

    private companion object {
        const val ANDROID_KEYSTORE = "AndroidKeyStore"
        const val PLAIN_ALIAS = "nimrag_device_secret_plain"
        const val BIO_ALIAS = "nimrag_device_secret_biometric"
        const val PLAIN_BLOB = "secret_plain"
        const val BIO_BLOB = "secret_biometric"
        const val AES_GCM = "AES/GCM/NoPadding"
        const val RSA_OAEP = "RSA/ECB/OAEPWithSHA-256AndMGF1Padding"
        const val IV_LENGTH = 12

        /** The Keystore's RSA-OAEP uses MGF1 with SHA-1 unless told otherwise; both sides must agree. */
        val OAEP = OAEPParameterSpec("SHA-256", "MGF1", MGF1ParameterSpec.SHA1, PSource.PSpecified.DEFAULT)
    }
}
