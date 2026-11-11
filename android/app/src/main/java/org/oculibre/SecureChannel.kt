package org.oculibre

import android.annotation.TargetApi
import android.content.Context
import android.os.Build
import android.security.KeyPairGeneratorSpec
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import java.math.BigInteger
import java.net.Socket
import java.security.KeyPairGenerator
import java.security.KeyStore
import java.security.MessageDigest
import java.security.Principal
import java.security.PrivateKey
import java.security.SecureRandom
import java.security.cert.CertificateException
import java.security.cert.X509Certificate
import java.security.spec.ECGenParameterSpec
import java.util.Date
import javax.net.ssl.SSLContext
import javax.net.ssl.SSLSocket
import javax.net.ssl.X509KeyManager
import javax.net.ssl.X509TrustManager
import javax.security.auth.x500.X500Principal

/**
 * Secure channel of Wi-Fi connections (protocol §13, docs/research/secure-channel.typ): the
 * platform's TLS, 1.3 when the phone has it (Android 10+), else 1.2 restricted to ECDHE + AEAD
 * suites. No certificate authority: the phone pins the computer's certificate fingerprint from the
 * pairing QR code, and authenticates with its own self-signed certificate, whose private key never
 * leaves the Android Keystore.
 */
object SecureChannel {
    private const val KEYSTORE = "AndroidKeyStore"
    private const val ALIAS = "oculibre-tls"

    /** Thrown when the computer is not the one this phone is paired with. */
    class WrongComputer : CertificateException("not the paired computer")

    /** USB through `adb reverse` (loopback) stays plaintext; everything else is a network. */
    fun isLoopback(host: String): Boolean =
        host == "localhost" || host == "::1" || host.startsWith("127.")

    private fun keyStore(): KeyStore = KeyStore.getInstance(KEYSTORE).apply { load(null) }

    /** Creates the phone's TLS key and self-signed certificate once (ECDSA P-256). */
    private fun ensureKey(context: Context): KeyStore {
        val ks = keyStore()
        if (ks.containsAlias(ALIAS)) return ks
        if (Build.VERSION.SDK_INT >= 23) generateModern() else generateLegacy(context)
        return keyStore()
    }

    @TargetApi(23)
    private fun generateModern() {
        val g = KeyPairGenerator.getInstance(KeyProperties.KEY_ALGORITHM_EC, KEYSTORE)
        g.initialize(
            KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_SIGN)
                .setAlgorithmParameterSpec(ECGenParameterSpec("secp256r1"))
                // TLS 1.2 signs with NONEwithECDSA on some versions: allow every digest.
                .setDigests(
                    KeyProperties.DIGEST_NONE, KeyProperties.DIGEST_SHA256,
                    KeyProperties.DIGEST_SHA384, KeyProperties.DIGEST_SHA512,
                )
                .setCertificateSubject(X500Principal("CN=Oculibre phone"))
                .setCertificateSerialNumber(BigInteger.ONE)
                .setCertificateNotBefore(Date(0))
                .setCertificateNotAfter(Date(NOT_AFTER_MS))
                .build(),
        )
        g.generateKeyPair()
    }

    /** Android 5.x (API 21–22). Not tested on a real device yet (#5 spike). */
    @Suppress("DEPRECATION")
    private fun generateLegacy(context: Context) {
        val spec = KeyPairGeneratorSpec.Builder(context)
            .setAlias(ALIAS)
            .setKeyType("EC")
            .setKeySize(256)
            .setSubject(X500Principal("CN=Oculibre phone"))
            .setSerialNumber(BigInteger.ONE)
            .setStartDate(Date(0))
            .setEndDate(Date(NOT_AFTER_MS))
            .build()
        val g = runCatching { KeyPairGenerator.getInstance("EC", KEYSTORE) }
            .getOrElse { KeyPairGenerator.getInstance("RSA", KEYSTORE) }
        g.initialize(spec)
        g.generateKeyPair()
    }

    fun sha256Hex(bytes: ByteArray): String =
        MessageDigest.getInstance("SHA-256").digest(bytes).joinToString("") { "%02x".format(it.toInt() and 0xFF) }

    /** This phone's certificate fingerprint (what the computer remembers once paired). */
    fun fingerprint(context: Context): String =
        sha256Hex(ensureKey(context).getCertificate(ALIAS).encoded)

    /**
     * TLS 1.3 suites, and for TLS 1.2 ECDHE (forward secrecy) signed with ECDSA (the computer's key)
     * with AES-GCM or ChaCha20-Poly1305: no CBC, no RSA key exchange, no PSK.
     */
    fun allowedSuite(name: String): Boolean =
        name.startsWith("TLS_AES_") || name.startsWith("TLS_CHACHA20_") ||
            (name.startsWith("TLS_ECDHE_ECDSA_WITH_") && (name.contains("_GCM_") || name.contains("CHACHA20")))

    /**
     * Runs the TLS handshake over the connected [plain] socket. Throws an `SSLException` whose
     * cause is [WrongComputer] if the computer's certificate is not [pinnedFingerprint].
     */
    fun connect(context: Context, plain: Socket, host: String, port: Int, pinnedFingerprint: String): SSLSocket {
        val ks = ensureKey(context)
        val key = ks.getKey(ALIAS, null) as PrivateKey
        val chain = arrayOf(ks.getCertificate(ALIAS) as X509Certificate)

        val keyManager = object : X509KeyManager {
            override fun chooseClientAlias(keyType: Array<out String>?, issuers: Array<out Principal>?, socket: Socket?) = ALIAS
            override fun getClientAliases(keyType: String?, issuers: Array<out Principal>?) = arrayOf(ALIAS)
            override fun getCertificateChain(alias: String?) = chain
            override fun getPrivateKey(alias: String?) = key
            override fun chooseServerAlias(keyType: String?, issuers: Array<out Principal>?, socket: Socket?): String? = null
            override fun getServerAliases(keyType: String?, issuers: Array<out Principal>?): Array<String>? = null
        }
        val trustManager = object : X509TrustManager {
            override fun checkServerTrusted(chain: Array<out X509Certificate>?, authType: String?) {
                val leaf = chain?.firstOrNull() ?: throw WrongComputer()
                if (sha256Hex(leaf.encoded) != pinnedFingerprint) throw WrongComputer()
            }

            override fun checkClientTrusted(chain: Array<out X509Certificate>?, authType: String?) =
                throw CertificateException("the phone is never a TLS server")

            override fun getAcceptedIssuers(): Array<X509Certificate> = emptyArray()
        }
        val tls = SSLContext.getInstance("TLS")
        tls.init(arrayOf(keyManager), arrayOf(trustManager), SecureRandom())
        val socket = tls.socketFactory.createSocket(plain, host, port, true) as SSLSocket
        socket.enabledProtocols = socket.supportedProtocols.filter { it == "TLSv1.3" || it == "TLSv1.2" }.toTypedArray()
        socket.enabledCipherSuites = socket.supportedCipherSuites.filter(::allowedSuite).toTypedArray()
        socket.startHandshake()
        return socket
    }

    /** "TLSv1.3" or "TLSv1.2". */
    fun version(socket: SSLSocket): String = socket.session?.protocol ?: "TLS"

    private const val NOT_AFTER_MS = 4_102_444_800_000L // 2100-01-01
}
