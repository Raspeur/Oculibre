package org.oculibre

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test
import java.io.ByteArrayInputStream
import java.io.File

/**
 * Shared conformance vectors (docs/protocol.typ §16): the same files are checked by the Rust crate
 * (desktop/protocol/tests/vectors.rs), so the phone and the computer cannot drift apart.
 */
class ProtocolVectorsTest {
    private val dir: File = run {
        // Gradle runs unit tests from the module directory (android/app); allow an override.
        val candidates = listOfNotNull(
            System.getProperty("oculibre.vectors")?.let(::File),
            File("../../tests/protocol/vectors"),
            File("tests/protocol/vectors"),
        )
        candidates.firstOrNull { it.isDirectory } ?: error("test vectors not found in $candidates")
    }

    private fun hex(s: String): ByteArray {
        val t = s.trim()
        return ByteArray(t.length / 2) { t.substring(2 * it, 2 * it + 2).toInt(16).toByte() }
    }

    private fun toHex(b: ByteArray) = b.joinToString("") { "%02x".format(it.toInt() and 0xFF) }

    @Suppress("UNCHECKED_CAST")
    private fun fields(m: Protocol.Message): Map<String, Any?> = when (m) {
        is Protocol.Message.Hello -> m.toJson()
        is Protocol.Message.Bye -> m.toJson()
        is Protocol.Message.StreamConfig -> m.config.toJson()
        is Protocol.Message.StreamEnd -> mapOf("reason" to m.reason)
        is Protocol.Message.Request -> m.toJson()
        is Protocol.Message.Response -> m.toJson()
        is Protocol.Message.Event -> m.toJson()
        is Protocol.Message.Ping -> mapOf("seq" to m.seq, "t1" to m.t1)
        is Protocol.Message.Pong -> mapOf("seq" to m.seq, "t1" to m.t1, "t2" to m.t2, "t3" to m.t3)
        is Protocol.Message.VideoFrame -> mapOf(
            "frame_number" to m.frameNumber, "config_id" to m.configId.toLong(), "key" to m.key,
            "discontinuity" to m.discontinuity, "data_hex" to toHex(m.data),
        )
        is Protocol.Message.Unknown -> mapOf("skipped_length" to m.length)
    }

    @Test
    fun everyVectorDecodesToItsDescription() {
        val files = dir.listFiles { f -> f.name.endsWith(".hex") }!!.sortedBy { it.name }
        assertTrue("only ${files.size} vectors found in $dir", files.size >= 25)
        for (f in files) {
            val name = f.name.removeSuffix(".hex")
            val bytes = hex(f.readText())
            @Suppress("UNCHECKED_CAST")
            val desc = Json.parse(File(dir, "$name.json").readText()) as Map<String, Any?>
            val reader = Protocol.MessageReader(ByteArrayInputStream(bytes))
            when (desc["kind"]) {
                "preamble" -> {
                    val p = reader.readPreamble()
                    assertEquals(name, desc["major"], p.major.toLong())
                    assertEquals(name, desc["minor"], p.minor.toLong())
                    assertEquals(name, desc["role"], p.roleName)
                    assertArrayEquals("$name: re-encoding differs", bytes, p.encode())
                }
                "message" -> {
                    val m = try {
                        reader.readMessage()
                    } catch (e: Protocol.ProtocolException) {
                        throw AssertionError("$name: ${e.kind}: ${e.message}", e)
                    }
                    assertEquals("$name: decoded fields differ", desc["fields"], fields(m))
                    @Suppress("UNCHECKED_CAST")
                    val h = desc["header"] as Map<String, Any?>
                    val header = Protocol.Header.decode(bytes.copyOfRange(0, Protocol.HEADER_LEN))
                    assertEquals(name, h["type"], header.type.toLong())
                    assertEquals(name, h["flags"], header.flags.toLong())
                    assertEquals(name, h["stream"], header.stream.toLong())
                    assertEquals(name, h["length"], header.length)
                    assertEquals(name, h["timestamp"], header.timestamp)
                    assertEquals(name, desc["type"], Protocol.Type.name(m.type).takeIf { m !is Protocol.Message.Unknown } ?: "UNKNOWN")
                    if (m !is Protocol.Message.Unknown && name != "hello_future_keys") {
                        assertEquals("$name: re-encoding differs", toHex(bytes), toHex(Protocol.encode(m)))
                    }
                }
                "error" -> {
                    try {
                        if (desc["stage"] == "preamble") reader.readPreamble() else reader.readMessage()
                        fail("$name: no error")
                    } catch (e: Protocol.ProtocolException) {
                        assertEquals("$name: ${e.message}", desc["expect_error"], e.kind)
                    }
                }
                else -> fail("$name: unknown kind ${desc["kind"]}")
            }
        }
    }
}
