package org.oculibre

import android.Manifest
import android.annotation.SuppressLint
import android.annotation.TargetApi
import android.app.Activity
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.pm.PackageManager
import android.graphics.Typeface
import android.hardware.camera2.CameraCaptureSession
import android.hardware.camera2.CameraCharacteristics
import android.hardware.camera2.CameraDevice
import android.hardware.camera2.CameraManager
import android.hardware.camera2.CameraMetadata
import android.hardware.camera2.CaptureRequest
import android.hardware.camera2.CaptureResult
import android.hardware.camera2.TotalCaptureResult
import android.hardware.camera2.params.OutputConfiguration
import android.hardware.camera2.params.SessionConfiguration
import android.media.MediaCodec
import android.media.MediaCodecInfo
import android.media.MediaFormat
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.HandlerThread
import android.os.Looper
import android.util.Range
import android.util.Size
import android.view.Surface
import android.widget.Button
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import android.widget.Toast
import java.util.Collections
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executor
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.concurrent.thread

/**
 * Lab: tries several ways of getting a *real* 60 fps out of the back camera, and measures
 * what the camera actually delivers (frame timestamps, exposure, ISO), see docs/research/60fps.md.
 *
 * The camera feeds a real H.264 encoder (output discarded), like the streaming pipeline,
 * because some HALs pick their sensor mode from the stream types.
 */
class LabActivity : Activity() {

    private class Strategy(
        val name: String,
        /** Pass the request as session parameters (SessionConfiguration, API 28+). */
        val sessionParams: Boolean,
        val configure: (CaptureRequest.Builder) -> Unit,
        val isReference: Boolean = false,
    )

    private class Sample(
        val timestampNs: Long,
        val exposureNs: Long?,
        val iso: Int?,
        val frameDurationNs: Long?,
        val aeRange: Range<Int>?,
    )

    private lateinit var output: TextView
    private lateinit var runButton: Button
    private val ui = Handler(Looper.getMainLooper())
    private val log = StringBuilder()
    private val cancelled = AtomicBoolean(false)

    @Volatile private var refExposureNs: Long? = null
    @Volatile private var refIso: Int? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val pad = (16 * resources.displayMetrics.density).toInt()

