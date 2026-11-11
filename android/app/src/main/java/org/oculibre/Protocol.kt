package org.oculibre

import java.io.EOFException
import java.io.IOException
import java.io.InputStream
import java.io.OutputStream

/**
 * Oculibre wire protocol 0.3, camera side. The specification is docs/protocol.typ; the host side
 * is the Rust crate desktop/protocol. Both pass the shared vectors of tests/protocol/vectors.
 *
 * Pure Kotlin/JVM: no Android class here, so the unit tests run on the build machine.
 */
object Protocol {
    const val MAJOR = 0
    const val MINOR = 3
    const val HOST_PORT = 47100

    const val PREAMBLE_LEN = 8
    const val HEADER_LEN = 16
    const val VIDEO_SUBHEADER_LEN = 8
    const val MAX_MESSAGE_LEN = 16L * 1024 * 1024
    const val MAX_JSON_LEN = 64 * 1024
    private val MAGIC = byteArrayOf(0x4F, 0x43, 0x55, 0x4C) // "OCUL"

    const val ROLE_CAMERA = 1
    const val ROLE_HOST = 2

    object Type {
        const val HELLO = 0x01
        const val BYE = 0x02
        const val PING = 0x03
        const val PONG = 0x04
        const val STREAM_CONFIG = 0x20
        const val VIDEO_FRAME = 0x21
        const val STREAM_END = 0x23
        const val REQUEST = 0x40
        const val RESPONSE = 0x41
        const val EVENT = 0x42

        fun isJson(t: Int) = t == HELLO || t == BYE || t == STREAM_CONFIG || t == STREAM_END ||
            t == REQUEST || t == RESPONSE || t == EVENT

        fun isDecoded(t: Int) = isJson(t) || t == PING || t == PONG || t == VIDEO_FRAME

        fun name(t: Int) = when (t) {
            HELLO -> "HELLO"; BYE -> "BYE"; PING -> "PING"; PONG -> "PONG"
            STREAM_CONFIG -> "STREAM_CONFIG"; VIDEO_FRAME -> "VIDEO_FRAME"; 0x22 -> "AUDIO_FRAME"
            STREAM_END -> "STREAM_END"; REQUEST -> "REQUEST"; RESPONSE -> "RESPONSE"; EVENT -> "EVENT"
            else -> "UNKNOWN"
        }
    }

    const val FLAG_KEY = 0x01
    const val FLAG_DISCONTINUITY = 0x02

    const val STREAM_SESSION = 0
    const val STREAM_VIDEO = 1

    object Feature {
        const val VIDEO_H264 = "video.h264"
        const val CLOCK = "clock"
        const val ORIENTATION = "orientation"
        const val STATS = "stats"
        const val CONTROL_KEYFRAME = "control.keyframe"
        const val CONTROL_BITRATE = "control.bitrate"
        /** set_mode (since 0.2). */
        const val CONTROL_MODE = "control.mode"
    }

    object ErrorCode {
        const val UNKNOWN_COMMAND = "unknown_command"
        const val UNSUPPORTED = "unsupported"
        const val INVALID_ARGS = "invalid_args"
        const val BUSY = "busy"
        const val FAILED = "failed"
    }

    /**
     * Everything that can go wrong while reading. [kind] is the stable identifier shared with the
     * Rust crate and the test vectors: io, connection_closed, bad_magic, bad_role, same_role,
     * incompatible_major, too_large, json_too_large, truncated, json.
     */
    class ProtocolException(val kind: String, message: String, cause: Throwable? = null) :
        IOException(message, cause)

    // ------------------------------------------------------------------ preamble (§4)

    data class Preamble(val major: Int, val minor: Int, val role: Int) {
        fun encode(): ByteArray = byteArrayOf(
            MAGIC[0], MAGIC[1], MAGIC[2], MAGIC[3], major.toByte(), minor.toByte(), role.toByte(), 0,
        )

