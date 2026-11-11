package org.oculibre

import java.net.Inet4Address
import java.net.NetworkInterface

/**
 * The phone's own IPv4 networks, to choose among several addresses of the computer the ones it can
 * reach: one on the same Wi-Fi first. The computer may also have VPN or container addresses.
 *
 * Pure JVM (java.net only): the choice is unit-tested.
 */
object Networks {
    /** An IPv4 network: address and prefix length (`192.168.1.26/24`). */
    data class Subnet(val address: Int, val prefix: Int) {
        fun contains(ip: Int): Boolean {
            val mask = if (prefix == 0) 0 else -1 shl (32 - prefix)
            return (ip and mask) == (address and mask)
        }
    }

    /** Tailscale and other carrier-grade NAT VPNs: 100.64.0.0/10. */
    private val CGNAT = Subnet(parse("100.64.0.0")!!, 10)

    /** `a.b.c.d` → 32-bit value, null for anything else (names, IPv6). */
    fun parse(ip: String): Int? {
        val parts = ip.trim().split('.')
        if (parts.size != 4) return null
        var value = 0
        for (p in parts) {
            val n = p.toIntOrNull() ?: return null
            if (n !in 0..255 || p.length > 3) return null
            value = (value shl 8) or n
        }
        return value
    }

    /** Networks of the phone's active interfaces (loopback left out). */
    fun local(): List<Subnet> = runCatching {
        NetworkInterface.getNetworkInterfaces()?.toList().orEmpty()
            .filter { it.isUp && !it.isLoopback }
            .flatMap { it.interfaceAddresses }
            .mapNotNull { a ->
                val v4 = a.address as? Inet4Address ?: return@mapNotNull null
                val ip = parse(v4.hostAddress ?: "") ?: return@mapNotNull null
                // A VPN address is often a /32: what matters is the VPN's whole range.
                if (CGNAT.contains(ip)) CGNAT else Subnet(ip, a.networkPrefixLength.toInt().coerceIn(0, 32))
            }
    }.getOrDefault(emptyList())

    /** How the phone would reach an address, best first. */
    enum class Route {
        /** On one of the phone's local networks (Wi-Fi, Ethernet): the fastest. */
        DIRECT,
        /** Through a VPN the phone is on (Tailscale): works from anywhere, slower. */
        VPN,
        /** On none of the phone's networks: maybe routed, maybe not. */
        UNKNOWN,
    }

    fun route(ip: String, subnets: List<Subnet>): Route {
        val v = parse(ip) ?: return Route.UNKNOWN
        return when {
            subnets.any { it != CGNAT && it.contains(v) } -> Route.DIRECT
            CGNAT in subnets && CGNAT.contains(v) -> Route.VPN
            else -> Route.UNKNOWN
        }
    }

    /** Is the phone on a local network (where mDNS can find the computer)? */
    fun hasLocalNetwork(subnets: List<Subnet>): Boolean = subnets.any { it != CGNAT }

    /**
     * [candidates] without duplicates, direct ones first, then through a VPN, then the others; the
     * given order is kept within each group.
     */
    fun rank(candidates: List<String>, subnets: List<Subnet>): List<String> =
        candidates.distinct().sortedBy { route(it, subnets).ordinal }
}
