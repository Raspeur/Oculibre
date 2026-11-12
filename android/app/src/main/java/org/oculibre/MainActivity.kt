package org.oculibre

import android.Manifest
import android.app.Activity
import android.app.AlertDialog
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.os.PowerManager
import android.provider.Settings
import android.view.Gravity
import android.view.View
import android.view.WindowManager
import android.widget.CheckBox
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import kotlin.concurrent.thread

/**
 * The everyday screen (design/mockups/PhoneHome): the computer, a big Start/Stop button, what
 * happens in plain words, a warning when Android withdrew something Oculibre needs. The camera
 * and the quality are chosen from the computer; the technical settings are in [AdvancedActivity].
 * Streaming itself runs in [StreamService], so it survives the screen turning off.
 * Built in code on purpose (no AndroidX, no layout XML); the look comes from [Ui].
 */
class MainActivity : Activity() {

    override fun attachBaseContext(base: Context) = super.attachBaseContext(Lang.wrap(base))

    private lateinit var alert: LinearLayout
    private lateinit var alertTitle: TextView
    private lateinit var alertText: TextView
    private lateinit var computerName: TextView
    private lateinit var computerDetail: TextView
    private lateinit var bigButton: LinearLayout
    private lateinit var bigSymbol: TextView
    private lateinit var bigLabel: TextView
    private lateinit var stateTitle: TextView
    private lateinit var stateDetail: TextView

    private var capabilities: DeviceCapabilities? = null
    /**
     * The cameras were read with the camera permission. Without it Android hides some keys,
     * among them the vendor tables of the hidden 60 fps: read again once it is granted.
     */
    private var probedWithCamera = false
    private var supportedModes: List<ModeSupport> = emptyList()
    /** What the warning banner fixes, if shown. */
    private var fixAlert: (() -> Unit)? = null

