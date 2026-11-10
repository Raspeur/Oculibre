package org.oculibre

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test
import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream

class ProtocolTest {
    private fun kindOf(block: () -> Unit): String = try {
        block()
        "none"
    } catch (e: Protocol.ProtocolException) {
        e.kind
    }

    @Test
    fun streamOfMessagesThenCleanClose() {
        val out = ByteArrayOutputStream()
        out.write(Protocol.Preamble.ours(Protocol.ROLE_HOST).encode())
        out.write(Protocol.encode(Protocol.Message.Ping(1, 2)))
        out.write(Protocol.encode(Protocol.Message.Unknown(0x7A, 0, 0)).also {
            it[7] = 5 // announce 5 payload bytes
        })
        out.write(ByteArray(5) { 9 })
        out.write(Protocol.encode(Protocol.Message.Request(3, "request_keyframe")))
        val r = Protocol.MessageReader(ByteArrayInputStream(out.toByteArray()))
        val p = r.readPreamble()
        Protocol.Preamble.ours(Protocol.ROLE_CAMERA).checkPeer(p)
        assertEquals(Protocol.Message.Ping(1, 2), r.readMessage())
        assertEquals(Protocol.Message.Unknown(0x7A, 0, 5), r.readMessage())
        assertEquals(Protocol.Message.Request(3, "request_keyframe"), r.readMessage())
        assertEquals("connection_closed", kindOf { r.readMessage() })
    }

    @Test
    fun truncatedMessageIsIo() {
        val b = Protocol.encode(Protocol.Message.Ping(1, 2)).copyOf(20)
        assertEquals("io", kindOf { Protocol.MessageReader(ByteArrayInputStream(b)).readMessage() })
    }

    @Test
    fun peerChecks() {
        val cam = Protocol.Preamble.ours(Protocol.ROLE_CAMERA)
        assertEquals("same_role", kindOf { cam.checkPeer(cam) })
        assertEquals("incompatible_major", kindOf { cam.checkPeer(Protocol.Preamble(1, 0, Protocol.ROLE_HOST)) })
        cam.checkPeer(Protocol.Preamble(0, 9, Protocol.ROLE_HOST)) // newer minor is fine
    }

    @Test
    fun videoFrameFromSlice() {
        val buf = byteArrayOf(7, 7, 0, 0, 0, 1, 0x65, 7)
        val bytes = Protocol.encodeVideoFrame(1, 99, true, true, 0xFFFF_FFFFL, 65535, buf, 2, 5)
        val m = Protocol.MessageReader(ByteArrayInputStream(bytes)).readMessage() as Protocol.Message.VideoFrame
        assertEquals(99L, m.timestamp)
        assertTrue(m.key && m.discontinuity)
        assertEquals(0xFFFF_FFFFL, m.frameNumber)
        assertEquals(65535, m.configId)
        assertArrayEquals(byteArrayOf(0, 0, 0, 1, 0x65), m.data)
    }

    @Test
    fun jsonFieldTypesAreChecked() {
        fun decode(type: Int, json: String) = kindOf {
            val p = json.toByteArray()
            Protocol.decode(Protocol.Header(type, length = p.size.toLong()), p)
        }
        assertEquals("json", decode(Protocol.Type.HELLO, "{\"app\": 3}"))
        assertEquals("json", decode(Protocol.Type.HELLO, "{\"features\": [1]}"))
        assertEquals("json", decode(Protocol.Type.HELLO, "null"))
        assertEquals("json", decode(Protocol.Type.HELLO, "{} x"))
        assertEquals("json", decode(Protocol.Type.REQUEST, "{\"id\": -1}"))
        assertEquals("json", decode(Protocol.Type.REQUEST, "{\"id\": 4294967296}"))
        assertEquals("json", decode(Protocol.Type.STREAM_CONFIG, "{\"config_id\": 65536}"))
        assertEquals("json", decode(Protocol.Type.STREAM_CONFIG, "{\"width\": 1.5}"))
        assertEquals("none", decode(Protocol.Type.STREAM_CONFIG, "{\"capture_fps\": null, \"x\": {\"y\": [1]}}"))
        assertEquals("json", decode(Protocol.Type.HELLO, "[".repeat(100) + "]".repeat(100)))
        val bad = byteArrayOf('{'.code.toByte(), 0xFF.toByte(), '}'.code.toByte())
        assertEquals("json", kindOf { Protocol.decode(Protocol.Header(Protocol.Type.EVENT, length = 3), bad) })
    }

    @Test
    fun defaultsApplied() {
        val p = "{\"config_id\":2,\"width\":640}".toByteArray()
        val m = Protocol.decode(Protocol.Header(Protocol.Type.STREAM_CONFIG, stream = 1, length = p.size.toLong()), p)
            as Protocol.Message.StreamConfig
        assertEquals("h264", m.config.codec)
        assertEquals(Protocol.Color(), m.config.color)
        assertNull(m.config.camera)
    }

    @Test
    fun jsonWriterMatchesSerde() {
        assertEquals("{\"a\":\"x\\\"\\\\\\n\\u0001é/\",\"b\":[1,2.5,true,null]}",
            Json.write(linkedMapOf("a" to "x\"\\\n\u0001é/", "b" to listOf(1L, 2.5, true, null))))
        assertEquals("{\"a\":1,\"b\":{\"c\":2,\"d\":3}}",
            Json.write(Json.sorted(mapOf("b" to mapOf("d" to 3L, "c" to 2L), "a" to 1L))))
        assertEquals(mapOf("k" to "é€"), Json.parse("{\"k\":\"\\u00e9\\u20ac\"}"))
    }

    @Test
    fun base64() {
        for (n in 0..10) {
            val b = ByteArray(n) { (it * 37 + 200).toByte() }
            assertArrayEquals(b, Base64Codec.decode(Base64Codec.encode(b)))
        }
        assertEquals("AAAAAWc=", Base64Codec.encode(byteArrayOf(0, 0, 0, 1, 0x67)))
        assertNull(Base64Codec.decode("AAA"))
        assertNull(Base64Codec.decode("A*AA"))
    }

    @Test
    fun clock() {
        // Responder clock 1000 µs ahead, 10 µs each way, 5 µs processing.
        assertEquals(Protocol.ClockSample(1000, 20), Protocol.clockSample(100, 1110, 1115, 125))
    }

    @Test
    fun responseHelpers() {
        val ok = Protocol.encode(Protocol.Message.Response.ok(7))
        assertEquals("{\"id\":7,\"ok\":true,\"result\":{}}", String(ok, Protocol.HEADER_LEN, ok.size - Protocol.HEADER_LEN))
        val err = Protocol.Message.Response.error(7, Protocol.ErrorCode.UNSUPPORTED)
        val bytes = Protocol.encode(err)
        val back = Protocol.MessageReader(ByteArrayInputStream(bytes)).readMessage()
        if (back != err) fail("$back != $err")
    }
}