        val roleName get() = if (role == ROLE_CAMERA) "camera" else "host"

        /** Throws when [peer] cannot talk to us: same role or different major. */
        fun checkPeer(peer: Preamble) {
            if (peer.role == role) throw ProtocolException("same_role", "both sides announced the same role")
            if (peer.major != major) {
                val update = if (peer.major > major) "this app" else "the other side"
                throw ProtocolException(
                    "incompatible_major",
                    "incompatible protocol versions: peer speaks ${peer.major}.${peer.minor}, we speak $major.x; update $update",
                )
            }
        }

        companion object {
            fun ours(role: Int) = Preamble(MAJOR, MINOR, role)

            fun decode(b: ByteArray): Preamble {
                for (i in 0 until 4) {
                    if (b[i] != MAGIC[i]) throw ProtocolException("bad_magic", "not an Oculibre peer")
                }
                val role = b[6].toInt() and 0xFF
                if (role != ROLE_CAMERA && role != ROLE_HOST) {
                    throw ProtocolException("bad_role", "invalid role $role in preamble")
                }
                return Preamble(b[4].toInt() and 0xFF, b[5].toInt() and 0xFF, role)
            }
        }
    }

    // ------------------------------------------------------------------ header (§5)

    data class Header(
        val type: Int,
        val flags: Int = 0,
        val stream: Int = STREAM_SESSION,
        val length: Long = 0,
        /** µs; camera clock for media, sender clock otherwise; 0 = none. */
        val timestamp: Long = 0,
    ) {
        fun encodeInto(b: ByteArray, at: Int = 0) {
            b[at] = type.toByte()
            b[at + 1] = flags.toByte()
            b[at + 2] = stream.toByte()
            b[at + 3] = 0
            putInt(b, at + 4, length)
            putLong(b, at + 8, timestamp)
        }

        companion object {
            /** Parses a header and enforces the message size limit. The reserved byte is ignored. */
            fun decode(b: ByteArray): Header {
                val length = getUInt(b, 4)
                if (length > MAX_MESSAGE_LEN) {
                    throw ProtocolException("too_large", "message of $length bytes (max $MAX_MESSAGE_LEN)")
                }
                return Header(
                    type = b[0].toInt() and 0xFF,
                    flags = b[1].toInt() and 0xFF,
                    stream = b[2].toInt() and 0xFF,
                    length = length,
                    timestamp = getLong(b, 8),
                )
            }
        }
    }

    // ------------------------------------------------------------------ JSON payloads (§7–§9)

    data class Device(
        val manufacturer: String? = null,
        val model: String? = null,
        val os: String? = null,
        val osVersion: String? = null,
        val sdk: Long? = null,
    ) {
        fun toJson(): Map<String, Any?> = obj(
            "manufacturer" to manufacturer, "model" to model, "os" to os,
            "os_version" to osVersion, "sdk" to sdk,
        )

        companion object {
            fun fromJson(o: Map<String, Any?>) = Device(
                manufacturer = optStr(o, "manufacturer"),
                model = optStr(o, "model"),
                os = optStr(o, "os"),
                osVersion = optStr(o, "os_version"),
                sdk = optU32(o, "sdk"),
            )
        }
    }

    data class Color(
        val primaries: String = "bt601",
        val transfer: String = "bt709",
        val matrix: String = "bt601",
        val range: String = "limited",
    ) {
        fun toJson(): Map<String, Any?> = obj(
            "primaries" to primaries, "transfer" to transfer, "matrix" to matrix, "range" to range,
        )

        companion object {
            fun fromJson(o: Map<String, Any?>): Color {
                val d = Color()
                return Color(
                    str(o, "primaries", d.primaries), str(o, "transfer", d.transfer),
                    str(o, "matrix", d.matrix), str(o, "range", d.range),
                )
            }
        }
    }

