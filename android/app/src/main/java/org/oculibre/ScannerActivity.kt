package org.oculibre

import android.Manifest
import android.annotation.SuppressLint
import android.app.Activity
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.Canvas
import android.graphics.ImageFormat
import android.graphics.Matrix
import android.graphics.Paint
import android.graphics.Path
import android.graphics.RectF
import android.graphics.SurfaceTexture
import android.hardware.camera2.CameraAccessException
import android.hardware.camera2.CameraCaptureSession
import android.hardware.camera2.CameraCharacteristics
import android.hardware.camera2.CameraDevice
import android.hardware.camera2.CameraManager
import android.hardware.camera2.CameraMetadata
import android.hardware.camera2.CaptureRequest
import android.media.ImageReader
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.HandlerThread
import android.os.Looper
import android.os.SystemClock
import android.provider.Settings
import android.util.Size
import android.view.Gravity
import android.view.Surface
import android.view.TextureView
import android.view.View
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.TextView
import kotlin.math.abs
import kotlin.math.max
import kotlin.math.min

/**
 * Scans the computer's pairing QR code with the phone's camera (no camera app, no Google
 * service): Camera2 shows the back camera on a TextureView and sends small YUV pictures to an
 * ImageReader; the luminance of each goes to [QrDecoder] (vendored ZXing) on a background thread.
 * A valid `oculibre://pair` code opens the usual confirmation ([PairingPrompt]); once paired the
 * screen closes with RESULT_OK. The camera is released while the dialog shows, and whenever the
 * screen is not in front.
 */
class ScannerActivity : Activity() {

    override fun attachBaseContext(base: Context) = super.attachBaseContext(Lang.wrap(base))

    private lateinit var preview: TextureView
    private lateinit var hint: TextView
    private lateinit var where: TextView
    private lateinit var blocked: LinearLayout
    private lateinit var blockedText: TextView
    private lateinit var blockedAction: TextView

    private val ui = Handler(Looper.getMainLooper())
    private val prefs by lazy { getSharedPreferences("oculibre", Context.MODE_PRIVATE) }

    // Camera state: touched on the background thread, started and stopped from the UI thread.
    private var worker: HandlerThread? = null
    private var background: Handler? = null
    private var camera: CameraDevice? = null
    private var session: CameraCaptureSession? = null
    private var reader: ImageReader? = null
    private var previewSize: Size? = null
    private var sensorOrientation = 90

    /** The camera should be open (between start and stop): late callbacks close what they get. */
    @Volatile private var active = false
    /** Pictures are decoded (not while the confirmation shows). */
    @Volatile private var scanning = false
    private var prompting = false
    private var askedPermission = false

