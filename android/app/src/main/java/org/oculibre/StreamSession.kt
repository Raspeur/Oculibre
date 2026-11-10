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
import android.hardware.camera2.TotalCaptureResult
import android.media.MediaCodec
import android.media.MediaCodecInfo
import android.media.MediaFormat
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.HandlerThread
import android.util.Range
import android.util.Size
import android.view.Surface
import java.io.IOException
import java.io.OutputStream
import java.net.InetSocketAddress
import java.net.Socket
import java.util.concurrent.ArrayBlockingQueue
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicLong
import kotlin.concurrent.thread

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
) {
    companion object {
        fun from(support: ModeSupport, caps: DeviceCapabilities): StreamConfig {
            val mode = support.mode
            return fromMode(mode, caps).copy(highSpeedFps = support.highSpeedFps)
        }

        private fun fromMode(mode: QualityMode, caps: DeviceCapabilities) = StreamConfig(
            width = mode.width,
            height = mode.height,
            fps = mode.fps,
            bitrate = mode.bitrate,
            avcLevel = mode.avcLevel,
            baseline = caps.baselineMaxLevel >= mode.avcLevel,
            encoderName = caps.encoderName,
            cameraId = caps.cameraId,
        )
    }
}

/**
 * Phase-1 pipeline:
 *
 *   Camera2 ──► encoder input Surface ──► MediaCodec (H.264) ──► queue ──► TCP socket
 *
 * The stream is raw H.264 Annex B (no Oculibre framing yet), so the PC can simply run
 * `ffplay -f h264 tcp://0.0.0.0:47100?listen=1`. The phone is the TCP client.
 *
 * Threads: setup, camera (HandlerThread), encoder drain, sender, stats, stop.
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

    private class Packet(val data: ByteArray, val isConfig: Boolean)

    private val cameraManager = context.getSystemService(Context.CAMERA_SERVICE) as CameraManager

    private val running = AtomicBoolean(false)
    private val stopped = AtomicBoolean(false)

    @Volatile private var socket: Socket? = null
    @Volatile private var codec: MediaCodec? = null
    @Volatile private var inputSurface: Surface? = null
    @Volatile private var camera: CameraDevice? = null
    @Volatile private var captureSession: CameraCaptureSession? = null
    @Volatile private var cameraThread: HandlerThread? = null
    @Volatile private var activeConfig: StreamConfig = requested

    @Volatile private var setupThread: Thread? = null
    @Volatile private var drainThread: Thread? = null
    @Volatile private var senderThread: Thread? = null
    @Volatile private var statsThread: Thread? = null

    private val queue = ArrayBlockingQueue<Packet>(QUEUE_CAPACITY)

    /** Only touched by the drain thread. */
    private var waitingForKeyframe = false

    private val framesSent = AtomicInteger()
    private val framesDropped = AtomicInteger()
    private val cameraFrames = AtomicInteger()
    private val bytesSent = AtomicLong()

    // ------------------------------------------------------------------ lifecycle

    fun start() {
        if (!running.compareAndSet(false, true)) return
        setupThread = thread(name = "oculibre-setup") {
            try {
                val s = Socket()
                s.tcpNoDelay = true
                s.connect(InetSocketAddress(host, port), CONNECT_TIMEOUT_MS)
                socket = s
                if (!running.get()) return@thread

                status("Connected — opening camera…")
                val cameraId = requested.cameraId ?: Capabilities.pickCamera(cameraManager)
                val chars = cameraManager.getCameraCharacteristics(cameraId)
                val size = pickSize(chars)
                val cfg = requested.copy(width = size.width, height = size.height)
                activeConfig = cfg

                startEncoder(cfg)
                startSender(s.getOutputStream())
                openCamera(cameraId, chars, cfg)
                startStats()
            } catch (e: Exception) {
                fail(e)
            }
        }
    }

    fun stop() = shutdown(null)

    private fun fail(e: Throwable) = fail(e.message ?: e.javaClass.simpleName)

    private fun fail(message: String) = shutdown(message)

    private fun shutdown(error: String?) {
        if (!stopped.compareAndSet(false, true)) return
        running.set(false)
        thread(name = "oculibre-stop") {
            // Closing the socket first unblocks a sender stuck in write().
            runCatching { socket?.close() }
            setupThread?.join(CONNECT_TIMEOUT_MS + 1000L)
            runCatching { captureSession?.close() }
            runCatching { camera?.close() }
            drainThread?.join(1000)
            senderThread?.join(1000)
            statsThread?.interrupt()
            runCatching { codec?.stop() }
            runCatching { codec?.release() }
            runCatching { inputSurface?.release() }
            cameraThread?.quitSafely()
            queue.clear()
            listener.onStopped(this, error)
        }
    }

    private fun status(text: String) = listener.onStatus(this, text)

    // ------------------------------------------------------------------ camera selection

    private fun pickSize(chars: CameraCharacteristics): Size {
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

    private fun baseFormat(cfg: StreamConfig): MediaFormat =
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

    private fun startEncoder(cfg: StreamConfig) {
        // Baseline = no B-frames = lowest latency, with the level matching size × fps.
        // Some encoders refuse an explicit profile, so fall back to the vendor default.
        val c = if (cfg.baseline) {
            val baseline = baseFormat(cfg).apply {
                setInteger(
                    MediaFormat.KEY_PROFILE,
                    MediaCodecInfo.CodecProfileLevel.AVCProfileBaseline,
                )
                if (Build.VERSION.SDK_INT >= 23) setInteger(MediaFormat.KEY_LEVEL, cfg.avcLevel)
            }
            try {
                configureEncoder(baseline, cfg.encoderName)
            } catch (e: Exception) {
                configureEncoder(baseFormat(cfg), cfg.encoderName)
            }
        } else {
            configureEncoder(baseFormat(cfg), cfg.encoderName)
        }
        codec = c
        inputSurface = c.createInputSurface()
        c.start()
        drainThread = thread(name = "oculibre-encoder") { drainLoop(c) }
    }

    private fun drainLoop(c: MediaCodec) {
        val info = MediaCodec.BufferInfo()
        try {
            while (running.get()) {
                val index = c.dequeueOutputBuffer(info, DEQUEUE_TIMEOUT_US)
                if (index < 0) continue // try again later / format changed / buffers changed

                val buffer = c.getOutputBuffer(index)
                if (buffer != null && info.size > 0) {
                    buffer.position(info.offset)
                    buffer.limit(info.offset + info.size)
                    val data = ByteArray(info.size)
                    buffer.get(data)
                    val isConfig = info.flags and MediaCodec.BUFFER_FLAG_CODEC_CONFIG != 0
                    val isKey = info.flags and MediaCodec.BUFFER_FLAG_KEY_FRAME != 0
                    enqueue(c, data, isConfig, isKey)
                }
                c.releaseOutputBuffer(index, false)

                if (info.flags and MediaCodec.BUFFER_FLAG_END_OF_STREAM != 0) break
            }
        } catch (e: Exception) {
            if (running.get()) fail(e)
        }
    }

    /**
     * Drop policy: if the network can't keep up and the queue is full, drop every frame
     * until the next keyframe (and ask the encoder for one right away). Latency never piles up.
     */
    private fun enqueue(c: MediaCodec, data: ByteArray, isConfig: Boolean, isKey: Boolean) {
        if (isKey) waitingForKeyframe = false
        if (waitingForKeyframe && !isConfig) {
            framesDropped.incrementAndGet()
            return
        }
        val accepted = if (isConfig) {
            queue.offer(Packet(data, true), 1, TimeUnit.SECONDS)
        } else {
            queue.offer(Packet(data, false))
        }
        if (!accepted) {
            framesDropped.incrementAndGet()
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

    private fun startSender(out: OutputStream) {
        senderThread = thread(name = "oculibre-sender") {
            try {
                while (running.get()) {
                    val packet = queue.poll(100, TimeUnit.MILLISECONDS) ?: continue
                    out.write(packet.data)
                    bytesSent.addAndGet(packet.data.size.toLong())
                    if (!packet.isConfig) framesSent.incrementAndGet()
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
    private fun openCamera(cameraId: String, chars: CameraCharacteristics, cfg: StreamConfig) {
        val ht = HandlerThread("oculibre-camera").also { it.start() }
        cameraThread = ht
        val handler = Handler(ht.looper)

        cameraManager.openCamera(cameraId, object : CameraDevice.StateCallback() {
            override fun onOpened(device: CameraDevice) {
                if (!running.get()) {
                    device.close()
                    return
                }
                camera = device
                if (cfg.highSpeedFps > 0 && Build.VERSION.SDK_INT >= 23) {
                    createHighSpeedSession(device, cfg, handler)
                } else {
                    createSession(device, chars, cfg, handler)
                }
            }

            override fun onDisconnected(device: CameraDevice) {
                device.close()
                fail("Camera disconnected (used by another app?)")
            }

            override fun onError(device: CameraDevice, error: Int) {
                device.close()
                fail("Camera error $error")
            }
        }, handler)
    }

    @Suppress("DEPRECATION") // createCaptureSession(List, …) is the only variant available on API 21–27
    private fun createSession(
        device: CameraDevice,
        chars: CameraCharacteristics,
        cfg: StreamConfig,
        handler: Handler,
    ) {
        val surface = inputSurface ?: return
        try {
            device.createCaptureSession(listOf(surface), object : CameraCaptureSession.StateCallback() {
                override fun onConfigured(session: CameraCaptureSession) {
                    if (!running.get()) {
                        session.close()
                        return
                    }
                    captureSession = session
                    try {
                        val request = device.createCaptureRequest(CameraDevice.TEMPLATE_RECORD).apply {
                            addTarget(surface)
                            set(CaptureRequest.CONTROL_MODE, CameraMetadata.CONTROL_MODE_AUTO)
                            val ranges = chars.get(CameraCharacteristics.CONTROL_AE_AVAILABLE_TARGET_FPS_RANGES)
                            // Above 30 fps, ask for [fps, fps] even when it is not listed: many HALs
                            // (Xiaomi, Samsung…) honour it although they hide it (docs/research/60fps.md).
                            // The real rate is measured by cameraCallback and shown in the stats.
                            val range = fpsRangeFor(ranges, cfg.fps)
                                ?: if (cfg.fps > 30) Range(cfg.fps, cfg.fps) else null
                            range?.let { set(CaptureRequest.CONTROL_AE_TARGET_FPS_RANGE, it) }
                            val afModes = chars.get(CameraCharacteristics.CONTROL_AF_AVAILABLE_MODES)
                            if (afModes?.contains(CameraMetadata.CONTROL_AF_MODE_CONTINUOUS_VIDEO) == true) {
                                set(
                                    CaptureRequest.CONTROL_AF_MODE,
                                    CameraMetadata.CONTROL_AF_MODE_CONTINUOUS_VIDEO,
                                )
                            }
                        }.build()
                        session.setRepeatingRequest(request, cameraCallback, handler)
                        status("Streaming ${cfg.width}x${cfg.height}…")
                    } catch (e: CameraAccessException) {
                        fail(e)
                    } catch (e: IllegalStateException) {
                        if (running.get()) fail(e)
                    }
                }

                override fun onConfigureFailed(session: CameraCaptureSession) {
                    fail("Camera session configuration failed")
                }
            }, handler)
        } catch (e: CameraAccessException) {
            fail(e)
        }
    }

    /**
     * Slow-motion session: fixed [cfg.highSpeedFps] range, requests sent as bursts.
     * Exposure is capped at 1/highSpeedFps s, hence "needs light".
     */
    @TargetApi(23)
    private fun createHighSpeedSession(device: CameraDevice, cfg: StreamConfig, handler: Handler) {
        val surface = inputSurface ?: return
        try {
            device.createConstrainedHighSpeedCaptureSession(
                listOf(surface),
                object : CameraCaptureSession.StateCallback() {
                    override fun onConfigured(session: CameraCaptureSession) {
                        if (!running.get()) {
                            session.close()
                            return
                        }
                        captureSession = session
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
                            status("Streaming ${cfg.width}x${cfg.height} (high-speed ${cfg.highSpeedFps} → ${cfg.fps} fps)…")
                        } catch (e: CameraAccessException) {
                            fail(e)
                        } catch (e: IllegalArgumentException) {
                            fail(e)
                        } catch (e: IllegalStateException) {
                            if (running.get()) fail(e)
                        }
                    }

                    override fun onConfigureFailed(session: CameraCaptureSession) {
                        fail("High-speed camera session refused")
                    }
                },
                handler,
            )
        } catch (e: CameraAccessException) {
            fail(e)
        } catch (e: IllegalArgumentException) {
            fail(e)
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
        }
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
                    val mbps = (bytes - lastBytes) * 8 / 1_000_000.0
                    val cameraFps = camera - lastCameraFrames
                    val captureFps = if (cfg.highSpeedFps > 0) cfg.highSpeedFps else cfg.fps
                    // After a warm-up, warn if the camera ignores the requested rate.
                    val warning = if (seconds >= 3 && cameraFps < captureFps * 0.85) {
                        "\n⚠ camera delivers $cameraFps fps, not $captureFps: this phone refuses it"
                    } else {
                        ""
                    }
                    status(
                        "Streaming ${cfg.width}x${cfg.height}\n" +
                            "camera $cameraFps fps · sent ${frames - lastFrames} fps · ${"%.1f".format(mbps)} Mbit/s\n" +
                            "dropped: ${framesDropped.get()}" + warning,
                    )
                    lastFrames = frames
                    lastCameraFrames = camera
                    lastBytes = bytes
                }
            } catch (e: InterruptedException) {
                // shutting down
            }
        }
    }

    companion object {
        private const val CONNECT_TIMEOUT_MS = 3000
        private const val DEQUEUE_TIMEOUT_US = 100_000L
        /** ~200 ms at 30 fps: beyond that we'd rather drop than add latency. */
        private const val QUEUE_CAPACITY = 6
    }
}
