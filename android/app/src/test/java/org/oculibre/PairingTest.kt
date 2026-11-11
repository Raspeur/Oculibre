package org.oculibre

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class PairingTest {
    private val fp = "ab".repeat(32)
    private val token = "3f0c9a2b7d1e4c5f8a6b0e2d9c7f1a3b"

    @Test
    fun parsesTheComputerLink() {
        // As desktop/cli/src/pairing.rs writes it.
        val o = PairingOffer.parse("oculibre://pair?v=1&n=raspeur%20PC&k=$fp&t=$token&a=192.168.1.50,100.1.2.3&p=47100")!!
        assertEquals("raspeur PC", o.name)
        assertEquals(fp, o.fingerprint)
        assertEquals(token, o.token)
        assertEquals(listOf("192.168.1.50", "100.1.2.3"), o.addresses)
        assertEquals(47100, o.port)
        assertEquals("abab abab abab abab", o.shortFingerprint)
    }

    @Test
    fun refusesBrokenOrUnknownLinks() {
        val ok = "oculibre://pair?v=1&k=$fp&t=$token&a=192.168.1.50"
        assertTrue(PairingOffer.parse(ok) != null)
        assertNull(PairingOffer.parse(ok.replace("v=1", "v=2"))) // a newer format
        assertNull(PairingOffer.parse(ok.replace(fp, "abcd"))) // fingerprint too short
        assertNull(PairingOffer.parse(ok.replace(token, "zz"))) // bad token
        assertNull(PairingOffer.parse(ok.replace("&a=192.168.1.50", ""))) // no address
        assertNull(PairingOffer.parse(ok.replace("a=192.168.1.50", "a=1.2.3.4;rm"))) // odd address
        assertNull(PairingOffer.parse("https://example.com/pair?v=1"))
        assertNull(PairingOffer.parse("$ok&p=99999"))
        assertEquals("computer", PairingOffer.parse(ok)!!.name)
    }

    @Test
    fun onlySafeTlsSuites() {
        assertTrue(SecureChannel.allowedSuite("TLS_AES_128_GCM_SHA256"))
        assertTrue(SecureChannel.allowedSuite("TLS_CHACHA20_POLY1305_SHA256"))
        assertTrue(SecureChannel.allowedSuite("TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256"))
        assertTrue(SecureChannel.allowedSuite("TLS_ECDHE_ECDSA_WITH_CHACHA20_POLY1305_SHA256"))
        assertTrue(!SecureChannel.allowedSuite("TLS_ECDHE_ECDSA_WITH_AES_128_CBC_SHA"))
        assertTrue(!SecureChannel.allowedSuite("TLS_RSA_WITH_AES_128_GCM_SHA256")) // no forward secrecy
        assertTrue(!SecureChannel.allowedSuite("TLS_ECDHE_PSK_WITH_CHACHA20_POLY1305_SHA256"))
        assertTrue(!SecureChannel.allowedSuite("TLS_EMPTY_RENEGOTIATION_INFO_SCSV"))
        assertTrue(SecureChannel.isLoopback("127.0.0.1") && !SecureChannel.isLoopback("192.168.1.50"))
    }
}

class DiscoveryTest {
    private val fp = "46b5ee534ffeee792fae09cfde631495702feb5a2e623fd01f1c258959457e5b"

    @Test
    fun recognisesThePairedComputer() {
        // TXT k as desktop/cli/src/discovery.rs writes it: the first 16 hex characters.
        assertTrue(Discovery.matches("46b5ee534ffeee79", fp))
        assertTrue(Discovery.matches("46B5EE534FFEEE79 ", fp))
        assertTrue(!Discovery.matches("46b5ee534ffeee70", fp)) // another computer
        assertTrue(!Discovery.matches("46b5", fp)) // too short to tell computers apart
        assertTrue(!Discovery.matches(null, fp))
    }
}

class NetworksTest {
    private val wifi = Networks.Subnet(Networks.parse("192.168.1.26")!!, 24)
    private val tailscale = Networks.Subnet(Networks.parse("100.64.0.0")!!, 10)

    @Test
    fun parsesOnlyIpv4Literals() {
        assertEquals(0xC0A80132.toInt(), Networks.parse("192.168.1.50"))
        assertNull(Networks.parse("192.168.1"))
        assertNull(Networks.parse("192.168.1.256"))
        assertNull(Networks.parse("pc.local"))
        assertNull(Networks.parse("::1"))
    }

    @Test
    fun theDirectWayAlwaysComesFirst() {
        val pc = listOf("100.112.121.63", "192.168.1.50")
        // Phone on the Wi-Fi only: the Wi-Fi address; the VPN one cannot be reached.
        assertEquals(listOf("192.168.1.50", "100.112.121.63"), Networks.rank(pc, listOf(wifi)))
        assertEquals(Networks.Route.UNKNOWN, Networks.route("100.112.121.63", listOf(wifi)))
        // Phone on the Wi-Fi and on Tailscale: still the Wi-Fi first, the VPN only after.
        assertEquals(listOf("192.168.1.50", "100.112.121.63"), Networks.rank(pc, listOf(wifi, tailscale)))
        assertEquals(Networks.Route.VPN, Networks.route("100.112.121.63", listOf(wifi, tailscale)))
        assertEquals(Networks.Route.DIRECT, Networks.route("192.168.1.50", listOf(wifi, tailscale)))
        // The field held the VPN address (used away from home): back home, direct first anyway.
        val known = listOf("100.112.121.63") + pc
        assertEquals(listOf("192.168.1.50", "100.112.121.63"), Networks.rank(known, listOf(wifi, tailscale)))
        // Away from home on Tailscale only: the VPN address, and no local network to search.
        assertEquals("100.112.121.63", Networks.rank(pc, listOf(tailscale)).first())
        assertTrue(!Networks.hasLocalNetwork(listOf(tailscale)) && Networks.hasLocalNetwork(listOf(wifi)))
    }

    @Test
    fun subnetBoundaries() {
        assertTrue(wifi.contains(Networks.parse("192.168.1.255")!!))
        assertTrue(!wifi.contains(Networks.parse("192.168.2.1")!!))
        assertTrue(Networks.Subnet(0, 0).contains(Networks.parse("8.8.8.8")!!))
    }
}
