package org.oculibre

import android.Manifest
import android.app.Activity
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.Typeface
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.text.InputType
import android.view.WindowManager
import android.widget.ArrayAdapter
import android.widget.Button
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.Spinner
import android.widget.TextView
import kotlin.concurrent.thread

/**
 * Phase-1 test screen: PC address, quality picker (only what this phone supports),
 * Start/Stop and a status line. Built in code on purpose (no AndroidX, no layout XML).
 */
class MainActivity : Activity() {

    private lateinit var hostInput: EditText
    private lateinit var qualitySpinner: Spinner
    private lateinit var button: Button
    private lateinit var status: TextView
    private lateinit var deviceInfo: TextView

    private var capabilities: DeviceCapabilities? = null
    private var supportedModes: List<ModeSupport> = emptyList()

    private var session: StreamSession? = null
    private val ui = Handler(Looper.getMainLooper())

    private val prefs by lazy { getSharedPreferences("oculibre", Context.MODE_PRIVATE) }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)

        val pad = (16 * resources.displayMetrics.density).toInt()

        hostInput = EditText(this).apply {
            setText(prefs.getString(PREF_HOST, DEFAULT_HOST))
            setSingleLine()
            inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_URI
        }
        qualitySpinner = Spinner(this)
        button = Button(this).apply {
            text = "Start"
            isEnabled = false // until the capabilities are known
            setOnClickListener { toggle() }
        }
        status = TextView(this).apply {
            text = "Checking what this phone supports…"
            typeface = Typeface.MONOSPACE
            setPadding(0, pad, 0, pad)
        }
        deviceInfo = TextView(this).apply {
            textSize = 12f
        }

        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            fitsSystemWindows = true
            setPadding(pad, pad, pad, pad)
            addView(TextView(context).apply {
                text = "PC address (127.0.0.1 over USB, or the PC's IP over Wi-Fi)"
            })
            addView(hostInput)
            addView(TextView(context).apply {
                text = "Quality"
                setPadding(0, pad / 2, 0, 0)
            })
            addView(qualitySpinner)
            addView(button)
            addView(status)
            addView(deviceInfo)
            addView(Button(context).apply {
                text = "Lab · 60 fps tests"
                setOnClickListener { startActivity(Intent(this@MainActivity, LabActivity::class.java)) }
            })
        }
        setContentView(root)

        probeCapabilities()
    }

    private fun probeCapabilities() {
        thread(name = "oculibre-probe") {
            val result = runCatching { Capabilities.probe(this) }
            ui.post {
                if (isFinishing) return@post
                result.onSuccess { showCapabilities(it) }
                    .onFailure { status.text = "Cannot read camera capabilities: ${it.message}" }
            }
        }
    }

    private fun showCapabilities(caps: DeviceCapabilities) {
        capabilities = caps
        supportedModes = caps.modes.filter { it.supported }

        val adapter = ArrayAdapter(this, android.R.layout.simple_spinner_item, supportedModes.map { it.label })
        adapter.setDropDownViewResource(android.R.layout.simple_spinner_dropdown_item)
        qualitySpinner.adapter = adapter

        val savedId = prefs.getString(PREF_MODE, Capabilities.DEFAULT_MODE_ID)
        val index = supportedModes.indexOfFirst { it.id == savedId }
            .takeIf { it >= 0 }
            ?: supportedModes.indexOfFirst { it.id == Capabilities.DEFAULT_MODE_ID }.coerceAtLeast(0)
        if (supportedModes.isNotEmpty()) qualitySpinner.setSelection(index)

        val unsupported = caps.modes.filter { !it.supported }
        deviceInfo.text = buildString {
            append("${Build.MANUFACTURER} ${Build.MODEL} · Android ${Build.VERSION.RELEASE} (API ${Build.VERSION.SDK_INT})\n")
            append("Camera ${caps.cameraId} · encoder ${caps.encoderName ?: "default"}\n")
            val unlisted = supportedModes.filter { it.unlisted }
            if (unlisted.isNotEmpty()) {
                append("\nNot officially listed (the real rate is checked while streaming):\n")
                unlisted.forEach { append("• ${it.label}\n") }
            }
            if (unsupported.isNotEmpty()) {
                append("\nNot available on this phone:\n")
                unsupported.forEach { append("• ${it.mode.label} — ${it.missing}\n") }
            }
        }

        if (supportedModes.isEmpty()) {
            status.text = "No supported quality found on this phone"
        } else {
            status.text = "Idle — port $PORT"
            button.isEnabled = true
        }
    }

    private fun toggle() {
        val current = session
        if (current == null) start() else current.stop()
    }

    private fun start() {
        if (Build.VERSION.SDK_INT >= 23 &&
            checkSelfPermission(Manifest.permission.CAMERA) != PackageManager.PERMISSION_GRANTED
        ) {
            requestPermissions(arrayOf(Manifest.permission.CAMERA), REQ_CAMERA)
            return
        }
        val caps = capabilities ?: return
        val support = supportedModes.getOrNull(qualitySpinner.selectedItemPosition) ?: return

        val host = hostInput.text.toString().trim().ifEmpty { DEFAULT_HOST }
        prefs.edit().putString(PREF_HOST, host).putString(PREF_MODE, support.id).apply()

        val newSession = StreamSession(this, host, PORT, StreamConfig.from(support, caps), listener)
        session = newSession
        setControlsEnabled(false)
        button.text = "Stop"
        status.text = "Connecting to $host:$PORT…"
        newSession.start()
    }

    private fun setControlsEnabled(enabled: Boolean) {
        hostInput.isEnabled = enabled
        qualitySpinner.isEnabled = enabled
    }

    private val listener = object : StreamSession.Listener {
        override fun onStatus(session: StreamSession, text: String) {
            ui.post { if (this@MainActivity.session === session) status.text = text }
        }

        override fun onStopped(session: StreamSession, error: String?) {
            ui.post {
                if (this@MainActivity.session !== session) return@post
                this@MainActivity.session = null
                button.text = "Start"
                setControlsEnabled(true)
                status.text = if (error == null) "Stopped" else "Stopped: $error"
            }
        }
    }

    override fun onRequestPermissionsResult(
        requestCode: Int,
        permissions: Array<out String>,
        grantResults: IntArray,
    ) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        if (requestCode != REQ_CAMERA) return
        if (grantResults.firstOrNull() == PackageManager.PERMISSION_GRANTED) {
            start()
        } else {
            status.text = "Camera permission denied"
        }
    }

    override fun onDestroy() {
        session?.stop()
        session = null
        super.onDestroy()
    }

    companion object {
        const val PORT = 47100
        const val DEFAULT_HOST = "127.0.0.1"
        private const val REQ_CAMERA = 1
        private const val PREF_HOST = "host"
        private const val PREF_MODE = "mode"
    }
}