        runButton = Button(this).apply {
            text = "Run 60 fps tests (~30 s)"
            setOnClickListener { startTests() }
        }
        val copyButton = Button(this).apply {
            text = "Copy results"
            setOnClickListener {
                val clipboard = getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
                clipboard.setPrimaryClip(ClipData.newPlainText("Oculibre lab", synchronized(log) { log.toString() }))
                Toast.makeText(this@LabActivity, "Copied", Toast.LENGTH_SHORT).show()
            }
        }
        output = TextView(this).apply {
            typeface = Typeface.MONOSPACE
            textSize = 11f
            setTextIsSelectable(true)
            setPadding(0, pad, 0, 0)
            text = "Point the back camera at your usual scene (normal room light), keep the phone still, " +
                "then run the tests. Close any app using the camera first."
        }
        val content = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(pad, pad, pad, pad)
            addView(runButton)
            addView(copyButton)
            addView(output)
        }
        setContentView(ScrollView(this).apply {
            fitsSystemWindows = true
            addView(content)
        })
    }

    override fun onDestroy() {
        cancelled.set(true)
        super.onDestroy()
    }

    private fun startTests() {
        if (Build.VERSION.SDK_INT >= 23 &&
            checkSelfPermission(Manifest.permission.CAMERA) != PackageManager.PERMISSION_GRANTED
        ) {
            requestPermissions(arrayOf(Manifest.permission.CAMERA), 1)
            return
        }
        synchronized(log) { log.setLength(0) }
        runButton.isEnabled = false
        thread(name = "oculibre-lab") { runLab() }
    }

    override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<out String>, grantResults: IntArray) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        if (grantResults.firstOrNull() == PackageManager.PERMISSION_GRANTED) startTests()
    }

    private fun append(line: String) {
        val text = synchronized(log) {
            log.append(line).append('\n')
            log.toString()
        }
        ui.post { output.text = text }
    }

    // ------------------------------------------------------------------ main sequence

    private fun runLab() {
        val cameraThread = HandlerThread("oculibre-lab-camera").also { it.start() }
        val handler = Handler(cameraThread.looper)
        val drainRunning = AtomicBoolean(true)
        var device: CameraDevice? = null
        var codec: MediaCodec? = null
        var surface: Surface? = null
        var drain: Thread? = null

        try {
            val cameraManager = getSystemService(Context.CAMERA_SERVICE) as CameraManager
            val cameraId = Capabilities.pickCamera(cameraManager)
            val chars = cameraManager.getCameraCharacteristics(cameraId)
            val size = pickSize(chars)
            describe(cameraId, chars, size)

            val (c, s) = createEncoder(size)
            codec = c
            surface = s
            drain = thread(name = "oculibre-lab-drain") {
                val info = MediaCodec.BufferInfo()
                while (drainRunning.get()) {
                    val index = try {
                        c.dequeueOutputBuffer(info, 50_000)
                    } catch (e: IllegalStateException) {
                        break
                    }
                    if (index >= 0) c.releaseOutputBuffer(index, false)
                }
            }

            append("")
            append("Strategy                                   | result")
            append("-------------------------------------------+------------------------------------------")
            for (strategy in buildStrategies(chars)) {
                if (cancelled.get()) break
                // A fresh camera for every strategy: a vendor key can crash the session and
                // close the device (seen on the S23 with S3b).
                val camera = try {
                    openCamera(cameraManager, cameraId, handler)
                } catch (e: Exception) {
                    append(row(strategy, "camera open failed: ${e.message}"))
                    continue
                }
                device = camera
                try {
                    append(runStrategy(camera, s, strategy, handler))
                } finally {
                    runCatching { camera.close() }
                    device = null
                    Thread.sleep(500)
                }
            }
        } catch (e: Exception) {
            append("ERROR: ${e.javaClass.simpleName}: ${e.message}")
        } finally {
            drainRunning.set(false)
            drain?.join(1000)
            runCatching { device?.close() }
            runCatching { codec?.stop() }
            runCatching { codec?.release() }
            runCatching { surface?.release() }
            cameraThread.quitSafely()
            append("\nDone. Tap \"Copy results\" and paste them into devices/<brand>/<model>/report.md")
            ui.post { runButton.isEnabled = true }
        }
    }

    private fun buildStrategies(chars: CameraCharacteristics): List<Strategy> {
        val ae = CaptureRequest.CONTROL_AE_TARGET_FPS_RANGE
        val list = mutableListOf(
            Strategy("S0  reference AE[30,30]", false, { it.set(ae, Range(30, 30)) }, isReference = true),
            Strategy("S1  AE[60,60] per request", false, { it.set(ae, Range(60, 60)) }),
            Strategy("S1b AE[30,60] per request", false, { it.set(ae, Range(30, 60)) }),
        )
        if (Build.VERSION.SDK_INT >= 28) {
            list += Strategy("S2  AE[60,60] session parameter", true, { it.set(ae, Range(60, 60)) })
        } else {
            append("(S2: session parameters need Android 9+, skipped)")
        }
        if (Build.VERSION.SDK_INT >= 29) {
            list += vendorStrategies()
        } else {
            append("(S3/S4: vendor keys need Android 10+, skipped)")
        }
        val caps = chars.get(CameraCharacteristics.REQUEST_AVAILABLE_CAPABILITIES)
        if (caps?.contains(CameraMetadata.REQUEST_AVAILABLE_CAPABILITIES_MANUAL_SENSOR) == true) {
            list += Strategy("S5  manual AE off, frameDuration 16.7ms", false, { b ->
                b.set(CaptureRequest.CONTROL_AE_MODE, CameraMetadata.CONTROL_AE_MODE_OFF)
                b.set(CaptureRequest.SENSOR_FRAME_DURATION, 16_666_666L)
                b.set(CaptureRequest.SENSOR_EXPOSURE_TIME, minOf(refExposureNs ?: 10_000_000L, 16_000_000L))
                b.set(CaptureRequest.SENSOR_SENSITIVITY, refIso ?: 400)
            })
        } else {
            append("(S5: no MANUAL_SENSOR capability, skipped)")
        }
        return list
    }

    @TargetApi(29)
    private fun vendorStrategies(): List<Strategy> {
        val ae = CaptureRequest.CONTROL_AE_TARGET_FPS_RANGE
        val maxFps = CaptureRequest.Key("samsung.android.control.recordingMaxFps", Int::class.javaObjectType)
        val minFps = CaptureRequest.Key("samsung.android.control.recordingMinFps", Int::class.javaObjectType)
        val noFpsLimits = CaptureRequest.Key("org.quic.camera.manualExposure.disableFPSLimits", Byte::class.javaObjectType)
        return listOf(
            Strategy("S3a Samsung recordingFps 24-60, AE[24,60]", true, { b ->
                b.set(maxFps, 60)
                b.set(minFps, 24)
                b.set(ae, Range(24, 60))
            }),
            Strategy("S3b Samsung recordingFps 60-60, AE[60,60]", true, { b ->
                b.set(maxFps, 60)
                b.set(minFps, 60)
                b.set(ae, Range(60, 60))
            }),
            Strategy("S4a Qualcomm disableFPSLimits, AE[60,60]", false, { b ->
                b.set(noFpsLimits, 1.toByte())
                b.set(ae, Range(60, 60))
            }),
            Strategy("S4b same as S4a, session parameters", true, { b ->
                b.set(noFpsLimits, 1.toByte())
                b.set(ae, Range(60, 60))
            }),
        )
    }

    // ------------------------------------------------------------------ one strategy

    private fun runStrategy(device: CameraDevice, surface: Surface, strategy: Strategy, handler: Handler): String {
        val samples = Collections.synchronizedList(ArrayList<Sample>())
        val callback = object : CameraCaptureSession.CaptureCallback() {
            override fun onCaptureCompleted(
                session: CameraCaptureSession,
                request: CaptureRequest,
                result: TotalCaptureResult,
            ) {
                val ts = result.get(CaptureResult.SENSOR_TIMESTAMP) ?: return
                samples += Sample(
                    ts,
                    result.get(CaptureResult.SENSOR_EXPOSURE_TIME),
                    result.get(CaptureResult.SENSOR_SENSITIVITY),
                    result.get(CaptureResult.SENSOR_FRAME_DURATION),
                    result.get(CaptureResult.CONTROL_AE_TARGET_FPS_RANGE),
                )
            }
        }

        val request = try {
            device.createCaptureRequest(CameraDevice.TEMPLATE_RECORD).apply {
                addTarget(surface)
                strategy.configure(this)
            }.build()
        } catch (e: IllegalArgumentException) {
            return row(strategy, "key not available on this phone")
        }

        val session = try {
            createSession(device, surface, strategy.sessionParams, request, handler)
        } catch (e: Exception) {
            return row(strategy, "session error: ${e.javaClass.simpleName} ${e.message ?: ""}".trim())
        } ?: return row(strategy, "session refused by the camera")

        try {
            session.setRepeatingRequest(request, callback, handler)
            Thread.sleep(1500) // let AE settle
            samples.clear()
            Thread.sleep(2000)
            session.stopRepeating()
        } catch (e: Exception) {
            runCatching { session.close() }
            return row(strategy, "error: ${e.javaClass.simpleName} ${e.message ?: ""}".trim())
        }
        session.close()
        Thread.sleep(400)

        val list = synchronized(samples) { samples.toList() }.sortedBy { it.timestampNs }
        if (list.size < 2) return row(strategy, "no frames received")
        val fps = (list.size - 1) * 1e9 / (list.last().timestampNs - list.first().timestampNs)
        val mid = list[list.size / 2]
        if (strategy.isReference) {
            refExposureNs = mid.exposureNs
            refIso = mid.iso
        }
        val exposure = mid.exposureNs?.let { "%.1fms".format(it / 1e6) } ?: "?"
        val frameDuration = mid.frameDurationNs?.let { "%.1fms".format(it / 1e6) } ?: "?"
        val verdict = if (fps >= 55) "  <== 60 fps!" else ""
        return row(
            strategy,
            "%.1f fps · exp %s ISO %s · frame %s · AE%s%s".format(
                fps, exposure, mid.iso ?: "?", frameDuration, mid.aeRange ?: "?", verdict,
            ),
        )
    }

    private fun row(strategy: Strategy, result: String) = strategy.name.padEnd(43) + "| " + result

    private fun createSession(
        device: CameraDevice,
        surface: Surface,
        withParams: Boolean,
        request: CaptureRequest,
        handler: Handler,
    ): CameraCaptureSession? {
        val latch = CountDownLatch(1)
        var created: CameraCaptureSession? = null
        val stateCallback = object : CameraCaptureSession.StateCallback() {
            override fun onConfigured(session: CameraCaptureSession) {
                created = session
                latch.countDown()
            }

            override fun onConfigureFailed(session: CameraCaptureSession) {
                latch.countDown()
            }
        }
        if (withParams && Build.VERSION.SDK_INT >= 28) {
            createSessionWithParameters(device, surface, request, stateCallback, handler)
        } else {
            @Suppress("DEPRECATION")
            device.createCaptureSession(listOf(surface), stateCallback, handler)
        }
        latch.await(5, TimeUnit.SECONDS)
        return created
    }

    @TargetApi(28)
    private fun createSessionWithParameters(
        device: CameraDevice,
        surface: Surface,
        request: CaptureRequest,
        stateCallback: CameraCaptureSession.StateCallback,
        handler: Handler,
    ) {
        val config = SessionConfiguration(
            SessionConfiguration.SESSION_REGULAR,
            listOf(OutputConfiguration(surface)),
            Executor { handler.post(it) },
            stateCallback,
        )
        config.sessionParameters = request
        device.createCaptureSession(config)
    }

    // ------------------------------------------------------------------ setup helpers

    @SuppressLint("MissingPermission") // checked in startTests()
    private fun openCamera(cameraManager: CameraManager, id: String, handler: Handler): CameraDevice {
        val latch = CountDownLatch(1)
        var opened: CameraDevice? = null
        var error: String? = null
        cameraManager.openCamera(id, object : CameraDevice.StateCallback() {
            override fun onOpened(camera: CameraDevice) {
                opened = camera
                latch.countDown()
            }

            override fun onDisconnected(camera: CameraDevice) {
                camera.close()
                error = "disconnected (used by another app?)"
                latch.countDown()
            }

            override fun onError(camera: CameraDevice, code: Int) {
                camera.close()
                error = "error $code"
                latch.countDown()
            }
        }, handler)
        if (!latch.await(5, TimeUnit.SECONDS)) throw IllegalStateException("camera open timeout")
        return opened ?: throw IllegalStateException("camera: $error")
    }

    private fun pickSize(chars: CameraCharacteristics): Size {
        val sizes = chars.get(CameraCharacteristics.SCALER_STREAM_CONFIGURATION_MAP)
            ?.getOutputSizes(MediaCodec::class.java)?.toList().orEmpty()
        return sizes.firstOrNull { it.width == 1920 && it.height == 1080 }
            ?: sizes.firstOrNull { it.width == 1280 && it.height == 720 }
            ?: sizes.firstOrNull()
            ?: throw IllegalStateException("camera reports no video size")
    }

    private fun createEncoder(size: Size): Pair<MediaCodec, Surface> {
        for (fps in intArrayOf(60, 30)) {
            val format = MediaFormat.createVideoFormat(MediaFormat.MIMETYPE_VIDEO_AVC, size.width, size.height).apply {
                setInteger(MediaFormat.KEY_COLOR_FORMAT, MediaCodecInfo.CodecCapabilities.COLOR_FormatSurface)
                setInteger(MediaFormat.KEY_BIT_RATE, 12_000_000)
                setInteger(MediaFormat.KEY_FRAME_RATE, fps)
                setInteger(MediaFormat.KEY_I_FRAME_INTERVAL, 1)
            }
            val codec = MediaCodec.createEncoderByType(MediaFormat.MIMETYPE_VIDEO_AVC)
            try {
                codec.configure(format, null, null, MediaCodec.CONFIGURE_FLAG_ENCODE)
                val surface = codec.createInputSurface()
                codec.start()
                return codec to surface
            } catch (e: Exception) {
                codec.release()
            }
        }
        throw IllegalStateException("no H.264 encoder for ${size.width}x${size.height}")
    }

    private fun describe(cameraId: String, chars: CameraCharacteristics, size: Size) {
        val level = when (chars.get(CameraCharacteristics.INFO_SUPPORTED_HARDWARE_LEVEL)) {
            CameraMetadata.INFO_SUPPORTED_HARDWARE_LEVEL_LEGACY -> "LEGACY"
            CameraMetadata.INFO_SUPPORTED_HARDWARE_LEVEL_LIMITED -> "LIMITED"
            CameraMetadata.INFO_SUPPORTED_HARDWARE_LEVEL_FULL -> "FULL"
            3 -> "LEVEL_3"
            4 -> "EXTERNAL"
            else -> "?"
        }
        append("Oculibre lab — 60 fps tests")
        append("${Build.MANUFACTURER} ${Build.MODEL} · Android ${Build.VERSION.RELEASE} (API ${Build.VERSION.SDK_INT}) · build ${Build.ID}")
        append("Camera $cameraId · ${size.width}x${size.height} · HW level $level")
        val ranges = chars.get(CameraCharacteristics.CONTROL_AE_AVAILABLE_TARGET_FPS_RANGES)
        append("Standard AE ranges: ${ranges?.joinToString(" ") ?: "?"}")
        if (Build.VERSION.SDK_INT >= 28) {
            val keys = chars.availableSessionKeys?.joinToString(", ") { it.name.substringAfterLast('.') }
            append("Session keys: ${keys ?: "none"}")
        }
        if (Build.VERSION.SDK_INT >= 29) describeVendorTags(chars)
    }

    @TargetApi(29)
    private fun describeVendorTags(chars: CameraCharacteristics) {
        fun read(name: String): IntArray? = runCatching {
            chars.get(CameraCharacteristics.Key(name, IntArray::class.java))
        }.getOrNull()

        read("samsung.android.control.effectAeAvailableTargetFpsRanges")?.let { v ->
            append("Samsung effect AE ranges: " + v.toList().chunked(2).joinToString(" ") { "[${it.joinToString(",")}]" })
        }
        read("org.quic.camera2.streamBasedFPS.info.StreamBasedFPSTable")?.let { v ->
            val fullHd = v.toList().chunked(4).filter { it.size == 4 && it[0] == 1920 && it[1] == 1080 }
            append("Qualcomm StreamBasedFPS 1080p: " + fullHd.joinToString(" ") { "[${it[2]},${it[3]}]" }.ifEmpty { "—" })
        }
        read("org.quic.camera2.customhfrfps.info.CustomHFRFpsTable")?.let { v ->
            val fullHd = v.toList().chunked(3).filter { it.size == 3 && it[0] == 1920 && it[1] == 1080 }
            append("Qualcomm CustomHFR 1080p: " + fullHd.joinToString(" ") { "${it[2]} fps" }.ifEmpty { "—" })
        }
    }
}