    data class CameraInfo(
        val id: String = "",
        /** back, front or external. */
        val facing: String = "",
        /** Degrees clockwise: 0, 90, 180, 270. */
        val sensorOrientation: Int = 0,
        val mirrored: Boolean = false,
    ) {
        fun toJson(): Map<String, Any?> = obj(
            "id" to id, "facing" to facing, "sensor_orientation" to sensorOrientation.toLong(),
            "mirrored" to mirrored,
        )

        companion object {
            fun fromJson(o: Map<String, Any?>) = CameraInfo(
                id = str(o, "id", ""),
                facing = str(o, "facing", ""),
                sensorOrientation = u16(o, "sensor_orientation", 0),
                mirrored = bool(o, "mirrored", false),
            )
        }
    }

    /** `STREAM_CONFIG` payload (§8.1). */
    data class VideoConfig(
        val configId: Int = 0,
        val kind: String = "video",
        val codec: String = "h264",
        /** Base64 of SPS + PPS, Annex B. See [codecDataBytes]. */
        val codecData: String = "",
        val width: Long = 0,
        val height: Long = 0,
        val fps: Long = 0,
        val captureFps: Long? = null,
        /** normal, unlisted or high_speed. */
        val path: String? = null,
        val bitrate: Long? = null,
        val color: Color = Color(),
        val camera: CameraInfo? = null,
    ) {
        fun codecDataBytes(): ByteArray? = Base64Codec.decode(codecData)

        fun toJson(): Map<String, Any?> = obj(
            "config_id" to configId.toLong(), "kind" to kind, "codec" to codec, "codec_data" to codecData,
            "width" to width, "height" to height, "fps" to fps, "capture_fps" to captureFps,
            "path" to path, "bitrate" to bitrate, "color" to color.toJson(), "camera" to camera?.toJson(),
        )

        companion object {
            fun fromJson(o: Map<String, Any?>) = VideoConfig(
                configId = u16(o, "config_id", 0),
                kind = str(o, "kind", "video"),
                codec = str(o, "codec", "h264"),
                codecData = str(o, "codec_data", ""),
                width = u32(o, "width", 0),
                height = u32(o, "height", 0),
                fps = u32(o, "fps", 0),
                captureFps = optU32(o, "capture_fps"),
                path = optStr(o, "path"),
                bitrate = optU32(o, "bitrate"),
                color = optObj(o, "color")?.let { Color.fromJson(it) } ?: Color(),
                camera = optObj(o, "camera")?.let { CameraInfo.fromJson(it) },
            )
        }
    }

    data class ErrorBody(val code: String = "", val message: String? = null) {
        fun toJson(): Map<String, Any?> = obj("code" to code, "message" to message)

        companion object {
            fun fromJson(o: Map<String, Any?>) = ErrorBody(str(o, "code", ""), optStr(o, "message"))
        }
    }

    // ------------------------------------------------------------------ messages

    sealed class Message {
        abstract val type: Int

        data class Hello(
            val app: String = "",
            val appVersion: String = "",
            /** Random UUID created once per installation, never derived from hardware ids. */
            val id: String = "",
            val name: String = "",
            val device: Device? = null,
            val features: List<String> = emptyList(),
            /** Optional capabilities catalogue (§7.4), free-form JSON. */
            val capabilities: Any? = null,
            /** Camera, inside TLS only: one-time token of the pairing QR code (§13, since 0.3). */
            val pairingToken: String? = null,
        ) : Message() {
            override val type get() = Type.HELLO
            fun hasFeature(f: String) = f in features
            fun toJson(): Map<String, Any?> = obj(
                "app" to app, "app_version" to appVersion, "id" to id, "name" to name,
                "device" to device?.toJson(), "features" to features,
                "capabilities" to Json.sorted(capabilities),
                "pairing_token" to pairingToken,
            )
        }

