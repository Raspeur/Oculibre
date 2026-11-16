package org.oculibre

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Test

/**
 * The vendored ZXing decoder reads the computer's pairing QR code from camera-like pictures:
 * 640×480 luminance, slightly blurred, straight or tilted (test/resources/qr, made with a QR
 * generator from a URI shaped like desktop/core pairing::uri).
 */
class QrDecoderTest {
    private fun frame(name: String): ByteArray =
        javaClass.classLoader!!.getResourceAsStream("qr/$name")!!.readBytes()

    private val expected: String =
        javaClass.classLoader!!.getResourceAsStream("qr/pairing.txt")!!.bufferedReader().readText().trim()

    @Test
    fun readsAStraightPairingCode() {
        val text = QrDecoder().decode(frame("pairing-straight-640x480.y"), 640, 480)
        assertEquals(expected, text)
        val offer = PairingOffer.parse(text!!)
        assertNotNull(offer)
        assertEquals("test-computer", offer!!.name)
        assertEquals(47100, offer.port)
    }

    @Test
    fun readsATiltedPairingCode() {
        assertEquals(expected, QrDecoder().decode(frame("pairing-tilted-640x480.y"), 640, 480))
    }

    @Test
    fun paddedRowsAreSkipped() {
        // Camera planes often have a row stride larger than the width.
        val tight = frame("pairing-straight-640x480.y")
        val stride = 704
        val padded = ByteArray(stride * 480)
        for (row in 0 until 480) System.arraycopy(tight, row * 640, padded, row * stride, 640)
        assertEquals(expected, QrDecoder().decode(padded, 640, 480, stride))
    }

    @Test
    fun noCodeGivesNull() {
        assertNull(QrDecoder().decode(ByteArray(640 * 480) { 120 }, 640, 480))
    }
}
