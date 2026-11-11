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
