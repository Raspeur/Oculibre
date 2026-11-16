package org.oculibre

import com.google.zxing.BarcodeFormat
import com.google.zxing.BinaryBitmap
import com.google.zxing.DecodeHintType
import com.google.zxing.PlanarYUVLuminanceSource
import com.google.zxing.ReaderException
import com.google.zxing.common.HybridBinarizer
import com.google.zxing.qrcode.QRCodeReader

/**
 * Reads a QR code from a camera picture's luminance (the Y plane of YUV_420_888: brightness is all
 * a QR code needs). Pure Kotlin over the vendored ZXing decoder (third_party/zxing): unit-tested on
 * the JVM. Not thread-safe: one per scanning thread.
 */
class QrDecoder {
    private val reader = QRCodeReader()
    private val hints = mapOf(
        DecodeHintType.POSSIBLE_FORMATS to listOf(BarcodeFormat.QR_CODE),
        DecodeHintType.CHARACTER_SET to "UTF-8",
    )

    /**
     * The text of the QR code in this picture, or null. [rowStride]: bytes per row of [luminance]
     * (at least [width]; camera planes often pad their rows).
     */
    fun decode(luminance: ByteArray, width: Int, height: Int, rowStride: Int = width): String? {
        val source = PlanarYUVLuminanceSource(luminance, rowStride, height, 0, 0, width, height, false)
        return try {
            reader.decode(BinaryBitmap(HybridBinarizer(source)), hints).text
        } catch (e: ReaderException) {
            null // no QR code in this picture, or unreadable: try the next one
        } finally {
            reader.reset()
        }
    }
}
