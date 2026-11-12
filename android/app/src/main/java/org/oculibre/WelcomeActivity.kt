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
import android.os.PowerManager
import android.provider.Settings
import android.view.Gravity
import android.view.View
import android.widget.FrameLayout
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView

/**
 * First launch, one step per screen (design/mockups/PhoneWelcomeA): the camera, notifications
 * (Android 13+), staying active with the screen off, connecting to the computer, then "all set". A step already settled is not
 * shown. Shown once, before the everyday screen ([MainActivity.needsWelcome]).
 */
class WelcomeActivity : Activity() {

    override fun attachBaseContext(base: Context) = super.attachBaseContext(Lang.wrap(base))

    private enum class Step { CAMERA, NOTIFICATIONS, BATTERY, CONNECT, READY }

    private lateinit var steps: List<Step>
    private var index = 0

    private lateinit var segments: LinearLayout
    private lateinit var progress: TextView
    private lateinit var hero: FrameLayout
    private lateinit var heroIcon: ImageView
    private lateinit var title: TextView
    private lateinit var body: TextView
    private lateinit var action: TextView
    private lateinit var secondary: TextView

    private val prefs by lazy { getSharedPreferences("oculibre", Context.MODE_PRIVATE) }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // The steps are chosen once. Android may destroy this screen while a system window is on
        // top (permission, battery setting, camera app) and create it again: then they come back
        // from the saved state, or the steps already done would vanish ("step 2 of 2").
        val saved = savedInstanceState?.getStringArrayList(STATE_STEPS)
        steps = saved?.map { Step.valueOf(it) } ?: buildList {
            if (!hasCameraPermission()) add(Step.CAMERA)
            if (!hasNotificationPermission()) add(Step.NOTIFICATIONS)
            if (!isBatteryExempt()) add(Step.BATTERY)
            if (PairingStore.get(this@WelcomeActivity) == null) add(Step.CONNECT)
            add(Step.READY)
        }
        index = (savedInstanceState?.getInt(STATE_INDEX) ?: 0).coerceIn(0, steps.size - 1)
        setContentView(buildScreen())
        show()
    }

    override fun onSaveInstanceState(outState: Bundle) {
        super.onSaveInstanceState(outState)
        outState.putStringArrayList(STATE_STEPS, ArrayList(steps.map { it.name }))
        outState.putInt(STATE_INDEX, index)
    }

    override fun onResume() {
        super.onResume()
        // Back from the battery popup (or the settings): settled, go on.
        if (current() == Step.BATTERY && isBatteryExempt()) next()
        // Back from the scanner with the computer's QR code read and confirmed: connected, go on.
        if (current() == Step.CONNECT && PairingStore.get(this) != null) next()
    }

    override fun onBackPressed() {
        if (index > 0) {
            index--
            show()
        } else {
            @Suppress("DEPRECATION")
            super.onBackPressed()
        }
    }

    // ------------------------------------------------------------------ layout

    private fun buildScreen(): View {
        val c = this
        segments = Ui.row(c, 6f)
        progress = Ui.text(c, Ui.TEXT_MD, R.color.oc_text_muted)
        heroIcon = ImageView(c)
        hero = FrameLayout(c).apply {
            layoutParams = LinearLayout.LayoutParams(Ui.dp(c, 96f), Ui.dp(c, 96f))
            addView(heroIcon, FrameLayout.LayoutParams(Ui.dp(c, 44f), Ui.dp(c, 44f), Gravity.CENTER))
        }
        title = Ui.text(c, 26f, display = true).apply { gravity = Gravity.CENTER }
        body = Ui.text(c, Ui.TEXT_LG, R.color.oc_text_muted).apply {
            gravity = Gravity.CENTER
            setLineSpacing(0f, 1.35f)
            maxWidth = Ui.dp(c, 320f)
        }
        action = Ui.primaryButton(c, "") { onAction() }
        secondary = Ui.ghostButton(c, "") { onSecondary() }

        val center = Ui.column(c, 20f).apply {
            gravity = Gravity.CENTER
            layoutParams = LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, 0, 1f)
            addView(hero)
            addView(title)
            addView(body)
        }
        val column = Ui.column(c, 10f).apply {
            val p = Ui.dp(c, 24f)
            setPadding(p, p, p, Ui.dp(c, 28f))
            addView(segments)
            addView(progress)
            addView(center)
            addView(action)
            addView(secondary)
        }
        val scroll = ScrollView(c).apply {
            isFillViewport = true
            addView(column, FrameLayout.LayoutParams(FrameLayout.LayoutParams.MATCH_PARENT, FrameLayout.LayoutParams.MATCH_PARENT))
        }
        return FrameLayout(c).apply {
            fitsSystemWindows = true
            addView(scroll, FrameLayout.LayoutParams(FrameLayout.LayoutParams.MATCH_PARENT, FrameLayout.LayoutParams.MATCH_PARENT))
        }
    }

    private fun current(): Step = steps[index]

    /** Draws the current step. */
    private fun show() {
        val step = current()
        val total = steps.size - 1 // "ready" is not a step to do
        segments.removeAllViews()
        repeat(total) { i ->
            segments.addView(View(this).apply {
                val colour = when {
                    i < index -> R.color.oc_live
                    i == index -> R.color.oc_accent
                    else -> R.color.oc_track
                }
                background = Ui.rounded(this@WelcomeActivity, colour, 2f)
                layoutParams = LinearLayout.LayoutParams(0, Ui.dp(this@WelcomeActivity, 4f), 1f)
            })
        }
        progress.text = if (step == Step.READY) getString(R.string.welcome_done_step)
        else getString(R.string.welcome_step, index + 1, total)

        val ready = step == Step.READY
        hero.background = Ui.oval(this, if (ready) R.color.oc_live_soft else R.color.oc_accent_soft,
            if (ready) R.color.oc_live_soft else R.color.oc_accent_soft, 0f)
        heroIcon.setImageResource(
            when (step) {
                Step.CAMERA -> R.drawable.ic_videocam
                Step.NOTIFICATIONS -> R.drawable.ic_notifications
                Step.BATTERY -> R.drawable.ic_battery
                Step.CONNECT -> R.drawable.ic_computer
                Step.READY -> R.drawable.ic_check
            },
        )
        val (t, b, a, s) = when (step) {
            Step.CAMERA -> listOf(R.string.welcome_camera_title, R.string.welcome_camera_text, R.string.welcome_camera_action, R.string.welcome_camera_why)
            Step.NOTIFICATIONS -> listOf(R.string.welcome_notifications_title, R.string.welcome_notifications_text, R.string.welcome_notifications_action, R.string.welcome_later)
            Step.BATTERY -> listOf(R.string.welcome_battery_title, R.string.welcome_battery_text, R.string.welcome_battery_action, R.string.welcome_later)
            // Over USB there is nothing to scan: "next", not "later".
            Step.CONNECT -> listOf(R.string.welcome_connect_title, R.string.welcome_connect_text, R.string.welcome_connect_action, R.string.welcome_next)
            Step.READY -> listOf(R.string.welcome_ready_title, R.string.welcome_ready_text, R.string.welcome_ready_action, 0)
        }
        title.text = getString(t)
        body.text = getString(b)
        action.text = getString(a)
        secondary.visibility = if (s == 0) View.INVISIBLE else View.VISIBLE
        if (s != 0) secondary.text = getString(s)
    }

    private fun next() {
        if (index < steps.size - 1) {
            index++
            show()
        }
    }

    // ------------------------------------------------------------------ actions

    private fun onAction() {
        when (current()) {
            Step.CAMERA ->
                if (Build.VERSION.SDK_INT >= 23) requestPermissions(arrayOf(Manifest.permission.CAMERA), REQ_CAMERA) else next()
            Step.NOTIFICATIONS -> {
                prefs.edit().putBoolean(MainActivity.PREF_ASKED_NOTIFICATIONS, true).apply()
                if (Build.VERSION.SDK_INT >= 33) {
                    requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), REQ_NOTIFICATIONS)
                } else {
                    next()
                }
            }
            Step.BATTERY -> {
                prefs.edit().putBoolean(MainActivity.PREF_ASKED_BATTERY, true).apply()
                val popup = Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS, Uri.parse("package:$packageName"))
                try {
                    startActivity(popup)
                } catch (e: Exception) {
                    next() // no such popup on this phone: nothing more to do here
                }
            }
            // The in-app scanner. Stay on this step: it goes on by itself once the phone is
            // paired (onResume), and stays if the user came back without pairing.
            Step.CONNECT -> startActivity(Intent(this, ScannerActivity::class.java))
            Step.READY -> finishWelcome()
        }
    }

    private fun onSecondary() {
        when (current()) {
            Step.CAMERA -> AlertDialog.Builder(this)
                .setMessage(R.string.welcome_camera_why_text)
                .setPositiveButton(R.string.ok, null)
                .show()
            Step.NOTIFICATIONS -> {
                prefs.edit().putBoolean(MainActivity.PREF_ASKED_NOTIFICATIONS, true).apply()
                next()
            }
            Step.BATTERY -> {
                prefs.edit().putBoolean(MainActivity.PREF_ASKED_BATTERY, true).apply()
                next()
            }
            Step.CONNECT -> next()
            Step.READY -> {}
        }
    }

    override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<out String>, grantResults: IntArray) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        // Notifications are optional: allowed or not, go on (the stream works without them).
        if (requestCode == REQ_NOTIFICATIONS) {
            next()
            return
        }
        if (requestCode != REQ_CAMERA) return
        if (hasCameraPermission()) {
            prefs.edit().putBoolean(MainActivity.PREF_CAMERA_GRANTED_ONCE, true).apply()
            next()
        } else {
            body.text = getString(R.string.welcome_camera_denied)
        }
    }

    private fun finishWelcome() {
        prefs.edit().putBoolean(MainActivity.PREF_WELCOMED, true).apply()
        finish()
    }

    private fun hasCameraPermission(): Boolean =
        Build.VERSION.SDK_INT < 23 ||
            checkSelfPermission(Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED

    /** Android 13+ asks for it; before, notifications are allowed by default. */
    private fun hasNotificationPermission(): Boolean =
        Build.VERSION.SDK_INT < 33 ||
            checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED

    private fun isBatteryExempt(): Boolean {
        if (Build.VERSION.SDK_INT < 23) return true
        return (getSystemService(Context.POWER_SERVICE) as PowerManager).isIgnoringBatteryOptimizations(packageName)
    }

    companion object {
        private const val REQ_CAMERA = 2
        private const val REQ_NOTIFICATIONS = 3
        private const val STATE_STEPS = "steps"
        private const val STATE_INDEX = "index"
    }
}
