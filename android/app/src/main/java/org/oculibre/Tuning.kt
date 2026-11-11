package org.oculibre

import android.content.Context

/**
 * Latency experiments (#12), switched in the Lab screen and read when a stream starts, so that
 * each one can be measured (`oculibre` session summary or `oculibre bench`) without rebuilding the
 * app. The ones that help become the default and keep a switch to turn them off for diagnosis;
 * the others go away.
 *
 * Results so far (1080p60): Qualcomm low-latency encoding took the Redmi Note 9 Pro's encoder from
 * ~50 to ~21 ms (4K30: ~110 to ~53 ms) and is now on by default; the preview template, fast camera
 * processing and stabilisation off changed nothing on the Redmi or the Galaxy S23.
 */
data class Tuning(
    /** Qualcomm encoders: `vendor.qti-ext-enc-low-latency.enable` (ignored by other encoders). */
    val qcomLowLatency: Boolean = true,
) {
    /** What differs from the defaults, for the status ("", or "qcom low latency off"). */
    val label: String
        get() = ITEMS.filter { it.get(this) != it.default }.joinToString(" · ") { "${it.short} ${if (it.get(this)) "on" else "off"}" }

    class Item(
        val key: String,
        val short: String,
        /** Label of the Lab check box (checked = on). */
        val description: String,
        val default: Boolean,
        val get: (Tuning) -> Boolean,
    )

    companion object {
        val ITEMS = listOf(
            Item("tune_qcom", "qcom low latency", "Encoder: Qualcomm low-latency mode (default: on)", true, { it.qcomLowLatency }),
        )

        private fun prefs(context: Context) = context.getSharedPreferences("oculibre", Context.MODE_PRIVATE)

        fun load(context: Context): Tuning = Tuning(
            qcomLowLatency = isOn(context, "tune_qcom"),
        )

        fun isOn(context: Context, key: String): Boolean {
            val default = ITEMS.firstOrNull { it.key == key }?.default ?: false
            return prefs(context).getBoolean(key, default)
        }

        fun set(context: Context, key: String, on: Boolean) {
            prefs(context).edit().putBoolean(key, on).apply()
        }
    }
}
