package org.oculibre

import android.annotation.SuppressLint
import android.annotation.TargetApi
import android.content.Context
import android.graphics.SurfaceTexture
import android.hardware.camera2.CameraAccessException
import android.hardware.camera2.CameraCaptureSession
import android.hardware.camera2.CameraCharacteristics
import android.hardware.camera2.CameraConstrainedHighSpeedCaptureSession
import android.hardware.camera2.CameraDevice
import android.hardware.camera2.CameraManager
import android.hardware.camera2.CameraMetadata
import android.hardware.camera2.CaptureRequest
import android.hardware.camera2.CaptureResult
import android.hardware.camera2.TotalCaptureResult
import android.media.MediaCodec
import android.media.MediaCodecInfo
import android.media.MediaFormat
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.HandlerThread
import android.os.SystemClock
import android.util.Range
import android.util.Size
import android.view.OrientationEventListener
import android.view.Surface
import java.io.BufferedInputStream
import java.io.BufferedOutputStream
import java.io.IOException
import java.net.InetSocketAddress
import java.net.Socket
import java.net.SocketTimeoutException
import java.util.UUID
import java.util.concurrent.ArrayBlockingQueue
import java.util.concurrent.CountDownLatch
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicLong
import kotlin.concurrent.thread
import org.oculibre.Protocol.Message

data class StreamConfig(
    val width: Int = 1280,
    val height: Int = 720,
    val fps: Int = 30,
    val bitrate: Int = 4_000_000,
    val iFrameIntervalSec: Int = 1,
    /** H.264 level matching size × fps (see [QualityMode.avcLevel]). */
    val avcLevel: Int = MediaCodecInfo.CodecProfileLevel.AVCLevel31,
    /** Request Baseline (no B-frames) — only if the encoder supports it at [avcLevel]. */
    val baseline: Boolean = true,
    /** Encoder found by [Capabilities.probe]; null = system default. */
    val encoderName: String? = null,
    /** Camera found by [Capabilities.probe]; null = back camera. */
    val cameraId: String? = null,
    /** > 0: capture with the high-speed session at this rate, encoder keeps [fps]. */
    val highSpeedFps: Int = 0,
    /** Capture path announced to the computer: normal, unlisted or high_speed. */
    val path: String = "normal",
    /** Every camera and mode (HELLO, get_capabilities, set_mode), null if unknown. */
    val device: DeviceCapabilities? = null,
) {
    companion object {
        fun from(support: ModeSupport, camera: CameraCapabilities, caps: DeviceCapabilities): StreamConfig {
            val mode = support.mode
            return StreamConfig(
                width = mode.width,
                height = mode.height,
                fps = mode.fps,
                bitrate = mode.bitrate,
                avcLevel = mode.avcLevel,
                baseline = caps.baselineMaxLevel >= mode.avcLevel,
                encoderName = caps.encoderName,
                cameraId = camera.id,
                highSpeedFps = support.highSpeedFps,
                path = support.path,
                device = caps,
            )
        }
    }
}

/**
 * One streaming session, speaking the Oculibre protocol 0.2 (docs/protocol.typ):
 *
 *   Camera2 ──► encoder input Surface ──► MediaCodec (H.264) ──► queue ──► TCP socket
 *                                                                           ▲   │
 *                                         PONG, RESPONSE, events ───────────┘   ▼
 *                                                       reader ◄── PING, REQUEST, BYE, stats
 *
 * The phone dials the computer (USB: 127.0.0.1 through `adb reverse`), sends its preamble and
 * HELLO, then a STREAM_CONFIG each time the encoder (re)starts, and one VIDEO_FRAME per access unit.
 * The camera never rotates the picture: it reports its sensor orientation and the phone rotation,
 * and the computer decides.
 *
 * Camera + encoder form a [Pipeline]: the computer's `set_mode` command replaces it as a whole
 * (STREAM_END, then a new STREAM_CONFIG), while the connection stays up.
 *
 * Threads: setup, camera (HandlerThread), encoder drain, sender, reader, control, mode, stats, stop.
 */