        data class Bye(val reason: String = "", val message: String? = null) : Message() {
            override val type get() = Type.BYE
            fun toJson(): Map<String, Any?> = obj("reason" to reason, "message" to message)
        }

        data class Ping(val seq: Long, val t1: Long) : Message() {
            override val type get() = Type.PING
        }

        data class Pong(val seq: Long, val t1: Long, val t2: Long, val t3: Long) : Message() {
            override val type get() = Type.PONG
        }

        data class StreamConfig(val stream: Int, val config: VideoConfig) : Message() {
            override val type get() = Type.STREAM_CONFIG
        }

        /** One complete access unit, Annex B. [data] is used as is (not copied). */
        class VideoFrame(
            val stream: Int,
            /** Capture time, camera clock, µs. */
            val timestamp: Long,
            val key: Boolean,
            val discontinuity: Boolean,
            val frameNumber: Long,
            val configId: Int,
            val data: ByteArray,
        ) : Message() {
            override val type get() = Type.VIDEO_FRAME
        }

        data class StreamEnd(val stream: Int, val reason: String = "") : Message() {
            override val type get() = Type.STREAM_END
        }

        data class Request(val id: Long, val cmd: String = "", val args: Any? = null) : Message() {
            override val type get() = Type.REQUEST
            fun toJson(): Map<String, Any?> = obj("id" to id, "cmd" to cmd, "args" to Json.sorted(args))
        }

        data class Response(
            val id: Long,
            val ok: Boolean = false,
            val result: Any? = null,
            val error: ErrorBody? = null,
        ) : Message() {
            override val type get() = Type.RESPONSE
            fun toJson(): Map<String, Any?> = obj(
                "id" to id, "ok" to ok, "result" to Json.sorted(result), "error" to error?.toJson(),
            )

            companion object {
                fun ok(id: Long, result: Any? = emptyMap<String, Any?>()) = Response(id, true, result, null)
                fun error(id: Long, code: String, message: String? = null) =
                    Response(id, false, null, ErrorBody(code, message))
            }
        }

        data class Event(val event: String = "", val data: Any? = null) : Message() {
            override val type get() = Type.EVENT

            // `data` is always written, even null, like the host does.
            fun toJson(): Map<String, Any?> = linkedMapOf("event" to event, "data" to Json.sorted(data))
        }

        /** A type this version does not know: its payload was skipped (§6). */
        data class Unknown(override val type: Int, val stream: Int, val length: Long) : Message()
    }

    // ------------------------------------------------------------------ encoding

    /** Header + payload, ready to be written in a single call. */
    fun encode(m: Message): ByteArray = when (m) {
        is Message.Hello -> json(m.type, STREAM_SESSION, m.toJson())
        is Message.Bye -> json(m.type, STREAM_SESSION, m.toJson())
        is Message.StreamConfig -> json(m.type, m.stream, m.config.toJson())
        is Message.StreamEnd -> json(m.type, m.stream, linkedMapOf("reason" to m.reason))
        is Message.Request -> json(m.type, STREAM_SESSION, m.toJson())
        is Message.Response -> json(m.type, STREAM_SESSION, m.toJson())
        is Message.Event -> json(m.type, STREAM_SESSION, m.toJson())
        is Message.Ping -> ByteArray(HEADER_LEN + 16).also {
            Header(Type.PING, length = 16).encodeInto(it)
            putInt(it, HEADER_LEN, m.seq)
            putLong(it, HEADER_LEN + 8, m.t1)
        }
        is Message.Pong -> ByteArray(HEADER_LEN + 32).also {
            Header(Type.PONG, length = 32).encodeInto(it)
            putInt(it, HEADER_LEN, m.seq)
            putLong(it, HEADER_LEN + 8, m.t1)
            putLong(it, HEADER_LEN + 16, m.t2)
            putLong(it, HEADER_LEN + 24, m.t3)
        }
        is Message.VideoFrame -> encodeVideoFrame(
            m.stream, m.timestamp, m.key, m.discontinuity, m.frameNumber, m.configId, m.data, 0, m.data.size,
        )
        is Message.Unknown -> ByteArray(HEADER_LEN).also { Header(m.type, stream = m.stream).encodeInto(it) }
    }

