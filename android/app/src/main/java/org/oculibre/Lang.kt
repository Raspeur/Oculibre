package org.oculibre

import android.content.Context
import android.content.res.Configuration
import java.util.Locale

/**
 * The app's language: the phone's (default), or one chosen in the settings. Every screen applies
 * it when it is created ([wrap] in attachBaseContext); Android 13 has a per-app language setting
 * of its own, this works from Android 5.
 */
object Lang {
    /** "" = the phone's language. */
    const val PREF = "language"
    val CHOICES = listOf("", "fr", "en")

    fun get(context: Context): String =
        context.getSharedPreferences("oculibre", Context.MODE_PRIVATE).getString(PREF, "") ?: ""

    fun set(context: Context, tag: String) {
        context.getSharedPreferences("oculibre", Context.MODE_PRIVATE).edit().putString(PREF, tag).apply()
    }

    /** The context a screen should use: the chosen language, or [base] unchanged. */
    fun wrap(base: Context): Context {
        val tag = get(base)
        if (tag.isEmpty()) return base
        val locale = Locale(tag)
        Locale.setDefault(locale)
        val config = Configuration(base.resources.configuration)
        config.setLocale(locale)
        return base.createConfigurationContext(config)
    }

    /** "Français", "English": each language in its own words. */
    fun name(tag: String): String {
        val locale = Locale(tag)
        return locale.getDisplayLanguage(locale).replaceFirstChar { it.titlecase(locale) }
    }
}
