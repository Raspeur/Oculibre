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
import android.view.View
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.ScrollView
import java.text.DateFormat
import java.text.SimpleDateFormat
import java.util.Locale

/**
 * Settings with the checks (design/mockups/PhoneWelcomeB): what Oculibre needs (camera,
 * notifications, staying active, Wi-Fi pairing) with a "Fix" for each, the paired computer,
 * the language, about, and the technical screens. Rebuilt at every resume, so a fix made in
 * Android's settings shows at once.
 */
class SettingsActivity : Activity() {

    private lateinit var content: LinearLayout
    private val prefs by lazy { getSharedPreferences("oculibre", Context.MODE_PRIVATE) }

    override fun attachBaseContext(base: Context) = super.attachBaseContext(Lang.wrap(base))

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        content = Ui.column(this, 18f).apply {
            val p = Ui.dp(this@SettingsActivity, 16f)
            setPadding(p, p, p, Ui.dp(this@SettingsActivity, 24f))
        }
        val header = Ui.row(this, 12f).apply {
            val p = Ui.dp(this@SettingsActivity, 16f)
            setPadding(p, Ui.dp(this@SettingsActivity, 10f), p, Ui.dp(this@SettingsActivity, 10f))
            background = Ui.rounded(this@SettingsActivity, R.color.oc_surface_1, 0f)
            addView(Ui.iconButton(this@SettingsActivity, "‹", getString(R.string.cancel)) { finish() })
            addView(Ui.text(this@SettingsActivity, Ui.TITLE_SM, display = true).apply { text = getString(R.string.settings_title) })
        }
        val page = Ui.column(this).apply {
            addView(header)
            addView(ScrollView(this@SettingsActivity).apply { addView(content) },
                LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, 0, 1f))
        }
        setContentView(FrameLayout(this).apply {
            fitsSystemWindows = true
            addView(page)
        })
    }

    override fun onResume() {
        super.onResume()
        render()
    }

    private fun render() {
        content.removeAllViews()

        // --- checks
        val camera = hasCameraPermission()
        val notifications = hasNotificationPermission()
        val battery = isBatteryExempt()
        val paired = PairingStore.get(this)
        val toFix = listOf(camera, notifications, battery).count { !it }
        content.addView(Ui.row(this).apply {
            addView(Ui.grow(Ui.sectionLabel(this@SettingsActivity, getString(R.string.checks))))
            addView(Ui.text(this@SettingsActivity, Ui.TEXT_MD, if (toFix == 0) R.color.oc_live else R.color.oc_accent_text, strong = true).apply {
                text = if (toFix == 0) getString(R.string.checks_all_good) else getString(R.string.checks_to_fix, toFix)
                setPadding(0, 0, Ui.dp(this@SettingsActivity, 4f), 0)
            })
        })
        val fix = getString(R.string.fix)
        content.addView(Ui.listCard(this).apply {
            addView(Ui.checkRow(this@SettingsActivity, getString(R.string.check_camera),
                getString(if (camera) R.string.check_camera_ok else R.string.check_camera_todo), camera, fix) {
                if (!camera) requestOrOpen(Manifest.permission.CAMERA)
            })
            if (Build.VERSION.SDK_INT >= 33) {
                addView(Ui.checkRow(this@SettingsActivity, getString(R.string.check_notifications),
                    getString(if (notifications) R.string.check_notifications_ok else R.string.check_notifications_todo),
                    notifications, fix) {
                    if (!notifications) requestOrOpen(Manifest.permission.POST_NOTIFICATIONS)
                })
            }
            addView(Ui.checkRow(this@SettingsActivity, getString(R.string.check_battery),
                getString(if (battery) R.string.check_battery_ok else R.string.check_battery_todo), battery, fix) {
                if (!battery) fixBattery()
            })
            // Pairing is only for Wi-Fi: not counted as a problem (the cable needs nothing).
            addView(Ui.checkRow(this@SettingsActivity, getString(R.string.check_paired),
                paired?.name ?: getString(R.string.check_paired_todo), paired != null, fix) {
                if (paired == null) startActivity(Intent(this@SettingsActivity, ScannerActivity::class.java))
            })
        })

        // --- computer
        content.addView(Ui.sectionLabel(this, getString(R.string.section_computer)))
        content.addView(Ui.listCard(this).apply {
            addView(Ui.card(this@SettingsActivity, 14f).apply {
                background = null
                addView(Ui.badge(this@SettingsActivity, "PC", R.color.oc_surface_4, R.color.oc_text_2).apply { textSize = Ui.TEXT_SM })
                addView(Ui.grow(Ui.column(this@SettingsActivity, 3f).apply {
                    addView(Ui.text(this@SettingsActivity, Ui.TEXT_LG, strong = true).apply {
                        text = paired?.name ?: getString(R.string.computer_none)
                    })
                    addView(Ui.text(this@SettingsActivity, Ui.TEXT_MD, R.color.oc_text_muted).apply {
                        text = if (paired == null) {
                            getString(R.string.computer_none_text)
                        } else {
                            listOfNotNull(
                                getString(R.string.computer_code, PairingOffer.shortFingerprint(paired.fingerprint)),
                                paired.pairedOn?.let { getString(R.string.computer_paired_on, readableDate(it)) },
                            ).joinToString(" · ")
                        }
                    })
                }))
            })
            // Scan again at any time: another computer, or the same one after it lost its key.
            addView(Ui.actionRow(this@SettingsActivity,
                getString(if (paired == null) R.string.scan_title else R.string.scan_other_computer), null) {
                startActivity(Intent(this@SettingsActivity, ScannerActivity::class.java))
            })
            if (paired != null) {
                addView(Ui.actionRow(this@SettingsActivity, getString(R.string.forget_computer), "", R.color.oc_accent_text) {
                    AlertDialog.Builder(this@SettingsActivity)
                        .setMessage(getString(R.string.forget_computer_confirm, paired.name))
                        .setPositiveButton(R.string.forget) { _, _ ->
                            PairingStore.forget(this@SettingsActivity)
                            render()
                        }
                        .setNegativeButton(R.string.cancel, null)
                        .show()
                })
            }
        })

        // --- app
        content.addView(Ui.sectionLabel(this, getString(R.string.section_app)))
        val language = Lang.get(this)
        content.addView(Ui.listCard(this).apply {
            addView(Ui.actionRow(this@SettingsActivity, getString(R.string.language),
                if (language.isEmpty()) getString(R.string.language_auto) else Lang.name(language)) { chooseLanguage() })
            addView(Ui.actionRow(this@SettingsActivity, getString(R.string.about),
                getString(R.string.about_version, version())) { showAbout() })
        })

        // --- advanced
        content.addView(Ui.sectionLabel(this, getString(R.string.section_advanced)))
        content.addView(Ui.listCard(this).apply {
            addView(Ui.actionRow(this@SettingsActivity, getString(R.string.advanced_settings), getString(R.string.advanced_settings_value)) {
                startActivity(Intent(this@SettingsActivity, AdvancedActivity::class.java))
            })
            addView(Ui.actionRow(this@SettingsActivity, getString(R.string.lab), null) {
                startActivity(Intent(this@SettingsActivity, LabActivity::class.java))
            })
        })
    }

    // ------------------------------------------------------------------ fixes

    /** Android asks again, unless the user refused for good: then the app's settings page. */
    private fun requestOrOpen(permission: String) {
        if (Build.VERSION.SDK_INT >= 23 && shouldShowRequestPermissionRationale(permission) ||
            Build.VERSION.SDK_INT >= 23 && !prefs.getBoolean("asked_$permission", false)
        ) {
            prefs.edit().putBoolean("asked_$permission", true).apply()
            requestPermissions(arrayOf(permission), 1)
        } else {
            openAppSettings()
        }
    }

    override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<out String>, grantResults: IntArray) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        if (hasCameraPermission()) prefs.edit().putBoolean(MainActivity.PREF_CAMERA_GRANTED_ONCE, true).apply()
        render()
    }

    private fun fixBattery() {
        prefs.edit().putBoolean(MainActivity.PREF_ASKED_BATTERY, true).apply()
        val popup = Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS, Uri.parse("package:$packageName"))
        try {
            startActivity(popup)
        } catch (e: Exception) {
            openAppSettings()
        }
    }

    private fun openAppSettings() {
        runCatching {
            startActivity(Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.parse("package:$packageName")))
        }
    }

    // ------------------------------------------------------------------ app

    private fun chooseLanguage() {
        val current = Lang.CHOICES.indexOf(Lang.get(this)).coerceAtLeast(0)
        val labels = Lang.CHOICES.map { if (it.isEmpty()) getString(R.string.language_auto) else Lang.name(it) }
        AlertDialog.Builder(this)
            .setTitle(R.string.language)
            .setSingleChoiceItems(labels.toTypedArray(), current) { dialog, which ->
                dialog.dismiss()
                if (which == current) return@setSingleChoiceItems
                Lang.set(this, Lang.CHOICES[which])
                // Every screen again, in the new language: the everyday one, then this one.
                startActivity(Intent(this, MainActivity::class.java)
                    .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK))
                startActivity(Intent(this, SettingsActivity::class.java))
            }
            .show()
    }

    private fun showAbout() {
        AlertDialog.Builder(this)
            .setTitle("Oculibre ${version()}")
            .setMessage(R.string.about_text)
            .setPositiveButton(R.string.ok, null)
            .show()
    }

    private fun version(): String =
        runCatching { packageManager.getPackageInfo(packageName, 0).versionName }.getOrNull() ?: "?"

    /** "2026-11-12" → "12 nov. 2026" in the app's language. */
    private fun readableDate(iso: String): String = runCatching {
        val date = SimpleDateFormat("yyyy-MM-dd", Locale.ROOT).parse(iso)!!
        DateFormat.getDateInstance(DateFormat.MEDIUM).format(date)
    }.getOrDefault(iso)

    // ------------------------------------------------------------------ state

    private fun hasCameraPermission(): Boolean =
        Build.VERSION.SDK_INT < 23 ||
            checkSelfPermission(Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED

    private fun hasNotificationPermission(): Boolean =
        Build.VERSION.SDK_INT < 33 ||
            checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED

    private fun isBatteryExempt(): Boolean {
        if (Build.VERSION.SDK_INT < 23) return true
        return (getSystemService(Context.POWER_SERVICE) as PowerManager).isIgnoringBatteryOptimizations(packageName)
    }
}