    /** `VIDEO_FRAME` from a slice of an encoder buffer, without an intermediate copy. */
    fun encodeVideoFrame(
        stream: Int, timestamp: Long, key: Boolean, discontinuity: Boolean,
        frameNumber: Long, configId: Int, data: ByteArray, offset: Int, length: Int,
    ): ByteArray {
        val payload = VIDEO_SUBHEADER_LEN + length
        val out = ByteArray(HEADER_LEN + payload)
        val flags = (if (key) FLAG_KEY else 0) or (if (discontinuity) FLAG_DISCONTINUITY else 0)
        Header(Type.VIDEO_FRAME, flags, stream, payload.toLong(), timestamp).encodeInto(out)
        putInt(out, HEADER_LEN, frameNumber)
        out[HEADER_LEN + 4] = (configId shr 8).toByte()
        out[HEADER_LEN + 5] = configId.toByte()
        System.arraycopy(data, offset, out, HEADER_LEN + VIDEO_SUBHEADER_LEN, length)
        return out
    }

    private fun json(type: Int, stream: Int, body: Map<String, Any?>): ByteArray {
        val payload = Json.writeBytes(body)
        val out = ByteArray(HEADER_LEN + payload.size)
        Header(type, 0, stream, payload.size.toLong(), 0).encodeInto(out)
        System.arraycopy(payload, 0, out, HEADER_LEN, payload.size)
        return out
    }

    // ------------------------------------------------------------------ decoding

    /** Decodes a message from its header and complete payload. */
    fun decode(h: Header, p: ByteArray): Message {
        val t = h.type
        if (Type.isJson(t) && p.size > MAX_JSON_LEN) {
            throw ProtocolException("json_too_large", "${Type.name(t)} JSON payload of ${p.size} bytes")
        }
        return when (t) {
            Type.HELLO -> jsonObject(t, p) { o ->
                Message.Hello(
                    app = str(o, "app", ""),
                    appVersion = str(o, "app_version", ""),
                    id = str(o, "id", ""),
                    name = str(o, "name", ""),
                    device = optObj(o, "device")?.let { Device.fromJson(it) },
                    features = strList(o, "features"),
                    capabilities = o["capabilities"],
                    pairingToken = optStr(o, "pairing_token"),
                )
            }
            Type.BYE -> jsonObject(t, p) { o -> Message.Bye(str(o, "reason", ""), optStr(o, "message")) }
            Type.STREAM_CONFIG -> jsonObject(t, p) { o -> Message.StreamConfig(h.stream, VideoConfig.fromJson(o)) }
            Type.STREAM_END -> jsonObject(t, p) { o -> Message.StreamEnd(h.stream, str(o, "reason", "")) }
            Type.REQUEST -> jsonObject(t, p) { o ->
                Message.Request(u32(o, "id", 0), str(o, "cmd", ""), o["args"])
            }
            Type.RESPONSE -> jsonObject(t, p) { o ->
                Message.Response(
                    id = u32(o, "id", 0),
                    ok = bool(o, "ok", false),
                    result = o["result"],
                    error = optObj(o, "error")?.let { ErrorBody.fromJson(it) },
                )
            }
            Type.EVENT -> jsonObject(t, p) { o -> Message.Event(str(o, "event", ""), o["data"]) }
            Type.PING -> {
                need(t, p, 16)
                Message.Ping(getUInt(p, 0), getLong(p, 8))
            }
            Type.PONG -> {
                need(t, p, 32)
                Message.Pong(getUInt(p, 0), getLong(p, 8), getLong(p, 16), getLong(p, 24))
            }
            Type.VIDEO_FRAME -> {
                need(t, p, VIDEO_SUBHEADER_LEN)
                Message.VideoFrame(
                    stream = h.stream,
                    timestamp = h.timestamp,
                    key = h.flags and FLAG_KEY != 0,
                    discontinuity = h.flags and FLAG_DISCONTINUITY != 0,
                    frameNumber = getUInt(p, 0),
                    configId = ((p[4].toInt() and 0xFF) shl 8) or (p[5].toInt() and 0xFF),
                    data = p.copyOfRange(VIDEO_SUBHEADER_LEN, p.size),
                )
            }
            else -> Message.Unknown(t, h.stream, h.length)
        }
    }

