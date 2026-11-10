package org.oculibre

import android.Manifest
import android.app.Activity
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.PowerManager
import android.provider.Settings
import android.graphics.Typeface
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.text.InputType
import android.view.View
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
 * Start/Stop and a status line. Streaming itself runs in [StreamService], so it survives
 * the screen turning off. Built in code on purpose (no AndroidX, no layout XML).
 */
class MainActivity : Activity() {

    private lateinit var hostInput: EditText
    private lateinit var qualitySpinner: Spinner
    private lateinit var button: Button
    private lateinit var status: TextView
    private lateinit var deviceInfo: TextView
    private lateinit var batteryWarning: Button
    private lateinit var miuiSettings: Button

    private var capabilities: DeviceCapabilities? = null
    private var supportedModes: List<ModeSupport> = emptyList()

    private val ui = Handler(Looper.getMainLooper())
    private val observer: (StreamService.Snapshot) -> Unit = { render(it) }

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
        batteryWarning = Button(this).apply {
            text = "⚠ Battery optimisation is on: streaming may stop with the screen off. Tap to fix."
            isAllCaps = false
            setOnClickListener { requestBatteryExemption() }
        }
        miuiSettings = Button(this).apply {
            text = "Xiaomi: open MIUI battery saver for Oculibre (choose \"No restrictions\")"
            isAllCaps = false
            setOnClickListener { openMiuiBatterySettings() }
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
            addView(batteryWarning)
            addView(miuiSettings)
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
        } else if (!StreamService.current.running) {
            status.text = StreamService.current.status ?: "Idle — port $PORT"
        }
        render(StreamService.current)
    }

    override fun onResume() {
        super.onResume()
        StreamService.observe(observer)
        updateBatteryViews()
    }

    // ------------------------------------------------------------------ battery optimisation

    private fun isBatteryExempt(): Boolean {
        if (Build.VERSION.SDK_INT < 23) return true
        val power = getSystemService(Context.POWER_SERVICE) as PowerManager
        return power.isIgnoringBatteryOptimizations(packageName)
    }

    private fun isXiaomi(): Boolean =
        Build.MANUFACTURER.equals("xiaomi", ignoreCase = true) ||
            Build.BRAND.equals("redmi", ignoreCase = true) || Build.BRAND.equals("poco", ignoreCase = true)

    private fun updateBatteryViews() {
        batteryWarning.visibility = if (isBatteryExempt()) View.GONE else View.VISIBLE
        miuiSettings.visibility = if (isXiaomi()) View.VISIBLE else View.GONE
    }

    /** Android's own popup: "Let Oculibre always run in the background?" */
    private fun requestBatteryExemption() {
        if (Build.VERSION.SDK_INT < 23 || isBatteryExempt()) return
        val popup = Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS, Uri.parse("package:$packageName"))
        val fallback = Intent(Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS)
        try {
            startActivity(popup)
        } catch (e: Exception) {
            runCatching { startActivity(fallback) }
        }
    }

    /** MIUI has its own battery saver on top of Android's; open the per-app page if we can. */
    private fun openMiuiBatterySettings() {
        val miui = Intent("miui.intent.action.HIDDEN_APPS_CONFIG_ACTIVITY")
            .putExtra("package_name", packageName)
            .putExtra("package_label", getString(R.string.app_name))
        val appDetails = Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.parse("package:$packageName"))
        try {
            startActivity(miui)
        } catch (e: Exception) {
            runCatching { startActivity(appDetails) }
        }
    }

    override fun onPause() {
        StreamService.removeObserver(observer)
        super.onPause()
    }

    /** Reflects the streaming service state (it keeps running when this screen is gone). */
    private fun render(state: StreamService.Snapshot) {
        button.text = if (state.running) "Stop" else "Start"
        button.isEnabled = state.running || supportedModes.isNotEmpty()
        setControlsEnabled(!state.running)
        if (state.status != null && (state.running || capabilities != null)) status.text = state.status
    }

    private fun toggle() {
        if (StreamService.current.running) StreamService.stop(this) else start()
    }

    private fun start() {
        if (Build.VERSION.SDK_INT >= 23) {
            val missing = mutableListOf<String>()
            if (checkSelfPermission(Manifest.permission.CAMERA) != PackageManager.PERMISSION_GRANTED) {
                missing += Manifest.permission.CAMERA
            }
            // Android 13+: needed to show the "streaming" notification (optional, asked once).
            if (Build.VERSION.SDK_INT >= 33 && !prefs.getBoolean(PREF_ASKED_NOTIFICATIONS, false) &&
                checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
            ) {
                missing += Manifest.permission.POST_NOTIFICATIONS
                prefs.edit().putBoolean(PREF_ASKED_NOTIFICATIONS, true).apply()
            }
            if (missing.isNotEmpty()) {
                requestPermissions(missing.toTypedArray(), REQ_CAMERA)
                return
            }
        }
        val caps = capabilities ?: return
        val support = supportedModes.getOrNull(qualitySpinner.selectedItemPosition) ?: return

        val host = hostInput.text.toString().trim().ifEmpty { DEFAULT_HOST }
        prefs.edit().putString(PREF_HOST, host).putString(PREF_MODE, support.id).apply()

        StreamService.start(this, host, PORT, StreamConfig.from(support, caps))

        // First start only: ask for the battery exemption right away (the warning button stays
        // visible afterwards until it is granted).
        if (!isBatteryExempt() && !prefs.getBoolean(PREF_ASKED_BATTERY, false)) {
            prefs.edit().putBoolean(PREF_ASKED_BATTERY, true).apply()
            requestBatteryExemption()
        }
    }

    private fun setControlsEnabled(enabled: Boolean) {
        hostInput.isEnabled = enabled
        qualitySpinner.isEnabled = enabled
    }


    override fun onRequestPermissionsResult(
        requestCode: Int,
        permissions: Array<out String>,
        grantResults: IntArray,
    ) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        if (requestCode != REQ_CAMERA) return
        if (Build.VERSION.SDK_INT < 23 ||
            checkSelfPermission(Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED
        ) {
            start()
        } else {
            status.text = "Camera permission denied"
        }
    }

    companion object {
        const val PORT = 47100
        const val DEFAULT_HOST = "127.0.0.1"
        private const val REQ_CAMERA = 1
        private const val PREF_HOST = "host"
        private const val PREF_MODE = "mode"
        private const val PREF_ASKED_NOTIFICATIONS = "asked_notifications"
        private const val PREF_ASKED_BATTERY = "asked_battery"
    }
}
