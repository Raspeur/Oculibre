package org.oculibre

import android.app.Activity
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.provider.Settings
import android.text.InputType
import android.view.View
import android.widget.AdapterView
import android.widget.ArrayAdapter
import android.widget.Button
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.Spinner
import android.widget.TextView
import kotlin.concurrent.thread

/**
 * Advanced screen (developers, and until the settings screen of the mockups exists): the
 * computer's address, the quality the phone starts with, the Wi-Fi pairing, battery shortcuts,
 * what the phone supports, the technical status line, the Lab.
 */
class AdvancedActivity : Activity() {

    private lateinit var hostInput: EditText
    private lateinit var qualitySpinner: Spinner
    private lateinit var status: TextView
    private lateinit var deviceInfo: TextView
    private lateinit var pairingInfo: TextView
    private lateinit var forgetPairing: Button
    private var supportedModes: List<ModeSupport> = emptyList()

    private val ui = Handler(Looper.getMainLooper())
    private val prefs by lazy { getSharedPreferences("oculibre", Context.MODE_PRIVATE) }
    private val observer: (StreamService.Snapshot) -> Unit = { state ->
        status.text = state.status ?: "Idle — port ${MainActivity.PORT}"
        hostInput.isEnabled = !state.running
        qualitySpinner.isEnabled = !state.running
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val pad = Ui.dp(this, 16f)

        hostInput = EditText(this).apply {
            setText(prefs.getString(MainActivity.PREF_HOST, MainActivity.DEFAULT_HOST))
            setSingleLine()
            inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_URI
        }
        qualitySpinner = Spinner(this)
        status = Ui.text(this, Ui.TEXT_SM, R.color.oc_text_muted).apply {
            typeface = android.graphics.Typeface.MONOSPACE
            setPadding(0, pad / 2, 0, pad / 2)
        }
        deviceInfo = Ui.text(this, Ui.TEXT_SM, R.color.oc_text_muted)
        pairingInfo = Ui.text(this, Ui.TEXT_MD, R.color.oc_text_2)
        forgetPairing = button("Forget this computer") {
            PairingStore.forget(this)
            updatePairingView()
        }

        val content = Ui.column(this, 8f).apply {
            setPadding(pad, pad, pad, pad)
            addView(Ui.row(this@AdvancedActivity, 12f).apply {
                addView(Ui.iconButton(this@AdvancedActivity, "‹", "Back") { finish() })
                addView(Ui.text(this@AdvancedActivity, Ui.TITLE_SM, display = true).apply { text = "Advanced" })
            })
            addView(label("Computer address (127.0.0.1 over USB; the cable is always tried first)"))
            addView(hostInput)
            addView(pairingInfo)
            addView(forgetPairing)
            addView(label("Quality when the phone starts (the computer can change it)"))
            addView(qualitySpinner)
            addView(button("Battery: let Oculibre run in the background") { requestBatteryExemption() })
            addView(button("Xiaomi: MIUI battery saver for Oculibre (\"No restrictions\")") { openMiuiBatterySettings() })
            addView(button("Lab · 60 fps tests") { startActivity(Intent(this@AdvancedActivity, LabActivity::class.java)) })
            addView(status)
            addView(deviceInfo)
        }
        setContentView(ScrollView(this).apply {
            fitsSystemWindows = true
            addView(content)
        })

        qualitySpinner.onItemSelectedListener = object : AdapterView.OnItemSelectedListener {
            override fun onItemSelected(parent: AdapterView<*>?, view: View?, position: Int, id: Long) {
                supportedModes.getOrNull(position)?.let { prefs.edit().putString(MainActivity.PREF_MODE, it.id).apply() }
            }
            override fun onNothingSelected(parent: AdapterView<*>?) {}
        }
        updatePairingView()
        probe()
    }

    override fun onResume() {
        super.onResume()
        StreamService.observe(observer)
        hostInput.setText(prefs.getString(MainActivity.PREF_HOST, MainActivity.DEFAULT_HOST))
    }

    override fun onPause() {
        StreamService.removeObserver(observer)
        if (!StreamService.current.running) {
            val host = hostInput.text.toString().trim().ifEmpty { MainActivity.DEFAULT_HOST }
            prefs.edit().putString(MainActivity.PREF_HOST, host).apply()
        }
        super.onPause()
    }

    private fun label(text: String) = Ui.text(this, Ui.TEXT_SM, R.color.oc_text_muted).apply {
        this.text = text
        setPadding(0, Ui.dp(context, 8f), 0, 0)
    }

    private fun button(text: String, onClick: () -> Unit) = Button(this).apply {
        this.text = text
        isAllCaps = false
        setOnClickListener { onClick() }
    }

    private fun updatePairingView() {
        val paired = PairingStore.get(this)
        pairingInfo.text = if (paired == null) {
            "Wi-Fi: not paired. On the computer: \"Add a phone\", then scan its QR code with the phone's camera app."
        } else {
            "Wi-Fi: paired with ${paired.name} (key ${PairingOffer.shortFingerprint(paired.fingerprint)}), encrypted"
        }
        forgetPairing.visibility = if (paired == null) View.GONE else View.VISIBLE
    }

    private fun probe() {
        thread(name = "oculibre-probe") {
            val result = runCatching { Capabilities.probe(this) }
            ui.post {
                if (isFinishing) return@post
                result.onSuccess { showCapabilities(it) }
                    .onFailure { deviceInfo.text = "Cannot read camera capabilities: ${it.message}" }
            }
        }
    }

    private fun showCapabilities(caps: DeviceCapabilities) {
        supportedModes = caps.main.modes.filter { it.supported }
        val adapter = ArrayAdapter(this, android.R.layout.simple_spinner_item, supportedModes.map { it.label })
        adapter.setDropDownViewResource(android.R.layout.simple_spinner_dropdown_item)
        qualitySpinner.adapter = adapter
        val savedId = prefs.getString(MainActivity.PREF_MODE, Capabilities.DEFAULT_MODE_ID)
        val index = supportedModes.indexOfFirst { it.id == savedId }.takeIf { it >= 0 }
            ?: supportedModes.indexOfFirst { it.id == Capabilities.DEFAULT_MODE_ID }.coerceAtLeast(0)
        if (supportedModes.isNotEmpty()) qualitySpinner.setSelection(index)

        val unsupported = caps.main.modes.filter { !it.supported }
        deviceInfo.text = buildString {
            append("${Build.MANUFACTURER} ${Build.MODEL} · Android ${Build.VERSION.RELEASE} (API ${Build.VERSION.SDK_INT})\n")
            append("Camera ${caps.main.id} (${caps.main.facing}) · encoder ${caps.encoderName ?: "default"}\n")
            if (caps.cameras.size > 1) {
                append("Other cameras: ")
                append(caps.cameras.drop(1).joinToString { "${it.id} (${it.facing}, ${it.equivalentFocalMm ?: "?"} mm)" })
                append("\n")
            }
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
    }

    private fun requestBatteryExemption() {
        if (Build.VERSION.SDK_INT < 23) return
        val popup = Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS, Uri.parse("package:$packageName"))
        try {
            startActivity(popup)
        } catch (e: Exception) {
            runCatching { startActivity(Intent(Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS)) }
        }
    }

    private fun openMiuiBatterySettings() {
        val miui = Intent("miui.intent.action.HIDDEN_APPS_CONFIG_ACTIVITY")
            .putExtra("package_name", packageName)
            .putExtra("package_label", getString(R.string.app_name))
        try {
            startActivity(miui)
        } catch (e: Exception) {
            runCatching {
                startActivity(Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.parse("package:$packageName")))
            }
        }
    }
}