    // Background thread only.
    private val decoder = QrDecoder()
    private var luminance = ByteArray(0)
    private var lastDecode = 0L

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        askedPermission = savedInstanceState?.getBoolean(STATE_ASKED) ?: false
        setContentView(buildScreen())
    }

    override fun onSaveInstanceState(outState: Bundle) {
        super.onSaveInstanceState(outState)
        outState.putBoolean(STATE_ASKED, askedPermission)
    }

    override fun onResume() {
        super.onResume()
        if (!prompting) start()
    }

    override fun onPause() {
        stop()
        super.onPause()
    }

    // ------------------------------------------------------------------ layout

    private fun buildScreen(): View {
        val c = this
        val white = 0xFFFFFFFF.toInt()
        preview = TextureView(c).apply {
            surfaceTextureListener = object : TextureView.SurfaceTextureListener {
                override fun onSurfaceTextureAvailable(st: SurfaceTexture, w: Int, h: Int) { if (active) openCamera() }
                override fun onSurfaceTextureSizeChanged(st: SurfaceTexture, w: Int, h: Int) = fitPreview()
                override fun onSurfaceTextureDestroyed(st: SurfaceTexture) = true
                override fun onSurfaceTextureUpdated(st: SurfaceTexture) {}
            }
            addOnLayoutChangeListener { _, _, _, _, _, _, _, _, _ -> fitPreview() }
        }
        val top = Ui.row(c, 12f).apply {
            val p = Ui.dp(c, 16f)
            setPadding(p, Ui.dp(c, 10f), p, Ui.dp(c, 10f))
            addView(Ui.iconButton(c, "‹", getString(R.string.cancel)) { finish() })
            addView(Ui.text(c, Ui.TITLE_SM, display = true).apply {
                text = getString(R.string.scan_title)
                setTextColor(white)
            })
        }
        hint = Ui.text(c, Ui.TEXT_LG, strong = true).apply {
            gravity = Gravity.CENTER
            setTextColor(white)
            setShadowLayer(6f, 0f, 1f, 0xCC000000.toInt())
        }
        where = Ui.text(c, Ui.TEXT_MD).apply {
            gravity = Gravity.CENTER
            text = getString(R.string.scan_where)
            setTextColor(0xCCFFFFFF.toInt())
            setShadowLayer(6f, 0f, 1f, 0xCC000000.toInt())
        }
        val bottom = Ui.column(c, 6f).apply {
            val p = Ui.dp(c, 24f)
            setPadding(p, p, p, Ui.dp(c, 32f))
            addView(hint)
            addView(where)
        }
        blockedText = Ui.text(c, Ui.TEXT_LG, R.color.oc_text).apply {
            gravity = Gravity.CENTER
            setLineSpacing(0f, 1.35f)
        }
        blockedAction = Ui.primaryButton(c, "") {}
        blocked = Ui.column(c, 20f).apply {
            gravity = Gravity.CENTER
            val p = Ui.dp(c, 32f)
            setPadding(p, p, p, p)
            setBackgroundColor(Ui.color(c, R.color.oc_bg))
            visibility = View.GONE
            addView(blockedText)
            addView(blockedAction, LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT))
        }
        val match = FrameLayout.LayoutParams.MATCH_PARENT
        val wrap = FrameLayout.LayoutParams.WRAP_CONTENT
        val stage = FrameLayout(c).apply {
            setBackgroundColor(0xFF000000.toInt())
            addView(preview, FrameLayout.LayoutParams(match, match))
            addView(Viewfinder(c), FrameLayout.LayoutParams(match, match))
            addView(bottom, FrameLayout.LayoutParams(match, wrap, Gravity.BOTTOM))
            addView(blocked, FrameLayout.LayoutParams(match, match))
            addView(top, FrameLayout.LayoutParams(match, wrap, Gravity.TOP))
        }
        return FrameLayout(c).apply {
            setBackgroundColor(0xFF000000.toInt())
            fitsSystemWindows = true
            addView(stage)
        }
    }

    private fun showScanning() {
        blocked.visibility = View.GONE
        hint.text = getString(R.string.scan_hint)
    }

    /** Instead of the camera: why it cannot scan, and the one thing to do about it. */
    private fun showBlocked(message: Int, action: Int, onAction: () -> Unit) {
        blockedText.text = getString(message)
        blockedAction.text = getString(action)
        blockedAction.setOnClickListener { onAction() }
        blocked.visibility = View.VISIBLE
    }

    private val restoreHint = Runnable { if (scanning) hint.text = getString(R.string.scan_hint) }

    // ------------------------------------------------------------------ camera

    private fun start() {
        if (active) return
        if (StreamService.current.running) {
            // One app at a time on the camera: the stream has it.
            showBlocked(R.string.scan_busy, R.string.retry) { start() }
            return
        }
        if (!hasCameraPermission()) {
            if (!askedPermission && Build.VERSION.SDK_INT >= 23) {
                askedPermission = true
                requestPermissions(arrayOf(Manifest.permission.CAMERA), REQ_CAMERA)
            } else {
                // Refused: Android won't ask again from here, the app's settings page can.
                showBlocked(R.string.scan_camera_denied, R.string.scan_allow) {
                    runCatching {
                        startActivity(Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.parse("package:$packageName")))
                    }
                }
            }
            return
        }
        showScanning()
        active = true
        worker = HandlerThread("oculibre-scan").also { it.start() }
        background = Handler(worker!!.looper)
        if (preview.isAvailable) openCamera() // else when the surface is ready
    }

    override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<out String>, grantResults: IntArray) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        if (requestCode == REQ_CAMERA && hasCameraPermission()) {
            prefs.edit().putBoolean(MainActivity.PREF_CAMERA_GRANTED_ONCE, true).apply()
        }
        // onResume follows and calls start() again: the camera, or the "allow in settings" screen.
    }

    private fun stop() {
        active = false
        scanning = false
        ui.removeCallbacks(restoreHint)
        val handler = background ?: return
        val s = session
        val d = camera
        val r = reader
        session = null
        camera = null
        reader = null
        background = null
        // Closed on the camera thread, after a picture being decoded is done with.
        handler.post {
            runCatching { s?.close() }
            runCatching { d?.close() }
            runCatching { r?.close() }
        }
        worker?.quitSafely()
        worker = null
    }

    private fun openCamera() {
        val handler = background ?: return
        if (camera != null || reader != null) return
        val manager = getSystemService(Context.CAMERA_SERVICE) as CameraManager
        try {
            val ids = manager.cameraIdList
            val id = ids.firstOrNull {
                manager.getCameraCharacteristics(it).get(CameraCharacteristics.LENS_FACING) == CameraMetadata.LENS_FACING_BACK
            } ?: ids.firstOrNull() ?: return cameraFailed()
            val chars = manager.getCameraCharacteristics(id)
            sensorOrientation = chars.get(CameraCharacteristics.SENSOR_ORIENTATION) ?: 90
            val map = chars.get(CameraCharacteristics.SCALER_STREAM_CONFIGURATION_MAP) ?: return cameraFailed()
            val analysis = analysisSize(map.getOutputSizes(ImageFormat.YUV_420_888))
            previewSize = previewSize(map.getOutputSizes(SurfaceTexture::class.java), analysis)
            fitPreview()
            reader = ImageReader.newInstance(analysis.width, analysis.height, ImageFormat.YUV_420_888, 2).apply {
                setOnImageAvailableListener({ onPicture(it) }, handler)
            }
            val autofocus = chars.get(CameraCharacteristics.CONTROL_AF_AVAILABLE_MODES)?.let { modes ->
                when {
                    CameraMetadata.CONTROL_AF_MODE_CONTINUOUS_PICTURE in modes -> CameraMetadata.CONTROL_AF_MODE_CONTINUOUS_PICTURE
                    CameraMetadata.CONTROL_AF_MODE_CONTINUOUS_VIDEO in modes -> CameraMetadata.CONTROL_AF_MODE_CONTINUOUS_VIDEO
                    else -> null
                }
            }
            manager.openCamera(id, object : CameraDevice.StateCallback() {
                override fun onOpened(device: CameraDevice) {
                    if (!active) return device.close()
                    camera = device
                    startSession(device, autofocus)
                }
                override fun onDisconnected(device: CameraDevice) {
                    device.close()
                    if (active) cameraFailed()
                }
                override fun onError(device: CameraDevice, error: Int) {
                    device.close()
                    if (active) cameraFailed()
                }
            }, handler)
        } catch (e: CameraAccessException) {
            cameraFailed()
        } catch (e: SecurityException) {
            cameraFailed()
        } catch (e: IllegalArgumentException) {
            cameraFailed()
        }
    }

    @Suppress("DEPRECATION") // createCaptureSession(List<Surface>, …): the only form on API 21–27
    private fun startSession(device: CameraDevice, autofocus: Int?) {
        val texture = preview.surfaceTexture ?: return cameraFailed()
        val size = previewSize ?: return cameraFailed()
        val imageSurface = reader?.surface ?: return
        texture.setDefaultBufferSize(size.width, size.height)
        val previewSurface = Surface(texture)
        try {
            device.createCaptureSession(listOf(previewSurface, imageSurface), object : CameraCaptureSession.StateCallback() {
                override fun onConfigured(s: CameraCaptureSession) {
                    if (!active) return s.close()
                    session = s
                    try {
                        val request = device.createCaptureRequest(CameraDevice.TEMPLATE_PREVIEW).apply {
                            addTarget(previewSurface)
                            addTarget(imageSurface)
                            set(CaptureRequest.CONTROL_MODE, CameraMetadata.CONTROL_MODE_AUTO)
                            autofocus?.let { set(CaptureRequest.CONTROL_AF_MODE, it) }
                        }
                        s.setRepeatingRequest(request.build(), null, background)
                        scanning = true
                    } catch (e: Exception) { // CameraAccessException, IllegalStateException (closed)
                        if (active) cameraFailed()
                    }
                }
                override fun onConfigureFailed(s: CameraCaptureSession) {
                    if (active) cameraFailed()
                }
            }, background)
        } catch (e: Exception) {
            if (active) cameraFailed()
        }
    }

    private fun cameraFailed() {
        ui.post {
            if (isFinishing) return@post
            stop()
            showBlocked(R.string.scan_camera_error, R.string.retry) { start() }
        }
    }

    // ------------------------------------------------------------------ decoding

    /** Camera thread. The newest picture only: older ones are dropped while one is decoded. */
    private fun onPicture(source: ImageReader) {
        val image = try {
            source.acquireLatestImage()
        } catch (e: IllegalStateException) {
            null
        } ?: return
        var text: String? = null
        try {
            val now = SystemClock.elapsedRealtime()
            if (!scanning || now - lastDecode < MIN_DECODE_INTERVAL_MS) return
            lastDecode = now
            // The Y plane: brightness, one byte per pixel (pixel stride 1), rows maybe padded.
            val plane = image.planes[0]
            val stride = plane.rowStride
            val buffer = plane.buffer
            if (luminance.size < stride * image.height) luminance = ByteArray(stride * image.height)
            buffer.rewind()
            buffer.get(luminance, 0, min(buffer.remaining(), luminance.size))
            text = decoder.decode(luminance, image.width, image.height, stride)
        } catch (e: IllegalStateException) {
            return // closed under us (stop)
        } finally {
            image.close()
        }
        text?.let { found -> ui.post { onCode(found) } }
    }

    /** UI thread: a QR code was read. */
    private fun onCode(text: String) {
        if (!scanning || prompting) return
        val offer = PairingOffer.parse(text)
        if (offer == null) {
            // Another QR code (a web site, a Wi-Fi code…), or a pairing code of a newer version.
            val newer = text.startsWith("oculibre://pair", ignoreCase = true)
            hint.text = getString(if (newer) R.string.pair_invalid else R.string.scan_not_oculibre)
            ui.removeCallbacks(restoreHint)
            ui.postDelayed(restoreHint, 2500)
            return
        }
        prompting = true
        stop() // the camera is not needed while the user checks the code
        hint.text = getString(R.string.scan_found)
        PairingPrompt.confirm(this, offer) { paired ->
            prompting = false
            if (paired) {
                setResult(RESULT_OK)
                finish()
            } else if (!isFinishing) {
                start() // cancelled: maybe the wrong computer, scan again (Back leaves)
            }
        }
    }

    // ------------------------------------------------------------------ sizes

    /** Enough pixels for a QR code across a third of the picture, little enough to decode fast. */
    private fun analysisSize(sizes: Array<Size>?): Size {
        val all = sizes?.toList().orEmpty()
        return all.filter { it.width <= 1280 && it.height <= 960 && it.width * it.height >= 640 * 480 }
            .maxByOrNull { it.width * it.height }
            ?: all.minByOrNull { abs(it.width * it.height - 640 * 480) }
            ?: Size(640, 480)
    }

    /** The preview: same shape as the analysed picture (what is aimed at is what is read). */
    private fun previewSize(sizes: Array<Size>?, analysis: Size): Size {
        val ratio = analysis.width.toFloat() / analysis.height
        return sizes.orEmpty()
            .filter { abs(it.width.toFloat() / it.height - ratio) < 0.02f && it.width <= 1920 && it.height <= 1440 }
            .maxByOrNull { it.width * it.height }
            ?: analysis
    }

    /**
     * Fills the TextureView with the camera picture, upright, cropped rather than stretched. The
     * screen is portrait (manifest); the sensor is usually mounted at 90°, so the picture shows
     * with its width and height swapped.
     */
    private fun fitPreview() {
        val size = previewSize ?: return
        val vw = preview.width.toFloat()
        val vh = preview.height.toFloat()
        if (vw == 0f || vh == 0f) return
        @Suppress("DEPRECATION")
        val rotation = windowManager.defaultDisplay.rotation
        val matrix = Matrix()
        val cx = vw / 2
        val cy = vh / 2
        if (rotation == Surface.ROTATION_90 || rotation == Surface.ROTATION_270) {
            // Large screens may ignore the portrait lock: turn the picture with the screen.
            val view = RectF(0f, 0f, vw, vh)
            val buffer = RectF(0f, 0f, size.height.toFloat(), size.width.toFloat())
            buffer.offset(cx - buffer.centerX(), cy - buffer.centerY())
            matrix.setRectToRect(view, buffer, Matrix.ScaleToFit.FILL)
            val scale = max(vh / size.height, vw / size.width)
            matrix.postScale(scale, scale, cx, cy)
            matrix.postRotate(90f * (rotation - 2), cx, cy)
        } else {
            val swapped = sensorOrientation % 180 != 0
            val cw = (if (swapped) size.height else size.width).toFloat()
            val ch = (if (swapped) size.width else size.height).toFloat()
            val scale = max(vw / cw, vh / ch)
            matrix.setScale(cw * scale / vw, ch * scale / vh, cx, cy)
            if (rotation == Surface.ROTATION_180) matrix.postRotate(180f, cx, cy)
        }
        preview.setTransform(matrix)
    }

    private fun hasCameraPermission(): Boolean =
        Build.VERSION.SDK_INT < 23 ||
            checkSelfPermission(Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED

    /** Darkens around the aiming square and draws its orange corners. */
    @SuppressLint("ViewConstructor")
    private class Viewfinder(context: Context) : View(context) {
        private val shade = Paint().apply { color = 0x88000000.toInt() }
        private val corner = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            color = Ui.color(context, R.color.oc_accent)
            style = Paint.Style.STROKE
            strokeWidth = Ui.dp(context, 4f).toFloat()
            strokeCap = Paint.Cap.ROUND
        }
        private val arm = Ui.dp(context, 28f).toFloat()
        private val path = Path()

        override fun onDraw(canvas: Canvas) {
            val w = width.toFloat()
            val h = height.toFloat()
            val side = min(w, h) * 0.72f
            val l = (w - side) / 2
            val t = (h - side) / 2 - h * 0.04f
            val r = l + side
            val b = t + side
            canvas.drawRect(0f, 0f, w, t, shade)
            canvas.drawRect(0f, b, w, h, shade)
            canvas.drawRect(0f, t, l, b, shade)
            canvas.drawRect(r, t, w, b, shade)
            path.reset()
            path.moveTo(l, t + arm); path.lineTo(l, t); path.lineTo(l + arm, t)
            path.moveTo(r - arm, t); path.lineTo(r, t); path.lineTo(r, t + arm)
            path.moveTo(r, b - arm); path.lineTo(r, b); path.lineTo(r - arm, b)
            path.moveTo(l + arm, b); path.lineTo(l, b); path.lineTo(l, b - arm)
            canvas.drawPath(path, corner)
        }
    }

    companion object {
        private const val REQ_CAMERA = 4
        private const val STATE_ASKED = "asked_camera"
        /** At most ~8 decodes a second: enough to feel instant, light on old phones. */
        private const val MIN_DECODE_INTERVAL_MS = 120L
    }
}