class StreamSession(
    context: Context,
    private val host: String,
    private val port: Int,
    private val requested: StreamConfig,
    private val listener: Listener,
) {
    interface Listener {
        fun onStatus(session: StreamSession, text: String)
        fun onStopped(session: StreamSession, error: String?)
    }

    /**
     * An encoded message ready to send; [ptsUs] = capture time of a frame, -1 otherwise;
     * [encodedUs] = when the encoder output it (same clock).
     */
    private class Packet(val bytes: ByteArray, val ptsUs: Long = -1, val encodedUs: Long = -1) {
        val isFrame get() = ptsUs >= 0
    }

    /** Average of the delays of the last second (µs in, ms out), shared between threads. */
    private class Delay {
        private val sum = AtomicLong()
        private val count = AtomicInteger()

        fun add(us: Long) {
            if (us in 0..10_000_000) {
                sum.addAndGet(us)
                count.incrementAndGet()
            }
        }

        /** Average in ms (0.1 ms steps) since the last call, null if nothing was measured. */
        fun takeMs(): Double? {
            val n = count.getAndSet(0)
            val total = sum.getAndSet(0)
            return if (n == 0) null else Math.round(total / n / 100.0) / 10.0
        }
    }

    private val appContext = context.applicationContext
    private val cameraManager = context.getSystemService(Context.CAMERA_SERVICE) as CameraManager

    private val running = AtomicBoolean(false)
    private val stopped = AtomicBoolean(false)

    @Volatile private var socket: Socket? = null
    @Volatile private var writer: Protocol.MessageWriter? = null
    /** Camera + encoder currently streaming (or starting). */
    @Volatile private var pipeline: Pipeline? = null
    @Volatile private var activeConfig: StreamConfig = requested
    @Volatile private var cameraInfo: Protocol.CameraInfo? = null
    @Volatile private var orientationListener: OrientationEventListener? = null
    /** When the current pipeline started (elapsedRealtime ms), for the frame-rate warning. */
    @Volatile private var pipelineStartMs = 0L

    /** Address actually used: [host], or the one found by mDNS. */
    @Volatile private var target: String = host

    /** One-time pairing token to send in HELLO (Wi-Fi, first connection after pairing). */
    @Volatile private var pairingToken: String? = null
    /** " · encrypted (TLSv1.3)" on Wi-Fi, for the status line. */
    @Volatile private var linkLabel = ""
    /** "\nLab settings: qcom low latency off" when Lab settings differ from the defaults. */
    @Volatile private var tuningLabel = ""

    @Volatile private var setupThread: Thread? = null
    @Volatile private var senderThread: Thread? = null
    @Volatile private var readerThread: Thread? = null
    @Volatile private var statsThread: Thread? = null

    /** Small writes that must not happen on the main thread (orientation events). */
    private val control: ExecutorService = Executors.newSingleThreadExecutor { r ->
        Thread(r, "oculibre-control")
    }

    /** set_mode work (closing and opening a camera takes a while: never on the reader thread). */
    private val modeExecutor: ExecutorService = Executors.newSingleThreadExecutor { r ->
        Thread(r, "oculibre-mode")
    }
    private val switching = AtomicBoolean(false)

    private val queue = ArrayBlockingQueue<Packet>(QUEUE_CAPACITY)

    /** Features both sides understand (HELLO intersection). */
    @Volatile private var shared: Set<String> = emptySet()

    /** Clock of the camera timestamps (§11): frames and PONG times must use the same one. */
    @Volatile private var realtimeClock = false

    /**
     * Set once the clock has been checked against the first encoded frame. Until then PINGs are
     * not answered, so the computer never mixes samples from two clocks.
     */
    private val clockChecked = AtomicBoolean(false)

    /** Sensor timestamp of the first captured frame (ns), for encoders with their own time base. */
    @Volatile private var firstSensorNs = -1L

    /** Encoder timestamp minus sent timestamp (µs). Drain thread only. */
    private var timestampShiftUs = 0L

    // Only touched by the drain thread.
    private var waitingForKeyframe = false
    private var pendingDiscontinuity = false
    private var frameNumber = 0L
    private var configId = 0
    private var outputFormat: MediaFormat? = null

    private val framesSent = AtomicInteger()
    private val framesDropped = AtomicInteger()
    private val cameraFrames = AtomicInteger()
    private val bytesSent = AtomicLong()
    /** Capture → handed to the network (camera + encoder + queue). */
    private val phoneDelay = Delay()
    /** Capture → the camera has finished the picture (onCaptureCompleted). */
    private val cameraDelay = Delay()
    /** Capture → out of the encoder (camera + encoder). */
    private val encodedDelay = Delay()
    /** Out of the encoder → handed to the network. */
    private val queueDelay = Delay()

    /**
     * Clock of SENSOR_TIMESTAMP for the current camera: boot time (true) or monotonic, decided on
     * its first picture (SENSOR_INFO_TIMESTAMP_SOURCE is not reliable, see checkClock); null =
     * not decided yet.
     */
    @Volatile private var sensorRealtime: Boolean? = null

    // What the computer reports (its `stats` event).
    @Volatile private var hostStats: Map<*, *>? = null

    /** Last device rotation sent, -1 = none yet. Main thread only. */
    private var lastRotation = -1

    // ------------------------------------------------------------------ lifecycle

    fun start() {
        if (!running.compareAndSet(false, true)) return
        setupThread = thread(name = "oculibre-setup") {
            try {
                val s = connect()
                socket = s
                if (!running.get()) return@thread
                socket = secure(s)
                if (!running.get()) return@thread

                status("Connected — handshake…")
                val reader = handshake(socket!!)
                if (!running.get()) return@thread

                status("Connected — opening camera…")
                // set_mode answers "busy" until this first camera is up (one start at a time).
                switching.set(true)
                try {
                    startReader(reader)
                    // Before the camera: the computer should know the rotation before the first frame.
                    startOrientation()
                    startSender()
                    startPipeline(requested)
                } finally {
                    switching.set(false)
                }
                startStats()
            } catch (e: PipelineException) {
                if (running.get()) fail(e.message ?: "Camera error", "camera_lost")
            } catch (e: Protocol.ProtocolException) {
                fail(e.message ?: e.kind, if (e.kind == "incompatible_major") "version" else "protocol_error")
            } catch (e: SocketTimeoutException) {
                fail("The computer did not answer (is it running Oculibre 0.1 or newer?)", "protocol_error")
            } catch (e: Exception) {
                fail(e)
            }
        }
    }

    fun stop() = shutdown(null, "user_stop")

    private fun fail(e: Throwable) = fail(e.message ?: e.javaClass.simpleName)

    private fun fail(message: String, byeReason: String = "other") = shutdown(message, byeReason)

    private fun shutdown(error: String?, byeReason: String?) {
        if (!stopped.compareAndSet(false, true)) return
        running.set(false)
        thread(name = "oculibre-stop") {
            orientationListener?.disable()
            // Polite goodbye, without waiting long on a congested link.
            val w = writer
            if (w != null && byeReason != null) {
                val bye = thread(name = "oculibre-bye") {
                    runCatching { w.write(Message.Bye(byeReason, error)) }
                }
                bye.join(BYE_TIMEOUT_MS)
            }
            // Closing the socket unblocks the sender (write) and the reader (read).
            runCatching { socket?.close() }
            // Unblock a camera start in progress (setup or set_mode), then wait for it.
            pipeline?.abort()
            setupThread?.join(CONNECT_TIMEOUT_MS + 1000L)
            modeExecutor.shutdownNow()
            runCatching { modeExecutor.awaitTermination(3, TimeUnit.SECONDS) }
            pipeline?.close()
            senderThread?.join(1000)
            readerThread?.join(1000)
            statsThread?.interrupt()
            control.shutdownNow()
            queue.clear()
            listener.onStopped(this, error)
        }
    }

    private fun status(text: String) = listener.onStatus(this, text)

    /** Now, on the clock of the camera timestamps, in µs. */
    private fun nowUs(): Long =
        if (realtimeClock) SystemClock.elapsedRealtimeNanos() / 1000 else System.nanoTime() / 1000

    // ------------------------------------------------------------------ protocol

    /**
     * TCP connection to the computer. Wi-Fi with a paired computer: the *direct* way always wins
     * (a VPN like Tailscale is much slower), whatever address the field holds:
     * 1. the computer's known addresses on one of this phone's local networks;
     * 2. the computer found on the local network (mDNS, §2);
     * 3. its addresses through a VPN the phone is on;
     * 4. any other known address.
     * The address that worked is remembered (and shown in the address field).
     */
    private fun connect(): Socket {
        fun open(address: String, timeoutMs: Int): Socket {
            // Not `Socket().apply { … port … }`: there `port` would be the socket's own (0).
            val s = Socket()
            s.tcpNoDelay = true
            s.connect(InetSocketAddress(address, port), timeoutMs)
            return s
        }
        val paired = if (SecureChannel.isLoopback(host)) null else PairingStore.get(appContext)
        if (paired == null) return open(host, CONNECT_TIMEOUT_MS)

        val subnets = Networks.local()
        val known = Networks.rank(listOf(host) + paired.addresses, subnets)
        val byRoute = known.groupBy { Networks.route(it, subnets) }
        val tried = mutableSetOf<String>()
        fun attempt(address: String, timeoutMs: Int, how: String): Socket? {
            if (!running.get() || !tried.add(address)) return null
            return try {
                open(address, timeoutMs).also {
                    target = address
                    if (address != host) {
                        PairingStore.rememberAddress(appContext, address)
                        status("Reached ${paired.name} at $address ($how)")
                    }
                }
            } catch (e: IOException) {
                null
            }
        }

        for (a in byRoute[Networks.Route.DIRECT].orEmpty()) {
            attempt(a, FIRST_TRY_TIMEOUT_MS, "local network")?.let { return it }
        }
        val vpn = byRoute[Networks.Route.VPN].orEmpty()
        if (Networks.hasLocalNetwork(subnets) && running.get()) {
            status("Looking for ${paired.name} on the network…")
            // Shorter when a VPN way remains to be tried.
            val timeout = if (vpn.isEmpty()) DISCOVERY_TIMEOUT_MS else DISCOVERY_TIMEOUT_MS / 2
            Discovery.find(appContext, paired.fingerprint, timeout)?.let { found ->
                attempt(found, CONNECT_TIMEOUT_MS, "found on the local network")?.let { return it }
            }
        }
        for (a in vpn) attempt(a, CONNECT_TIMEOUT_MS, "through the VPN")?.let { return it }
        for (a in byRoute[Networks.Route.UNKNOWN].orEmpty()) {
            attempt(a, FIRST_TRY_TIMEOUT_MS, "known address")?.let { return it }
        }
        throw IOException(
            "Cannot reach ${paired.name} (tried ${tried.joinToString()}, and not found on this " +
                "network): is `oculibre --wifi` running there?",
        )
    }

    /** Wi-Fi: TLS with the paired computer's pinned key (§13). USB (loopback) stays plaintext. */
    private fun secure(s: Socket): Socket {
        if (SecureChannel.isLoopback(target)) return s
        val paired = PairingStore.get(appContext) ?: throw IOException(
            "Wi-Fi needs pairing first: run `oculibre --pair` on the computer and scan its QR code " +
                "with the phone's camera",
        )
        status("Connected — encrypting…")
        s.soTimeout = HANDSHAKE_TIMEOUT_MS
        val tls = try {
            SecureChannel.connect(appContext, s, target, port, paired.fingerprint)
        } catch (e: javax.net.ssl.SSLException) {
            if (generateSequence<Throwable>(e) { it.cause }.any { it is SecureChannel.WrongComputer }) {
                throw IOException(
                    "This computer is not the one paired with this phone (${paired.name}): " +
                        "pair again with `oculibre --pair`",
                )
            }
            throw IOException("Secure connection failed: ${e.message}")
        }
        pairingToken = paired.token
        linkLabel = " · encrypted (${SecureChannel.version(tls)})"
        return tls
    }

    /** Preambles and HELLOs (§4, §7.1). The computer has 5 s to answer. */
    private fun handshake(s: Socket): Protocol.MessageReader {
        s.soTimeout = HANDSHAKE_TIMEOUT_MS
        val w = Protocol.MessageWriter(BufferedOutputStream(s.getOutputStream(), 64 * 1024))
        val r = Protocol.MessageReader(BufferedInputStream(s.getInputStream(), 64 * 1024))
        val ours = Protocol.Preamble.ours(Protocol.ROLE_CAMERA)
        w.writeRaw(ours.encode())
        w.write(hello())
        writer = w

        val peer = try {
            r.readPreamble()
        } catch (e: Protocol.ProtocolException) {
            if (e.kind == "connection_closed") {
                throw IOException("The computer closed the connection (over Wi-Fi it must run `oculibre --wifi`)")
            }
            throw e
        }
        ours.checkPeer(peer)

        while (true) {
            when (val m = r.readMessage()) {
                is Message.Hello -> {
                    shared = m.features.toSet() intersect OUR_FEATURES.toSet()
                    // The computer accepted the pairing token: it is used up.
                    if (pairingToken != null) {
                        PairingStore.clearToken(appContext)
                        pairingToken = null
                    }
                    break
                }
                is Message.Bye -> {
                    if (m.reason == "unpaired" && pairingToken != null) PairingStore.clearToken(appContext)
                    throw IOException(byeText(m))
                }
                else -> {} // nothing else is allowed before HELLO: ignore
            }
        }
        // From now on, 5 s of silence means the computer is gone (§7.3).
        s.soTimeout = SILENCE_TIMEOUT_MS
        return r
    }

    private fun hello(): Message.Hello {
        val prefs = appContext.getSharedPreferences("oculibre", Context.MODE_PRIVATE)
        // Random, per installation, never derived from hardware identifiers (§7.1).
        val id = prefs.getString(PREF_INSTALL_ID, null) ?: UUID.randomUUID().toString().also {
            prefs.edit().putString(PREF_INSTALL_ID, it).apply()
        }
        val version = runCatching {
            appContext.packageManager.getPackageInfo(appContext.packageName, 0).versionName
        }.getOrNull() ?: ""
        return Message.Hello(
            app = "oculibre-android",
            appVersion = version,
            id = id,
            name = Build.MODEL,
            device = Protocol.Device(
                manufacturer = Build.MANUFACTURER,
                model = Build.MODEL,
                os = "android",
                osVersion = Build.VERSION.RELEASE,
                sdk = Build.VERSION.SDK_INT.toLong(),
            ),
            features = OUR_FEATURES,
            capabilities = requested.device?.catalogue(),
            pairingToken = pairingToken,
        )
    }

    private fun byeText(m: Message.Bye): String {
        val why = when (m.reason) {
            "user_stop", "shutdown" -> "The computer stopped the stream"
            "busy" -> "The computer is busy"
            "version" -> "Incompatible versions: update Oculibre"
            "protocol_error" -> "The computer rejected the stream (protocol error)"
            "unpaired" -> "This phone is not paired with the computer: run `oculibre --pair` there " +
                "and scan the QR code"
            else -> "The computer closed the connection (${m.reason})"
        }
        return if (m.message.isNullOrEmpty()) why else "$why: ${m.message}"
    }

    /** Messages from the computer. */
    private fun startReader(r: Protocol.MessageReader) {
        readerThread = thread(name = "oculibre-reader") {
            try {
                while (running.get()) {
                    when (val m = r.readMessage()) {
                        is Message.Ping -> if (clockChecked.get()) {
                            val t2 = nowUs()
                            writer?.write(Message.Pong(m.seq, m.t1, t2, nowUs()))
                        }
                        is Message.Request -> handle(m)?.let { writer?.write(it) }
                        is Message.Event -> if (m.event == "stats") hostStats = m.data as? Map<*, *>
                        is Message.Bye -> {
                            // The computer leaves on purpose: no BYE back.
                            shutdown(byeText(m), null)
                            return@thread
                        }
                        else -> {} // PONG (we don't ping), unknown types…
                    }
                }
            } catch (e: Protocol.ProtocolException) {
                if (!running.get()) return@thread
                when (e.kind) {
                    "connection_closed" -> fail("The computer closed the connection", "other")
                    "io" -> fail("Connection lost: ${e.message}", "other")
                    else -> fail("Invalid data from the computer: ${e.message}", "protocol_error")
                }
            } catch (e: SocketTimeoutException) {
                if (running.get()) fail("The computer stopped answering", "other")
            } catch (e: IOException) {
                if (running.get()) fail("Connection lost: ${e.message}", "other")
            }
        }
    }

    /**
     * Commands of §9.1. Every request gets exactly one response: returned here, or written later
     * by the mode thread for `set_mode` (null).
     */
    private fun handle(req: Message.Request): Message.Response? {
        when (req.cmd) {
            "get_capabilities" -> return requested.device?.let { Message.Response.ok(req.id, it.catalogue()) }
                ?: Message.Response.error(req.id, Protocol.ErrorCode.UNSUPPORTED, "capabilities unknown")
            "set_mode" -> {
                if (!switching.compareAndSet(false, true)) {
                    return Message.Response.error(req.id, Protocol.ErrorCode.BUSY, "already changing mode")
                }
                try {
                    modeExecutor.execute {
                        val response = try {
                            setMode(req)
                        } finally {
                            switching.set(false)
                        }
                        runCatching { writer?.write(response) }
                    }
                } catch (e: Exception) { // shutting down
                    switching.set(false)
                    return Message.Response.error(req.id, Protocol.ErrorCode.BUSY, "stopping")
                }
                return null
            }
        }
        val c = pipeline?.codec ?: return Message.Response.error(req.id, Protocol.ErrorCode.BUSY, "encoder not ready")
        return when (req.cmd) {
            "request_keyframe" -> {
                requestKeyframe(c)
                Message.Response.ok(req.id)
            }
            "set_bitrate" -> {
                val bitrate = ((req.args as? Map<*, *>)?.get("bitrate") as? Long)
                if (bitrate == null || bitrate < MIN_BITRATE || bitrate > MAX_BITRATE) {
                    Message.Response.error(
                        req.id, Protocol.ErrorCode.INVALID_ARGS,
                        "bitrate must be an integer between $MIN_BITRATE and $MAX_BITRATE",
                    )
                } else {
                    try {
                        c.setParameters(Bundle().apply { putInt(MediaCodec.PARAMETER_KEY_VIDEO_BITRATE, bitrate.toInt()) })
                        activeConfig = activeConfig.copy(bitrate = bitrate.toInt())
                        Message.Response.ok(req.id)
                    } catch (e: IllegalStateException) {
                        Message.Response.error(req.id, Protocol.ErrorCode.FAILED, e.message)
                    }
                }
            }
            else -> Message.Response.error(req.id, Protocol.ErrorCode.UNKNOWN_COMMAND, req.cmd)
        }
    }

    /**
     * `set_mode` (§9.1, since 0.2), on the mode thread: stops the current camera and encoder, sends
     * STREAM_END, starts the new ones (their STREAM_CONFIG follows). If the new mode fails, goes
     * back to the previous one so the computer keeps a picture.
     */
    private fun setMode(req: Message.Request): Message.Response {
        fun error(code: String, message: String) = Message.Response.error(req.id, code, message)
        val args = req.args as? Map<*, *> ?: return error(Protocol.ErrorCode.INVALID_ARGS, "args missing")
        val cameraId = args["camera"] as? String
        val width = (args["width"] as? Long)?.toInt()
        val height = (args["height"] as? Long)?.toInt()
        val fps = (args["fps"] as? Long)?.toInt()
        val path = args["path"] as? String
        if (cameraId == null || width == null || height == null || fps == null) {
            return error(Protocol.ErrorCode.INVALID_ARGS, "camera, width, height and fps are required")
        }
        val device = requested.device ?: return error(Protocol.ErrorCode.UNSUPPORTED, "capabilities unknown")
        val (camera, support) = device.find(cameraId, width, height, fps, path)
            ?: return error(Protocol.ErrorCode.UNSUPPORTED, "camera $cameraId can't do ${width}x$height@$fps ${path ?: ""}".trim())

        val previous = activeConfig
        status("Switching to camera $cameraId · ${support.label}…")
        pipeline?.close()
        val end = Protocol.encode(Message.StreamEnd(Protocol.STREAM_VIDEO, "mode_change"))
        if (!queue.offer(Packet(end), 1, TimeUnit.SECONDS)) {
            fail("Network too slow: cannot even send the end of the stream")
            return error(Protocol.ErrorCode.FAILED, "network too slow")
        }
        return try {
            startPipeline(StreamConfig.from(support, camera, device))
            Message.Response.ok(req.id)
        } catch (e: Exception) {
            val why = e.message ?: e.javaClass.simpleName
            if (running.get()) {
                try {
                    startPipeline(previous)
                } catch (e2: Exception) {
                    if (running.get()) fail("Cannot restart the camera: ${e2.message}", "camera_lost")
                }
            }
            error(Protocol.ErrorCode.FAILED, why)
        }
    }

    private fun sendEvent(event: String, data: Any?) {
        val w = writer ?: return
        runCatching { control.execute { runCatching { w.write(Message.Event(event, data)) } } }
    }

    /**
     * Phone rotation for the computer's `auto` mode (§10), counter-clockwise from the natural
     * position like Display.getRotation(), quantised with a 15° hysteresis; null when flat.
     */
    private fun startOrientation() {
        if (Protocol.Feature.ORIENTATION !in shared) return
        val l = object : OrientationEventListener(appContext) {
            override fun onOrientationChanged(degrees: Int) {
                if (!running.get()) return
                val rotation = if (degrees == ORIENTATION_UNKNOWN) {
                    null
                } else {
                    // OrientationEventListener is clockwise; the protocol uses the display convention.
                    val current = if (lastRotation < 0 || lastRotation == 360) -1 else (360 - lastRotation) % 360
                    val clockwise = quantise(degrees, current)
                    (360 - clockwise) % 360
                }
                val code = rotation ?: 360 // 360 = flat/unknown
                if (code == lastRotation) return
                lastRotation = code
                sendEvent("orientation", mapOf("device_rotation" to rotation?.toLong()))
            }
        }
        orientationListener = l
        if (l.canDetectOrientation()) {
            l.enable()
        } else {
            sendEvent("orientation", mapOf("device_rotation" to null))
        }
    }

    // ------------------------------------------------------------------ camera selection

    private fun pickSize(chars: CameraCharacteristics, requested: StreamConfig): Size {
        val map = chars.get(CameraCharacteristics.SCALER_STREAM_CONFIGURATION_MAP)
            ?: throw IllegalStateException("Camera has no stream configuration map")
        val sizes = map.getOutputSizes(MediaCodec::class.java)?.takeIf { it.isNotEmpty() }
            ?: map.getOutputSizes(SurfaceTexture::class.java)
            ?: throw IllegalStateException("Camera reports no output size")

        sizes.firstOrNull { it.width == requested.width && it.height == requested.height }
            ?.let { return it }

        val notLarger = sizes.filter { it.width <= requested.width && it.height <= requested.height }
        val sixteenNine = notLarger.filter { it.width * 9 == it.height * 16 }
        return (sixteenNine.ifEmpty { notLarger })
            .maxByOrNull { it.width * it.height }
            ?: sizes.minByOrNull { it.width * it.height }!!
    }

    // ------------------------------------------------------------------ encoder

    private fun baseFormat(cfg: StreamConfig, tuning: Tuning): MediaFormat =
        MediaFormat.createVideoFormat(MediaFormat.MIMETYPE_VIDEO_AVC, cfg.width, cfg.height).apply {
            setInteger(
                MediaFormat.KEY_COLOR_FORMAT,
                MediaCodecInfo.CodecCapabilities.COLOR_FormatSurface,
            )
            setInteger(MediaFormat.KEY_BIT_RATE, cfg.bitrate)
            setInteger(MediaFormat.KEY_FRAME_RATE, cfg.fps)
            setInteger(MediaFormat.KEY_I_FRAME_INTERVAL, cfg.iFrameIntervalSec)
            // Keep producing frames when the scene is static, so the PC never starves.
            setLong(MediaFormat.KEY_REPEAT_PREVIOUS_FRAME_AFTER, 100_000L)
            if (Build.VERSION.SDK_INT >= 23) setInteger(MediaFormat.KEY_PRIORITY, 0) // realtime
            if (Build.VERSION.SDK_INT >= 30) setInteger(MediaFormat.KEY_LATENCY, 1)
            // High-speed capture (e.g. 120 fps): drop frames before the encoder to reach cfg.fps.
            // Public as KEY_MAX_FPS_TO_ENCODER since API 29, understood by older versions too.
            if (cfg.highSpeedFps > 0) setFloat("max-fps-to-encoder", cfg.fps.toFloat())
            // Qualcomm's own low-latency mode (#12): their encoders otherwise keep ~3 frames
            // (Redmi Note 9 Pro 1080p60: encoder 50 → 21 ms). Ignored by other encoders.
            if (tuning.qcomLowLatency) setInteger("vendor.qti-ext-enc-low-latency.enable", 1)
        }

    private fun configureEncoder(format: MediaFormat, name: String?): MediaCodec {
        val c = if (name != null) {
            MediaCodec.createByCodecName(name)
        } else {
            MediaCodec.createEncoderByType(MediaFormat.MIMETYPE_VIDEO_AVC)
        }
        try {
            c.configure(format, null, null, MediaCodec.CONFIGURE_FLAG_ENCODE)
            return c
        } catch (e: Exception) {
            c.release()
            throw e
        }
    }

    // ------------------------------------------------------------------ pipeline

    /** A camera or encoder problem while starting a [Pipeline]. */
    private class PipelineException(message: String) : IOException(message)

    /** Camera + encoder of one mode. `set_mode` replaces it as a whole. */
    private inner class Pipeline(
        val cfg: StreamConfig,
        val cameraId: String,
        val chars: CameraCharacteristics,
        /** Latency experiments (#12) read when this pipeline started. */
        val tuning: Tuning,
    ) {
        @Volatile var active = true
        @Volatile var codec: MediaCodec? = null
        @Volatile var surface: Surface? = null
        @Volatile var device: CameraDevice? = null
        @Volatile var session: CameraCaptureSession? = null
        @Volatile var cameraThread: HandlerThread? = null
        @Volatile var drainThread: Thread? = null
        /** Counted down once the camera repeats its request, or on the first error. */
        val started = CountDownLatch(1)
        @Volatile var startError: String? = null
        private val deviceClosed = CountDownLatch(1)

        fun onDeviceClosed() = deviceClosed.countDown()

        /** Fails the start; once streaming, a camera or encoder error ends the whole session. */
        fun failed(message: String, byeReason: String = "other") {
            if (!active) return
            if (started.count > 0) {
                startError = message
                started.countDown()
            } else {
                fail(message, byeReason)
            }
        }

        /** Stops waiting for the start (the session is stopping). */
        fun abort() {
            active = false
            started.countDown()
        }

        fun close() {
            abort()
            runCatching { session?.close() }
            device?.let {
                runCatching { it.close() }
                // Opening the next camera before this one is really closed can fail ("in use").
                runCatching { deviceClosed.await(2, TimeUnit.SECONDS) }
            }
            drainThread?.join(1000)
            runCatching { codec?.stop() }
            runCatching { codec?.release() }
            runCatching { surface?.release() }
            cameraThread?.quitSafely()
        }
    }

    /**
     * Starts the camera and the encoder for [wanted] and waits until the camera streams. Throws a
     * [PipelineException] (with everything closed) if the camera or the encoder refuses.
     */
    private fun startPipeline(wanted: StreamConfig): Pipeline {
        val cameraId = wanted.cameraId ?: Capabilities.pickCamera(cameraManager)
        val chars = cameraManager.getCameraCharacteristics(cameraId)
        val size = pickSize(chars, wanted)
        val cfg = wanted.copy(width = size.width, height = size.height, cameraId = cameraId)

        // A new stream: the encoder may use another clock (§11). PINGs wait for the first frame.
        clockChecked.set(false)
        realtimeClock = Build.VERSION.SDK_INT >= 23 &&
            chars.get(CameraCharacteristics.SENSOR_INFO_TIMESTAMP_SOURCE) ==
            CameraMetadata.SENSOR_INFO_TIMESTAMP_SOURCE_REALTIME
        sensorRealtime = null
        firstSensorNs = -1L
        cameraInfo = Protocol.CameraInfo(
            id = cameraId,
            facing = Capabilities.facingName(chars.get(CameraCharacteristics.LENS_FACING)),
            sensorOrientation = chars.get(CameraCharacteristics.SENSOR_ORIENTATION) ?: 0,
            mirrored = false,
        )
        // Drain-thread state, written before the drain thread starts.
        waitingForKeyframe = false
        pendingDiscontinuity = false
        outputFormat = null
        activeConfig = cfg
        pipelineStartMs = SystemClock.elapsedRealtime()

        val tuning = Tuning.load(appContext)
        tuningLabel = if (tuning.label.isEmpty()) "" else "\nLab settings: ${tuning.label}"
        val p = Pipeline(cfg, cameraId, chars, tuning)
        pipeline = p
        try {
            if (!running.get()) throw PipelineException("stopped")
            startEncoder(p)
            openCamera(p)
            if (!p.started.await(START_TIMEOUT_S, TimeUnit.SECONDS)) {
                throw PipelineException("the camera did not start within $START_TIMEOUT_S s")
            }
            p.startError?.let { throw PipelineException(it) }
            if (!p.active || !running.get()) throw PipelineException("stopped")
            return p
        } catch (e: Exception) {
            p.close()
            throw e as? PipelineException ?: PipelineException(e.message ?: e.javaClass.simpleName)
        }
    }

    private fun startEncoder(p: Pipeline) {
        val cfg = p.cfg
        // Baseline = no B-frames = lowest latency, with the level matching size × fps.
        // Some encoders refuse an explicit profile, so fall back to the vendor default.
        val c = if (cfg.baseline) {
            val baseline = baseFormat(cfg, p.tuning).apply {
                setInteger(
                    MediaFormat.KEY_PROFILE,
                    MediaCodecInfo.CodecProfileLevel.AVCProfileBaseline,
                )
                if (Build.VERSION.SDK_INT >= 23) setInteger(MediaFormat.KEY_LEVEL, cfg.avcLevel)
            }
            try {
                configureEncoder(baseline, cfg.encoderName)
            } catch (e: Exception) {
                configureEncoder(baseFormat(cfg, p.tuning), cfg.encoderName)
            }
        } else {
            configureEncoder(baseFormat(cfg, p.tuning), cfg.encoderName)
        }
        p.codec = c
        p.surface = c.createInputSurface()
        c.start()
        p.drainThread = thread(name = "oculibre-encoder") { drainLoop(p, c) }
    }

    private fun drainLoop(p: Pipeline, c: MediaCodec) {
        val info = MediaCodec.BufferInfo()
        var scratch = ByteArray(256 * 1024)
        try {
            while (running.get() && p.active) {
                val index = c.dequeueOutputBuffer(info, DEQUEUE_TIMEOUT_US)
                if (index == MediaCodec.INFO_OUTPUT_FORMAT_CHANGED) {
                    outputFormat = c.outputFormat
                    continue
                }
                if (index < 0) continue // try again later / buffers changed

                val buffer = c.getOutputBuffer(index)
                if (buffer != null && info.size > 0) {
                    if (scratch.size < info.size) scratch = ByteArray(info.size * 2)
                    buffer.position(info.offset)
                    buffer.limit(info.offset + info.size)
                    buffer.get(scratch, 0, info.size)
                    if (info.flags and MediaCodec.BUFFER_FLAG_CODEC_CONFIG != 0) {
                        sendStreamConfig(scratch.copyOf(info.size))
                    } else {
                        val isKey = info.flags and MediaCodec.BUFFER_FLAG_KEY_FRAME != 0
                        sendFrame(c, scratch, info.size, info.presentationTimeUs, isKey)
                    }
                }
                c.releaseOutputBuffer(index, false)

                if (info.flags and MediaCodec.BUFFER_FLAG_END_OF_STREAM != 0) break
            }
        } catch (e: Exception) {
            if (running.get()) p.failed("Encoder error: ${e.message ?: e.javaClass.simpleName}")
        }
    }

    /** SPS + PPS from the encoder: a new configuration of stream 1 (§8.1). */
    private fun sendStreamConfig(codecData: ByteArray) {
        val cfg = activeConfig
        configId = (configId + 1) and 0xFFFF
        val config = Protocol.VideoConfig(
            configId = configId,
            codecData = Base64Codec.encode(codecData),
            width = cfg.width.toLong(),
            height = cfg.height.toLong(),
            fps = cfg.fps.toLong(),
            captureFps = (if (cfg.highSpeedFps > 0) cfg.highSpeedFps else cfg.fps).toLong(),
            path = cfg.path,
            bitrate = cfg.bitrate.toLong(),
            color = colorOf(outputFormat),
            camera = cameraInfo,
        )
        val bytes = Protocol.encode(Message.StreamConfig(Protocol.STREAM_VIDEO, config))
        // Never dropped: wait for room (the frames behind it are useless without it).
        if (!queue.offer(Packet(bytes), 1, TimeUnit.SECONDS)) {
            fail("Network too slow: cannot even send the stream configuration")
        }
        waitingForKeyframe = false
    }

    /**
     * Drop policy (§12): if the network can't keep up and the queue is full, drop every frame
     * until the next keyframe (and ask the encoder for one right away); the first frame sent
     * after a gap carries DISCONTINUITY. Latency never piles up.
     */
    private fun sendFrame(c: MediaCodec, data: ByteArray, size: Int, ptsUs: Long, isKey: Boolean) {
        val number = frameNumber
        frameNumber = (frameNumber + 1) and 0xFFFF_FFFFL // counts dropped frames too
        if (configId == 0) return // no STREAM_CONFIG yet (should not happen)
        if (isKey) waitingForKeyframe = false
        if (waitingForKeyframe) {
            framesDropped.incrementAndGet()
            return
        }
        if (!clockChecked.get()) checkClock(ptsUs)
        val timestampUs = ptsUs - timestampShiftUs
        val encodedUs = nowUs()
        encodedDelay.add(encodedUs - timestampUs)
        val bytes = Protocol.encodeVideoFrame(
            Protocol.STREAM_VIDEO, timestampUs, isKey, pendingDiscontinuity, number, configId, data, 0, size,
        )
        if (queue.offer(Packet(bytes, timestampUs, encodedUs))) {
            pendingDiscontinuity = false
        } else {
            framesDropped.incrementAndGet()
            pendingDiscontinuity = true
            waitingForKeyframe = true
            requestKeyframe(c)
        }
    }

    private fun requestKeyframe(c: MediaCodec) {
        runCatching {
            c.setParameters(Bundle().apply { putInt(MediaCodec.PARAMETER_KEY_REQUEST_SYNC_FRAME, 0) })
        }
    }

    // ------------------------------------------------------------------ network

    private fun startSender() {
        val w = writer ?: return
        senderThread = thread(name = "oculibre-sender") {
            try {
                while (running.get()) {
                    val packet = queue.poll(100, TimeUnit.MILLISECONDS) ?: continue
                    w.writeRaw(packet.bytes)
                    bytesSent.addAndGet(packet.bytes.size.toLong())
                    if (packet.isFrame) {
                        framesSent.incrementAndGet()
                        // Capture → handed to the network: camera, encoder and queue.
                        if (clockChecked.get()) {
                            val now = nowUs()
                            phoneDelay.add(now - packet.ptsUs)
                            queueDelay.add(now - packet.encodedUs)
                        }
                    }
                }
            } catch (e: IOException) {
                if (running.get()) fail("Connection lost: ${e.message}")
            } catch (e: InterruptedException) {
                // shutting down
            }
        }
    }

    // ------------------------------------------------------------------ camera

    @SuppressLint("MissingPermission") // checked by MainActivity before start()
    private fun openCamera(p: Pipeline) {
        val ht = HandlerThread("oculibre-camera").also { it.start() }
        p.cameraThread = ht
        val handler = Handler(ht.looper)

        try {
            cameraManager.openCamera(p.cameraId, object : CameraDevice.StateCallback() {
                override fun onOpened(device: CameraDevice) {
                    if (!running.get() || !p.active) {
                        device.close()
                        return
                    }
                    p.device = device
                    if (p.cfg.highSpeedFps > 0 && Build.VERSION.SDK_INT >= 23) {
                        createHighSpeedSession(p, device, handler)
                    } else {
                        createSession(p, device, handler)
                    }
                }

                override fun onDisconnected(device: CameraDevice) {
                    device.close()
                    p.failed("Camera disconnected (used by another app?)", "camera_lost")
                }

                override fun onError(device: CameraDevice, error: Int) {
                    device.close()
                    p.failed("Camera error $error", "camera_lost")
                }

                override fun onClosed(device: CameraDevice) = p.onDeviceClosed()
            }, handler)
        } catch (e: Exception) {
            // CameraAccessException, IllegalArgumentException (unknown id), SecurityException…
            p.failed("Cannot open camera ${p.cameraId}: ${e.message ?: e.javaClass.simpleName}")
        }
    }

    @Suppress("DEPRECATION") // createCaptureSession(List, …) is the only variant available on API 21–27
    private fun createSession(p: Pipeline, device: CameraDevice, handler: Handler) {
        val surface = p.surface ?: return
        val cfg = p.cfg
        try {
            device.createCaptureSession(listOf(surface), object : CameraCaptureSession.StateCallback() {
                override fun onConfigured(session: CameraCaptureSession) {
                    if (!running.get() || !p.active) {
                        session.close()
                        return
                    }
                    p.session = session
                    try {
                        val request = device.createCaptureRequest(CameraDevice.TEMPLATE_RECORD).apply {
                            addTarget(surface)
                            set(CaptureRequest.CONTROL_MODE, CameraMetadata.CONTROL_MODE_AUTO)
                            val ranges = p.chars.get(CameraCharacteristics.CONTROL_AE_AVAILABLE_TARGET_FPS_RANGES)
                            // Above 30 fps, ask for [fps, fps] even when it is not listed: many HALs
                            // (Xiaomi, Samsung…) honour it although they hide it (docs/research/60fps.typ).
                            // The real rate is measured by cameraCallback and shown in the stats.
                            val range = fpsRangeFor(ranges, cfg.fps)
                                ?: if (cfg.fps > 30) Range(cfg.fps, cfg.fps) else null
                            range?.let { set(CaptureRequest.CONTROL_AE_TARGET_FPS_RANGE, it) }
                            val afModes = p.chars.get(CameraCharacteristics.CONTROL_AF_AVAILABLE_MODES)
                            if (afModes?.contains(CameraMetadata.CONTROL_AF_MODE_CONTINUOUS_VIDEO) == true) {
                                set(
                                    CaptureRequest.CONTROL_AF_MODE,
                                    CameraMetadata.CONTROL_AF_MODE_CONTINUOUS_VIDEO,
                                )
                            }
                        }.build()
                        session.setRepeatingRequest(request, cameraCallback, handler)
                        p.started.countDown()
                        status("Streaming ${cfg.width}x${cfg.height}…")
                    } catch (e: Exception) {
                        // CameraAccessException, IllegalStateException (closed meanwhile)…
                        p.failed("Camera request refused: ${e.message ?: e.javaClass.simpleName}")
                    }
                }

                override fun onConfigureFailed(session: CameraCaptureSession) {
                    p.failed("Camera session configuration failed")
                }
            }, handler)
        } catch (e: Exception) {
            p.failed("Camera session refused: ${e.message ?: e.javaClass.simpleName}")
        }
    }

    /**
     * Slow-motion session: fixed [StreamConfig.highSpeedFps] range, requests sent as bursts.
     * Exposure is capped at 1/highSpeedFps s, hence "needs light".
     */
    @TargetApi(23)
    private fun createHighSpeedSession(p: Pipeline, device: CameraDevice, handler: Handler) {
        val surface = p.surface ?: return
        val cfg = p.cfg
        try {
            device.createConstrainedHighSpeedCaptureSession(
                listOf(surface),
                object : CameraCaptureSession.StateCallback() {
                    override fun onConfigured(session: CameraCaptureSession) {
                        if (!running.get() || !p.active) {
                            session.close()
                            return
                        }
                        p.session = session
                        try {
                            val highSpeed = session as CameraConstrainedHighSpeedCaptureSession
                            val request = device.createCaptureRequest(CameraDevice.TEMPLATE_RECORD).apply {
                                addTarget(surface)
                                set(
                                    CaptureRequest.CONTROL_AE_TARGET_FPS_RANGE,
                                    Range(cfg.highSpeedFps, cfg.highSpeedFps),
                                )
                            }.build()
                            highSpeed.setRepeatingBurst(
                                highSpeed.createHighSpeedRequestList(request),
                                cameraCallback,
                                handler,
                            )
                            p.started.countDown()
                            status("Streaming ${cfg.width}x${cfg.height} (high-speed ${cfg.highSpeedFps} → ${cfg.fps} fps)…")
                        } catch (e: Exception) {
                            p.failed("High-speed request refused: ${e.message ?: e.javaClass.simpleName}")
                        }
                    }

                    override fun onConfigureFailed(session: CameraCaptureSession) {
                        p.failed("High-speed camera session refused")
                    }
                },
                handler,
            )
        } catch (e: Exception) {
            p.failed("High-speed camera session refused: ${e.message ?: e.javaClass.simpleName}")
        }
    }

    // ------------------------------------------------------------------ stats

    /** Counts frames actually delivered by the camera (≠ requested rate on some phones). */
    private val cameraCallback = object : CameraCaptureSession.CaptureCallback() {
        override fun onCaptureCompleted(
            session: CameraCaptureSession,
            request: CaptureRequest,
            result: TotalCaptureResult,
        ) {
            cameraFrames.incrementAndGet()
            val sensorNs = result.get(CaptureResult.SENSOR_TIMESTAMP) ?: return
            if (firstSensorNs < 0) firstSensorNs = sensorNs
            // How long the camera took to finish this picture, on the sensor's own clock: the one
            // closest to the timestamp of its first picture (they differ by the time spent asleep).
            val bootNs = SystemClock.elapsedRealtimeNanos()
            val monotonicNs = System.nanoTime()
            val realtime = sensorRealtime
                ?: (Math.abs(bootNs - sensorNs) < Math.abs(monotonicNs - sensorNs)).also { sensorRealtime = it }
            cameraDelay.add(((if (realtime) bootNs else monotonicNs) - sensorNs) / 1000)
        }
    }

    /**
     * Which clock the frame timestamps really use, decided on the first encoded frame (it left the
     * camera a few tens of milliseconds ago, so it is close to its clock and far from the other one:
     * the two differ by the time the phone has spent asleep).
     *
     * The *encoder* timestamps matter, not the sensor's: on many phones (Galaxy S23 among them) the
     * sensor counts in boot time but the camera service converts the frames it sends to a video
     * encoder to the monotonic clock, and SENSOR_INFO_TIMESTAMP_SOURCE says nothing about that.
     * If the encoder uses neither clock, its timestamps are shifted onto the sensor's.
     */
    private fun checkClock(ptsUs: Long) {
        val monotonicUs = System.nanoTime() / 1000
        val bootUs = SystemClock.elapsedRealtimeNanos() / 1000
        val toMonotonic = Math.abs(monotonicUs - ptsUs)
        val toBoot = Math.abs(bootUs - ptsUs)
        if (minOf(toMonotonic, toBoot) <= CLOCK_MATCH_US) {
            realtimeClock = toBoot < toMonotonic
            timestampShiftUs = 0
        } else {
            val sensorUs = firstSensorNs / 1000
            if (sensorUs > 0) {
                realtimeClock = Math.abs(bootUs - sensorUs) < Math.abs(monotonicUs - sensorUs)
                timestampShiftUs = ptsUs - sensorUs
            } else {
                // No reference at all: the encoder's own delay is not counted.
                realtimeClock = false
                timestampShiftUs = ptsUs - monotonicUs
            }
        }
        clockChecked.set(true)
    }

    private fun startStats() {
        statsThread = thread(name = "oculibre-stats") {
            var lastFrames = 0
            var lastCameraFrames = 0
            var lastBytes = 0L
            var seconds = 0
            try {
                while (running.get()) {
                    Thread.sleep(1000)
                    seconds++
                    val frames = framesSent.get()
                    val camera = cameraFrames.get()
                    val bytes = bytesSent.get()
                    val cfg = activeConfig
                    val bitrate = (bytes - lastBytes) * 8
                    val cameraFps = camera - lastCameraFrames
                    val sentFps = frames - lastFrames
                    lastFrames = frames
                    lastCameraFrames = camera
                    lastBytes = bytes

                    val phoneMs = phoneDelay.takeMs()
                    val cameraMs = cameraDelay.takeMs()
                    val encodedMs = encodedDelay.takeMs()
                    val queueMs = queueDelay.takeMs()
                    if (Protocol.Feature.STATS in shared) {
                        val data = linkedMapOf<String, Any?>(
                            "camera_fps" to cameraFps.toLong(),
                            "sent_fps" to sentFps.toLong(),
                            "bitrate" to bitrate,
                            "dropped" to framesDropped.get().toLong(),
                            "queue" to queue.size.toLong(),
                        )
                        // Averages of the last second, ms: capture → network, and its parts.
                        phoneMs?.let { data["latency_ms"] = it }
                        cameraMs?.let { data["camera_ms"] = it }
                        if (encodedMs != null && cameraMs != null) {
                            data["encoder_ms"] = Math.round(maxOf(0.0, encodedMs - cameraMs) * 10) / 10.0
                        }
                        queueMs?.let { data["queue_ms"] = it }
                        runCatching { writer?.write(Message.Event("stats", data)) }
                    }

                    val captureFps = if (cfg.highSpeedFps > 0) cfg.highSpeedFps else cfg.fps
                    // After a warm-up, warn if the camera ignores the requested rate.
                    val warm = seconds >= 3 && SystemClock.elapsedRealtime() - pipelineStartMs >= 3000
                    val warning = if (warm && cameraFps < captureFps * 0.85) {
                        "\n⚠ camera delivers $cameraFps fps, not $captureFps: this phone refuses it"
                    } else {
                        ""
                    }
                    val pc = hostStats?.let { h ->
                        val decoded = h["decoded_fps"] as? Long
                        fun ms(v: Any?) = when (v) {
                            is Long -> "$v"
                            is Double -> "%.0f".format(v)
                            else -> null
                        }
                        buildString {
                            append("\ncomputer: ")
                            append(if (decoded != null) "$decoded fps shown" else "connected")
                            ms(h["latency_ms"])?.let { append(" · latency $it ms") }
                        }
                    } ?: ""
                    status(
                        "Streaming ${cfg.width}x${cfg.height}$linkLabel$tuningLabel\n" +
                            "camera $cameraFps fps · sent $sentFps fps · ${"%.1f".format(bitrate / 1_000_000.0)} Mbit/s\n" +
                            "dropped: ${framesDropped.get()}" + pc + warning,
                    )
                }
            } catch (e: InterruptedException) {
                // shutting down
            }
        }
    }

    companion object {
        private const val CONNECT_TIMEOUT_MS = 3000
        /** Wi-Fi, paired: the known address gets a short try before looking on the network. */
        private const val FIRST_TRY_TIMEOUT_MS = 2000
        private const val DISCOVERY_TIMEOUT_MS = 6000L
        private const val HANDSHAKE_TIMEOUT_MS = 5000
        private const val SILENCE_TIMEOUT_MS = 5000
        private const val BYE_TIMEOUT_MS = 300L
        private const val DEQUEUE_TIMEOUT_US = 100_000L
        /** ~200 ms at 30 fps: beyond that we'd rather drop than add latency. */
        private const val QUEUE_CAPACITY = 6
        private const val MIN_BITRATE = 100_000L
        private const val MAX_BITRATE = 100_000_000L
        private const val PREF_INSTALL_ID = "install_id"
        /** A first frame closer than this to a clock uses that clock. */
        private const val CLOCK_MATCH_US = 2_000_000L
        /** Time allowed for a camera to open and start streaming. */
        private const val START_TIMEOUT_S = 10L

        private val OUR_FEATURES = listOf(
            Protocol.Feature.VIDEO_H264,
            Protocol.Feature.CLOCK,
            Protocol.Feature.ORIENTATION,
            Protocol.Feature.STATS,
            Protocol.Feature.CONTROL_KEYFRAME,
            Protocol.Feature.CONTROL_BITRATE,
            Protocol.Feature.CONTROL_MODE,
        )

        /**
         * Rounds a clockwise angle (0–359) to 0/90/180/270. Keeps [current] (or -1 for none)
         * until the angle is more than 60° away from it, so the value doesn't flicker at 45°.
         */
        fun quantise(degrees: Int, current: Int): Int {
            if (current >= 0) {
                val diff = Math.abs(((degrees - current) % 360 + 540) % 360 - 180)
                if (diff <= 60) return current
            }
            return ((degrees + 45) / 90 * 90) % 360
        }

        /** Colour description of the encoder output (§8.1); absent keys keep the defaults. */
        private fun colorOf(format: MediaFormat?): Protocol.Color {
            val d = Protocol.Color()
            if (format == null || Build.VERSION.SDK_INT < 24) return d
            fun int(key: String) = if (format.containsKey(key)) format.getInteger(key) else null
            val standard = when (int(MediaFormat.KEY_COLOR_STANDARD)) {
                MediaFormat.COLOR_STANDARD_BT709 -> "bt709"
                MediaFormat.COLOR_STANDARD_BT601_PAL, MediaFormat.COLOR_STANDARD_BT601_NTSC -> "bt601"
                MediaFormat.COLOR_STANDARD_BT2020 -> "bt2020"
                null -> null
                else -> "unknown"
            }
            val transfer = when (int(MediaFormat.KEY_COLOR_TRANSFER)) {
                MediaFormat.COLOR_TRANSFER_SDR_VIDEO -> "bt709"
                MediaFormat.COLOR_TRANSFER_ST2084 -> "pq"
                MediaFormat.COLOR_TRANSFER_HLG -> "hlg"
                null -> null
                else -> "unknown"
            }
            val range = when (int(MediaFormat.KEY_COLOR_RANGE)) {
                MediaFormat.COLOR_RANGE_FULL -> "full"
                MediaFormat.COLOR_RANGE_LIMITED -> "limited"
                else -> null
            }
            return Protocol.Color(
                primaries = standard ?: d.primaries,
                transfer = transfer ?: d.transfer,
                matrix = standard ?: d.matrix,
                range = range ?: d.range,
            )
        }
    }
}
