package org.oculibre

import android.content.Context
import java.net.URLDecoder

/**
 * What the computer's pairing QR code carries (protocol §13):
 * `oculibre://pair?v=1&n=<name>&k=<fingerprint>&t=<token>&a=<ip>,<ip>&p=<port>`.
 *
 * Pure Kotlin (no Android class): parsed in JVM unit tests.
 */
data class PairingOffer(
    /** The computer's name, for people. */
    val name: String,
    /** SHA-256 of the computer's TLS certificate, lowercase hex: what the phone pins. */
    val fingerprint: String,
    /** One-time token, sent once in HELLO inside TLS. */
    val token: String,
    val addresses: List<String>,
    val port: Int,
) {
    /** `a1b2 c3d4 e5f6 a7b8`, as `oculibre` prints it. */
    val shortFingerprint: String get() = shortFingerprint(fingerprint)

    companion object {
        private const val PREFIX = "oculibre://pair?"
        private val HEX64 = Regex("[0-9a-f]{64}")
        private val TOKEN = Regex("[0-9a-f]{16,128}")
        private val ADDRESS = Regex("[0-9A-Za-z.:%-]{1,64}")

        fun shortFingerprint(fp: String) = fp.take(16).chunked(4).joinToString(" ")

        /** Null if [uri] is not a valid pairing link of a version we know. */
        fun parse(uri: String): PairingOffer? {
            if (!uri.startsWith(PREFIX, ignoreCase = true)) return null
            val params = uri.substring(PREFIX.length).split('&').mapNotNull { kv ->
                val i = kv.indexOf('=')
                if (i <= 0) null else kv.substring(0, i) to decode(kv.substring(i + 1))
            }.toMap()
            if (params["v"] != "1") return null
            val fingerprint = params["k"]?.lowercase()?.takeIf { HEX64.matches(it) } ?: return null
            val token = params["t"]?.lowercase()?.takeIf { TOKEN.matches(it) } ?: return null
            val addresses = params["a"]?.split(',')?.map { it.trim() }?.filter { ADDRESS.matches(it) }
                ?.takeIf { it.isNotEmpty() } ?: return null
            val port = params["p"]?.toIntOrNull() ?: Protocol.HOST_PORT
            if (port !in 1..65535) return null
            val name = params["n"]?.trim()?.take(64)?.ifEmpty { null } ?: "computer"
            return PairingOffer(name, fingerprint, token, addresses, port)
        }

        private fun decode(s: String): String? = runCatching { URLDecoder.decode(s, "UTF-8") }.getOrNull()
    }
}

/** The computer this phone is paired with (one at a time), in the app's private preferences. */
data class PairedComputer(
    val name: String,
    val fingerprint: String,
    /** Still to send in the next HELLO; cleared once the computer accepted it. */
    val token: String?,
    val addresses: List<String>,
)

object PairingStore {
    private const val PREFS = "oculibre"

    private fun prefs(context: Context) = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)

    fun get(context: Context): PairedComputer? {
        val p = prefs(context)
        val fp = p.getString("pair_fingerprint", null) ?: return null
        return PairedComputer(
            name = p.getString("pair_name", null) ?: "computer",
            fingerprint = fp,
            token = p.getString("pair_token", null),
            addresses = p.getString("pair_addresses", null)?.split(',')?.filter { it.isNotEmpty() } ?: emptyList(),
        )
    }

    fun save(context: Context, offer: PairingOffer) {
        prefs(context).edit()
            .putString("pair_name", offer.name)
            .putString("pair_fingerprint", offer.fingerprint)
            .putString("pair_token", offer.token)
            .putString("pair_addresses", offer.addresses.joinToString(","))
            .apply()
    }

    /** The computer was found at [address] (mDNS): use it from now on. */
    fun rememberAddress(context: Context, address: String) {
        val p = prefs(context)
        val known = p.getString("pair_addresses", null)?.split(',')?.filter { it.isNotEmpty() && it != address }
            ?: emptyList()
        p.edit()
            .putString("pair_addresses", (listOf(address) + known).joinToString(","))
            .putString(HOST_PREF, address) // the address field of MainActivity
            .apply()
    }

    /** Key of the address field in the app's preferences (shared with MainActivity). */
    const val HOST_PREF = "host"

    fun clearToken(context: Context) {
        prefs(context).edit().remove("pair_token").apply()
    }

    fun forget(context: Context) {
        prefs(context).edit()
            .remove("pair_name").remove("pair_fingerprint").remove("pair_token").remove("pair_addresses")
            .apply()
    }
}