    private inline fun <T> jsonObject(type: Int, p: ByteArray, build: (Map<String, Any?>) -> T): T {
        val v = try {
            Json.parse(p)
        } catch (e: Json.ParseException) {
            throw ProtocolException("json", "invalid ${Type.name(type)} JSON: ${e.message}", e)
        }
        @Suppress("UNCHECKED_CAST")
        val o = v as? Map<String, Any?>
            ?: throw ProtocolException("json", "${Type.name(type)} payload is not a JSON object")
        try {
            return build(o)
        } catch (e: FieldException) {
            throw ProtocolException("json", "invalid ${Type.name(type)} JSON: ${e.message}", e)
        }
    }

    private fun need(type: Int, p: ByteArray, n: Int) {
        if (p.size < n) {
            throw ProtocolException("truncated", "${Type.name(type)} payload too short: ${p.size} bytes, need $n")
        }
    }

    // ------------------------------------------------------------------ stream I/O

    /**
     * Reads a preamble then messages. Limits are enforced from the header alone, before any
     * allocation; unknown types are skipped without being buffered.
     */
    class MessageReader(private val input: InputStream) {
        private val headerBuf = ByteArray(HEADER_LEN)

        fun readPreamble(): Preamble {
            val b = ByteArray(PREAMBLE_LEN)
            readFullyOrClosed(b)
            return Preamble.decode(b)
        }

        /** Throws kind `connection_closed` on a clean close between two messages, `io` inside one. */
        fun readMessage(): Message {
            readFullyOrClosed(headerBuf)
            val h = Header.decode(headerBuf)
            val len = h.length.toInt()
            if (!Type.isDecoded(h.type)) {
                skipFully(len.toLong())
                return Message.Unknown(h.type, h.stream, h.length)
            }
            if (Type.isJson(h.type) && len > MAX_JSON_LEN) {
                throw ProtocolException("json_too_large", "${Type.name(h.type)} announces $len bytes")
            }
            val p = ByteArray(len)
            readFully(p, 0)
            return decode(h, p)
        }

        private fun readFullyOrClosed(b: ByteArray) {
            val n = input.read(b, 0, b.size)
            if (n < 0) throw ProtocolException("connection_closed", "connection closed by the peer")
            readFully(b, n)
        }

        private fun readFully(b: ByteArray, from: Int) {
            var filled = from
            while (filled < b.size) {
                val n = try {
                    input.read(b, filled, b.size - filled)
                } catch (e: IOException) {
                    throw ProtocolException("io", e.message ?: "I/O error", e)
                }
                if (n < 0) throw ProtocolException("io", "connection closed in the middle of a message", EOFException())
                filled += n
            }
        }

        private fun skipFully(n: Long) {
            var left = n
            val scratch = ByteArray(minOf(n, 8192L).toInt().coerceAtLeast(1))
            while (left > 0) {
                val r = input.read(scratch, 0, minOf(left, scratch.size.toLong()).toInt())
                if (r < 0) throw ProtocolException("io", "connection closed in the middle of a message", EOFException())
                left -= r
            }
        }
    }

    /** Serialises writes from several threads; each message goes out in a single write. */
    class MessageWriter(private val output: OutputStream) {
        fun write(m: Message) = writeRaw(encode(m))