    private val ui = Handler(Looper.getMainLooper())
    private val observer: (StreamService.Snapshot) -> Unit = { render(it) }
    private val prefs by lazy { getSharedPreferences("oculibre", Context.MODE_PRIVATE) }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(buildScreen())
        probeCapabilities()
        // A screen created again (configuration change, process restarted) gets its first intent
        // back, pairing link included: it was handled already.
        val asking = savedInstanceState == null && handlePairingLink(intent)
        // With a pairing dialog open, the first-launch screens wait for it (see its dismiss).
        if (!asking) maybeWelcome()
    }

    // ------------------------------------------------------------------ layout

    private fun buildScreen(): View {
        val c = this
        val header = Ui.row(c, 10f).apply {
            addView(Ui.logo(c, 36f))
            addView(Ui.grow(Ui.text(c, 19f, display = true).apply { text = getString(R.string.app_name) }))
            addView(Ui.iconButton(c, "☰", getString(R.string.menu)) {
                startActivity(Intent(c, SettingsActivity::class.java))
            })
        }

        alertTitle = Ui.text(c, Ui.TEXT_BASE, strong = true)
        alertText = Ui.text(c, Ui.TEXT_MD, R.color.oc_text_2)
        alert = Ui.row(c, 12f).apply {
            background = Ui.rounded(c, R.color.oc_accent_soft, Ui.RADIUS_MD, R.color.oc_accent_line)
            val p = Ui.dp(c, 12f)
            setPadding(p, p, p, p)
            visibility = View.GONE
            isClickable = true
            setOnClickListener { fixAlert?.invoke() }
            addView(Ui.badge(c, "!", R.color.oc_accent_soft, R.color.oc_accent_text, 28f))
            addView(Ui.grow(Ui.column(c, 2f).apply {
                addView(alertTitle)
                addView(alertText)
            }))
            addView(Ui.text(c, Ui.TEXT_MD, R.color.oc_accent_text, strong = true).apply { text = getString(R.string.fix) })
        }

        computerName = Ui.text(c, Ui.TEXT_LG, strong = true)
        computerDetail = Ui.text(c, Ui.TEXT_MD, R.color.oc_text_muted)
        val computer = Ui.card(c, 14f).apply {
            addView(Ui.badge(c, "PC", R.color.oc_surface_4, R.color.oc_text_2, 44f).apply { textSize = Ui.TEXT_MD })
            addView(Ui.grow(Ui.column(c, 3f).apply {
                addView(computerName)
                addView(computerDetail)
            }))
        }

        bigSymbol = Ui.text(c, 34f).apply { gravity = Gravity.CENTER }
        bigLabel = Ui.text(c, Ui.TITLE_SM, display = true).apply { gravity = Gravity.CENTER }
        bigButton = Ui.column(c, 6f).apply {
            gravity = Gravity.CENTER
            isClickable = true
            isFocusable = true
            layoutParams = LinearLayout.LayoutParams(Ui.dp(c, 196f), Ui.dp(c, 196f))
            setOnClickListener { toggle() }
            addView(bigSymbol)
            addView(bigLabel)
        }
        stateTitle = Ui.text(c, Ui.TITLE_LG, display = true).apply { gravity = Gravity.CENTER }
        stateDetail = Ui.text(c, Ui.TEXT_BASE, R.color.oc_text_muted).apply {
            gravity = Gravity.CENTER
            val p = Ui.dp(c, 12f)
            setPadding(p, 0, p, 0)
        }
        val center = Ui.column(c, 18f).apply {
            gravity = Gravity.CENTER
            layoutParams = LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, 0, 1f)
            addView(bigButton)
            addView(stateTitle)
            addView(stateDetail)
        }

        val screenOffBox = CheckBox(c).apply {
            isChecked = prefs.getBoolean(PREF_SCREEN_OFF, true)
            setOnCheckedChangeListener { _, checked ->
                prefs.edit().putBoolean(PREF_SCREEN_OFF, checked).apply()
                applyScreen(StreamService.current)
            }
        }
        val screenOff = Ui.card(c, 12f).apply {
            val p = Ui.dp(c, 14f)
            setPadding(Ui.dp(c, 16f), p, Ui.dp(c, 8f), p)
            isClickable = true
            setOnClickListener { screenOffBox.toggle() }
            addView(Ui.grow(Ui.column(c, 3f).apply {
                addView(Ui.text(c, Ui.TEXT_LG, strong = true).apply { text = getString(R.string.screen_off_title) })
                addView(Ui.text(c, Ui.TEXT_MD, R.color.oc_text_muted).apply { text = getString(R.string.screen_off_hint) })
            }))
            addView(screenOffBox)
        }

        val footer = Ui.text(c, Ui.TEXT_MD, R.color.oc_text_muted).apply {
            text = getString(R.string.footer)
            gravity = Gravity.CENTER
        }

        // The column keeps its own margins; the frame around it takes the system bars (with
        // fitsSystemWindows, a view's padding is replaced by the bars' size).
        val column = Ui.column(c, 18f).apply {
            val p = Ui.dp(c, 20f)
            setPadding(p, Ui.dp(c, 24f), p, Ui.dp(c, 24f))
            addView(header)
            addView(alert)
            addView(computer)
            addView(center)
            addView(screenOff)
            addView(footer)
        }
        // Scrolls on small screens only: fillViewport lets the button's space grow on tall ones.
        val scroll = ScrollView(c).apply {
            isFillViewport = true
            addView(column, FrameLayout.LayoutParams(FrameLayout.LayoutParams.MATCH_PARENT, FrameLayout.LayoutParams.MATCH_PARENT))
        }
        return FrameLayout(c).apply {
            fitsSystemWindows = true
            addView(scroll, FrameLayout.LayoutParams(FrameLayout.LayoutParams.MATCH_PARENT, FrameLayout.LayoutParams.MATCH_PARENT))
        }
    }

    /**
     * While live with "turn the screen off" checked: lowest brightness at once, and the screen is
     * no longer kept on, so it goes off after the phone's usual delay (the service keeps
     * streaming). Unchecked, or not live: the screen stays on with its normal brightness while the
     * app shows something happening.
     */
    private fun applyScreen(state: StreamService.Snapshot) {
        val live = state.phase == StreamService.Phase.LIVE
        val saving = live && prefs.getBoolean(PREF_SCREEN_OFF, true)
        if (state.running && !saving) {
            window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        } else {
            window.clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        }
        window.attributes = window.attributes.apply {
            screenBrightness = if (saving) 0.01f else WindowManager.LayoutParams.BRIGHTNESS_OVERRIDE_NONE
        }
    }

    /** Reflects the streaming service (it keeps running when this screen is gone). */
    private fun render(state: StreamService.Snapshot) {
        val running = state.running
        val live = state.phase == StreamService.Phase.LIVE
        bigLabel.text = getString(if (running) R.string.stop else R.string.start)
        bigSymbol.text = if (running) "■" else "▶"
        val (fill, ring, ink) = when {
            live -> Triple(R.color.oc_bg, R.color.oc_live, R.color.oc_live)
            running -> Triple(R.color.oc_bg, R.color.oc_accent_text, R.color.oc_accent_text)
            else -> Triple(R.color.oc_accent, R.color.oc_accent, R.color.oc_on_accent)
        }
        bigButton.background = Ui.oval(this, fill, ring, 6f)
        bigSymbol.setTextColor(Ui.color(this, ink))
        bigLabel.setTextColor(Ui.color(this, ink))
        val canStart = running || supportedModes.isNotEmpty()
        bigButton.isEnabled = canStart
        bigButton.alpha = if (canStart) 1f else 0.4f

        stateTitle.setTextColor(Ui.color(this, if (live) R.color.oc_live else R.color.oc_text))
        when {
            capabilities == null -> {
                stateTitle.text = getString(R.string.state_checking)
                stateDetail.text = ""
            }
            supportedModes.isEmpty() -> {
                stateTitle.text = getString(R.string.state_ready)
                stateDetail.text = getString(R.string.no_quality)
            }
            live -> {
                stateTitle.text = getString(R.string.state_live)
                stateDetail.text = state.live ?: ""
            }
            state.phase == StreamService.Phase.SEARCHING -> {
                stateTitle.text = getString(R.string.state_searching)
                stateDetail.text = state.status ?: ""
            }
            running -> {
                stateTitle.text = getString(R.string.state_connecting)
                stateDetail.text = state.status ?: ""
            }
            else -> {
                stateTitle.text = getString(R.string.state_ready)
                // After a stop with an error, say why; else what to do.
                val status = state.status
                stateDetail.text = if (status != null && status.startsWith("Stopped:")) status else getString(R.string.ready_detail)
            }
        }
        renderComputer()
        applyScreen(state)
    }

    private fun renderComputer() {
        val paired = PairingStore.get(this)
        if (paired != null) {
            computerName.text = paired.name
            computerDetail.text = getString(R.string.wifi_encrypted)
            // The green padlock of the mockup: the Wi-Fi link is encrypted.
            computerDetail.setCompoundDrawablesRelativeWithIntrinsicBounds(R.drawable.ic_lock, 0, 0, 0)
            computerDetail.compoundDrawablePadding = Ui.dp(this, 6f)
        } else {
            computerName.text = getString(R.string.usb_cable)
            computerDetail.text = getString(R.string.usb_detail)
            computerDetail.setCompoundDrawablesRelativeWithIntrinsicBounds(0, 0, 0, 0)
        }
    }

    /** The warning banner: camera access withdrawn first, then battery saving. */
    private fun renderAlert() {
        val (title, text, fix) = when {
            !hasCameraPermission() && prefs.getBoolean(PREF_CAMERA_GRANTED_ONCE, false) -> Triple(
                R.string.alert_camera_title,
                R.string.alert_camera_text,
                { openAppSettings() },
            )
            !isBatteryExempt() && prefs.getBoolean(PREF_ASKED_BATTERY, false) -> Triple(
                R.string.alert_battery_title,
                R.string.alert_battery_text,
                { if (isXiaomi()) openMiuiBatterySettings() else requestBatteryExemption() },
            )
            else -> {
                alert.visibility = View.GONE
                fixAlert = null
                return
            }
        }
        alertTitle.text = getString(title)
        alertText.text = getString(text)
        fixAlert = fix
        alert.visibility = View.VISIBLE
    }

    override fun onResume() {
        super.onResume()
        StreamService.observe(observer)
        renderAlert()
        // Camera permission granted elsewhere (system settings) since the cameras were read.
        if (capabilities != null && !probedWithCamera && hasCameraPermission()) probeCapabilities()
    }

    override fun onPause() {
        StreamService.removeObserver(observer)
        super.onPause()
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        handlePairingLink(intent)
    }

    // ------------------------------------------------------------------ Wi-Fi pairing (§13)

    /** The first-launch screens, if not done yet (again after a pairing link closed them). */
    private fun maybeWelcome() {
        if (needsWelcome()) startActivity(Intent(this, WelcomeActivity::class.java))
    }

    /**
     * `oculibre://pair?…` from the computer's QR code (opened by the camera app) or sent over
     * USB. Any app or web page can open such a link: nothing is saved before the user confirms,
     * after comparing the key with the one the computer shows. Returns whether a dialog opened.
     */
    private fun handlePairingLink(intent: Intent?): Boolean {
        val link = intent?.data?.toString() ?: return false
        if (!link.startsWith("oculibre://pair", ignoreCase = true)) return false
        intent.data = null
        // Reopened from the recent apps: Android replays the first intent, link included.
        if (intent.flags and Intent.FLAG_ACTIVITY_LAUNCHED_FROM_HISTORY != 0) return false
        val offer = PairingOffer.parse(link)
        if (offer == null) {
            stateDetail.text = "This pairing link is not valid (or comes from a newer Oculibre): update the app"
            return false
        }
        // Each QR code has a one-time token: an offer already shown (accepted or cancelled) is
        // never asked again; a new QR code is.
        if (prefs.getString(PREF_LAST_OFFER, null) == offer.token) return false
        prefs.edit().putString(PREF_LAST_OFFER, offer.token).apply()
        // The computer's address on this phone's network (it may also have VPN or container ones).
        val address = Networks.rank(offer.addresses, Networks.local()).first()
        AlertDialog.Builder(this)
            .setTitle("Pair with ${offer.name}?")
            .setMessage(
                "Wi-Fi streaming to this computer will be encrypted, and only this computer will be " +
                    "able to receive it.\n\nCheck that the computer shows the same code:\n" +
                    "${offer.shortFingerprint}\n\nAddress: ${address}",
            )
            .setPositiveButton("Pair") { _, _ ->
                PairingStore.save(this, offer)
                prefs.edit().putString(PREF_HOST, address).apply()
                renderComputer()
                stateDetail.text = "Paired with ${offer.name}: press Start"
            }
            .setNegativeButton("Cancel", null)
            // The link closed the first-launch screens (this screen is singleTask): back to them.
            .setOnDismissListener { maybeWelcome() }
            .show()
        return true
    }

    /**
     * The first-launch screens, once. A phone that already has the camera (an install from before
     * them) skips them.
     */
    private fun needsWelcome(): Boolean {
        if (prefs.getBoolean(PREF_WELCOMED, false)) return false
        if (hasCameraPermission() && prefs.contains(PREF_MODE)) {
            prefs.edit().putBoolean(PREF_WELCOMED, true).apply()
            return false
        }
        return true
    }

    // ------------------------------------------------------------------ cameras

    private fun hasCameraPermission(): Boolean =
        Build.VERSION.SDK_INT < 23 ||
            checkSelfPermission(Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED

    /** Reads what the cameras can do (off the UI thread), then calls [then] on the UI thread. */
    private fun probeCapabilities(then: (() -> Unit)? = null) {
        val withCamera = hasCameraPermission()
        thread(name = "oculibre-probe") {
            val result = runCatching { Capabilities.probe(this) }
            ui.post {
                if (isFinishing) return@post
                result.onSuccess {
                    probedWithCamera = withCamera
                    capabilities = it
                    supportedModes = it.main.modes.filter { m -> m.supported }
                    render(StreamService.current)
                    then?.invoke()
                }.onFailure { stateDetail.text = "Cannot read camera capabilities: ${it.message}" }
            }
        }
    }

    // ------------------------------------------------------------------ start / stop

    private fun toggle() {
        if (StreamService.current.running) StreamService.stop(this) else start()
    }

    private fun start() {
        if (Build.VERSION.SDK_INT >= 23) {
            val missing = mutableListOf<String>()
            if (!hasCameraPermission()) missing += Manifest.permission.CAMERA
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
        // The quality chosen last (from the computer, or in Advanced), else the default one.
        val savedId = prefs.getString(PREF_MODE, Capabilities.DEFAULT_MODE_ID)
        val support = supportedModes.firstOrNull { it.id == savedId }
            ?: supportedModes.firstOrNull { it.id == Capabilities.DEFAULT_MODE_ID }
            ?: supportedModes.firstOrNull()
            ?: return
        val host = prefs.getString(PREF_HOST, DEFAULT_HOST) ?: DEFAULT_HOST

        StreamService.start(this, host, PORT, StreamConfig.from(support, caps.main, caps))

        // First start only: ask for the battery exemption right away (the warning banner shows
        // afterwards until it is granted).
        if (!isBatteryExempt() && !prefs.getBoolean(PREF_ASKED_BATTERY, false)) {
            prefs.edit().putBoolean(PREF_ASKED_BATTERY, true).apply()
            requestBatteryExemption()
        }
    }

    override fun onRequestPermissionsResult(
        requestCode: Int,
        permissions: Array<out String>,
        grantResults: IntArray,
    ) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        if (requestCode != REQ_CAMERA) return
        if (hasCameraPermission()) {
            prefs.edit().putBoolean(PREF_CAMERA_GRANTED_ONCE, true).apply()
            // Read the cameras again first: without the permission the hidden 60 fps were missing.
            probeCapabilities { start() }
        } else {
            stateDetail.text = getString(R.string.camera_denied)
        }
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

    /** Android's own popup: "Let Oculibre always run in the background?" */
    private fun requestBatteryExemption() {
        if (Build.VERSION.SDK_INT < 23 || isBatteryExempt()) return
        val popup = Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS, Uri.parse("package:$packageName"))
        try {
            startActivity(popup)
        } catch (e: Exception) {
            runCatching { startActivity(Intent(Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS)) }
        }
    }

    /** MIUI has its own battery saver on top of Android's; open the per-app page if we can. */
    private fun openMiuiBatterySettings() {
        val miui = Intent("miui.intent.action.HIDDEN_APPS_CONFIG_ACTIVITY")
            .putExtra("package_name", packageName)
            .putExtra("package_label", getString(R.string.app_name))
        try {
            startActivity(miui)
        } catch (e: Exception) {
            openAppSettings()
        }
    }

    private fun openAppSettings() {
        runCatching {
            startActivity(Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.parse("package:$packageName")))
        }
    }

    companion object {
        const val PORT = 47100
        const val DEFAULT_HOST = "127.0.0.1"
        const val PREF_HOST = PairingStore.HOST_PREF
        const val PREF_MODE = "mode"
        private const val REQ_CAMERA = 1
        const val PREF_ASKED_NOTIFICATIONS = "asked_notifications"
        const val PREF_ASKED_BATTERY = "asked_battery"
        /** The camera was granted once: if it is missing now, it was withdrawn. */
        const val PREF_CAMERA_GRANTED_ONCE = "camera_granted_once"
        /** The first-launch screens were done. */
        const val PREF_WELCOMED = "welcomed"
        /** Token of the last pairing link shown: the same link is not asked twice. */
        private const val PREF_LAST_OFFER = "last_pairing_offer"
        private const val PREF_SCREEN_OFF = "screen_off_while_live"
    }
}