        fun writeRaw(bytes: ByteArray) {
            synchronized(this) {
                output.write(bytes)
                output.flush()
            }
        }
    }

    // ------------------------------------------------------------------ clock (§11)

    /** Responder clock minus sender clock, and round trip without the responder's processing. */
    data class ClockSample(val offsetUs: Long, val rttUs: Long)

    fun clockSample(t1: Long, t2: Long, t3: Long, t4: Long) =
        ClockSample(((t2 - t1) + (t3 - t4)) / 2, (t4 - t1) - (t3 - t2))

    // ------------------------------------------------------------------ helpers

    private class FieldException(message: String) : Exception(message)

    /** JSON object without null values (absent optional keys are omitted, like the host). */
    private fun obj(vararg pairs: Pair<String, Any?>): Map<String, Any?> {
        val m = LinkedHashMap<String, Any?>()
        for ((k, v) in pairs) if (v != null) m[k] = v
        return m
    }

    private fun str(o: Map<String, Any?>, k: String, default: String): String {
        if (!o.containsKey(k)) return default
        return o[k] as? String ?: throw FieldException("'$k' must be a string")
    }

    private fun optStr(o: Map<String, Any?>, k: String): String? {
        val v = o[k] ?: return null
        return v as? String ?: throw FieldException("'$k' must be a string")
    }

    private fun bool(o: Map<String, Any?>, k: String, default: Boolean): Boolean {
        if (!o.containsKey(k)) return default
        return o[k] as? Boolean ?: throw FieldException("'$k' must be a boolean")
    }

    private fun unsigned(k: String, v: Any?, max: Long): Long {
        val n = v as? Long ?: throw FieldException("'$k' must be an integer")
        if (n < 0 || n > max) throw FieldException("'$k' out of range")
        return n
    }

    private fun u32(o: Map<String, Any?>, k: String, default: Long): Long =
        if (!o.containsKey(k)) default else unsigned(k, o[k], 0xFFFF_FFFFL)

    private fun optU32(o: Map<String, Any?>, k: String): Long? = o[k]?.let { unsigned(k, it, 0xFFFF_FFFFL) }

    private fun u16(o: Map<String, Any?>, k: String, default: Int): Int =
        if (!o.containsKey(k)) default else unsigned(k, o[k], 0xFFFF).toInt()

    @Suppress("UNCHECKED_CAST")
    private fun optObj(o: Map<String, Any?>, k: String): Map<String, Any?>? {
        val v = o[k] ?: return null
        return v as? Map<String, Any?> ?: throw FieldException("'$k' must be an object")
    }

    private fun strList(o: Map<String, Any?>, k: String): List<String> {
        if (!o.containsKey(k)) return emptyList()
        val l = o[k] as? List<*> ?: throw FieldException("'$k' must be an array")
        return l.map { it as? String ?: throw FieldException("'$k' must contain strings") }
    }

    private fun putInt(b: ByteArray, at: Int, v: Long) {
        b[at] = (v ushr 24).toByte(); b[at + 1] = (v ushr 16).toByte()
        b[at + 2] = (v ushr 8).toByte(); b[at + 3] = v.toByte()
    }

    private fun putLong(b: ByteArray, at: Int, v: Long) {
        for (i in 0 until 8) b[at + i] = (v ushr (56 - 8 * i)).toByte()
    }

    private fun getUInt(b: ByteArray, at: Int): Long =
        ((b[at].toLong() and 0xFF) shl 24) or ((b[at + 1].toLong() and 0xFF) shl 16) or
            ((b[at + 2].toLong() and 0xFF) shl 8) or (b[at + 3].toLong() and 0xFF)

    private fun getLong(b: ByteArray, at: Int): Long {
        var v = 0L
        for (i in 0 until 8) v = (v shl 8) or (b[at + i].toLong() and 0xFF)
        return v
    }
}
