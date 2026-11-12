package org.oculibre

import android.content.Context
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.os.Build
import android.util.TypedValue
import android.view.Gravity
import android.view.View
import android.widget.LinearLayout
import android.widget.TextView
import kotlin.math.roundToInt

/**
 * The look of design/mockups/ui.css for platform views (no AndroidX): sizes, shapes, texts.
 * Colours come from res/values/colors.xml (the same --oc-* tokens).
 */
object Ui {
    // ui.css sizes, in dp / sp
    const val RADIUS_SM = 6f // controls, buttons
    const val RADIUS_MD = 8f // cards
    const val TEXT_SM = 12f
    const val TEXT_MD = 13f
    const val TEXT_BASE = 14f
    const val TEXT_LG = 15f
    const val TITLE_SM = 20f
    const val TITLE_LG = 22f

    fun dp(context: Context, value: Float): Int =
        (value * context.resources.displayMetrics.density).roundToInt()

    fun color(context: Context, id: Int): Int =
        if (Build.VERSION.SDK_INT >= 23) {
            context.getColor(id)
        } else {
            @Suppress("DEPRECATION")
            context.resources.getColor(id)
        }

    /** A rounded rectangle: fill, optional border (1 dp). */
    fun rounded(context: Context, fill: Int, radiusDp: Float, border: Int? = null, borderDp: Float = 1f) =
        GradientDrawable().apply {
            setColor(color(context, fill))
            cornerRadius = dp(context, radiusDp).toFloat()
            border?.let { setStroke(dp(context, borderDp), color(context, it)) }
        }

    /** A disc or a ring (the big button). */
    fun oval(context: Context, fill: Int, border: Int, borderDp: Float) = GradientDrawable().apply {
        shape = GradientDrawable.OVAL
        setColor(color(context, fill))
        setStroke(dp(context, borderDp), color(context, border))
    }

    /** Body text; `strong` = semibold, `display` = the title face (Space Grotesk's role). */
    fun text(
        context: Context,
        sizeSp: Float = TEXT_BASE,
        colorId: Int = R.color.oc_text,
        strong: Boolean = false,
        display: Boolean = false,
    ) = TextView(context).apply {
        setTextSize(TypedValue.COMPLEX_UNIT_SP, sizeSp)
        setTextColor(color(context, colorId))
        typeface = when {
            display -> Typeface.create("sans-serif", Typeface.BOLD)
            strong -> Typeface.create("sans-serif-medium", Typeface.NORMAL)
            else -> Typeface.create("sans-serif", Typeface.NORMAL)
        }
        setLineSpacing(0f, 1.2f)
    }

    fun column(context: Context, gapDp: Float = 0f) = LinearLayout(context).apply {
        orientation = LinearLayout.VERTICAL
        if (gapDp > 0f) {
            showDividers = LinearLayout.SHOW_DIVIDER_MIDDLE
            dividerDrawable = GradientDrawable().apply { setSize(0, dp(context, gapDp)) }
        }
    }

    fun row(context: Context, gapDp: Float = 0f) = LinearLayout(context).apply {
        orientation = LinearLayout.HORIZONTAL
        gravity = Gravity.CENTER_VERTICAL
        if (gapDp > 0f) {
            showDividers = LinearLayout.SHOW_DIVIDER_MIDDLE
            dividerDrawable = GradientDrawable().apply { setSize(dp(context, gapDp), 0) }
        }
    }

    /** .oc-card on the phone: surface 3, border, 8 dp corners, 16 dp padding. */
    fun card(context: Context, gapDp: Float = 0f) = row(context, gapDp).apply {
        background = rounded(context, R.color.oc_surface_3, RADIUS_MD, R.color.oc_border)
        val p = dp(context, 16f)
        setPadding(p, p, p, p)
    }

    /** .oc-logo: the navy logo on its light tile. */
    fun logo(context: Context, sizeDp: Float) = android.widget.ImageView(context).apply {
        setImageResource(R.drawable.logo)
        background = rounded(context, R.color.oc_logo_tile, RADIUS_MD)
        val p = dp(context, sizeDp * 0.14f)
        setPadding(p, p, p, p)
        layoutParams = LinearLayout.LayoutParams(dp(context, sizeDp), dp(context, sizeDp))
    }

    /** .oc-icon-btn--lg: a 44 dp square button with a symbol. */
    fun iconButton(context: Context, symbol: String, description: String, onClick: () -> Unit) =
        text(context, 20f, R.color.oc_text_2).apply {
            text = symbol
            gravity = Gravity.CENTER
            contentDescription = description
            background = rounded(context, R.color.oc_surface_3, RADIUS_SM, R.color.oc_border_strong)
            layoutParams = LinearLayout.LayoutParams(dp(context, 44f), dp(context, 44f))
            isClickable = true
            isFocusable = true
            setOnClickListener { onClick() }
        }

    /** .oc-badge: a small square with a symbol (icons are kept as text: no image assets). */
    fun badge(context: Context, symbol: String, fill: Int, colorId: Int, sizeDp: Float = 32f) =
        text(context, TEXT_LG, colorId, display = true).apply {
            text = symbol
            gravity = Gravity.CENTER
            background = rounded(context, fill, RADIUS_SM)
            layoutParams = LinearLayout.LayoutParams(dp(context, sizeDp), dp(context, sizeDp))
            importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO
        }

    /** .oc-btn--primary .oc-btn--lg .oc-btn--block: the one orange action of a screen. */
    fun primaryButton(context: Context, label: String, onClick: () -> Unit) =
        text(context, TEXT_LG, R.color.oc_on_accent, strong = true).apply {
            text = label
            gravity = Gravity.CENTER
            background = rounded(context, R.color.oc_accent, RADIUS_SM)
            minHeight = dp(context, 48f)
            isClickable = true
            isFocusable = true
            setOnClickListener { onClick() }
        }

    /** .oc-btn--ghost: a quiet text button. */
    fun ghostButton(context: Context, label: String, onClick: () -> Unit) =
        text(context, TEXT_BASE, R.color.oc_text_2).apply {
            text = label
            gravity = Gravity.CENTER
            minHeight = dp(context, 44f)
            isClickable = true
            isFocusable = true
            setOnClickListener { onClick() }
        }

    /** .oc-label: a section title, small capitals. */
    fun sectionLabel(context: Context, label: String) = text(context, TEXT_SM, R.color.oc_text_muted, strong = true).apply {
        text = label.uppercase()
        letterSpacing = 0.08f
        setPadding(dp(context, 4f), 0, dp(context, 4f), 0)
    }

    /** .oc-card .oc-list: rows in a card, separated by thin lines. */
    fun listCard(context: Context) = LinearLayout(context).apply {
        orientation = LinearLayout.VERTICAL
        background = rounded(context, R.color.oc_surface_3, RADIUS_MD, R.color.oc_border)
        showDividers = LinearLayout.SHOW_DIVIDER_MIDDLE
        dividerDrawable = GradientDrawable().apply {
            setColor(color(context, R.color.oc_border))
            setSize(0, dp(context, 1f))
        }
        clipToOutline = true
    }

    /** A check of the settings: ✓ OK, or ! and "Fix ›". */
    fun checkRow(context: Context, title: String, detail: String, ok: Boolean, fix: String, onClick: () -> Unit) =
        row(context, 14f).apply {
            val p = dp(context, 14f)
            setPadding(p, dp(context, 12f), p, dp(context, 12f))
            minimumHeight = dp(context, 68f)
            isClickable = true
            isFocusable = true
            setOnClickListener { onClick() }
            addView(
                if (ok) badge(context, "✓", R.color.oc_live_soft, R.color.oc_live)
                else badge(context, "!", R.color.oc_accent_soft, R.color.oc_accent_text),
            )
            addView(grow(column(context, 3f).apply {
                addView(text(context, TEXT_LG, strong = true).apply { text = title })
                addView(text(context, TEXT_MD, R.color.oc_text_muted).apply { text = detail })
            }))
            addView(text(context, TEXT_MD, if (ok) R.color.oc_live else R.color.oc_accent_text, strong = true).apply {
                text = if (ok) context.getString(R.string.check_ok) else fix
            })
        }

    /** A row that opens something: a label, and a quiet value with ›. */
    fun actionRow(context: Context, label: String, value: String?, colorId: Int = R.color.oc_text, onClick: () -> Unit) =
        row(context, 12f).apply {
            val p = dp(context, 14f)
            setPadding(p, 0, p, 0)
            minimumHeight = dp(context, 52f)
            isClickable = true
            isFocusable = true
            setOnClickListener { onClick() }
            addView(grow(text(context, TEXT_LG, colorId).apply { text = label }))
            addView(text(context, TEXT_MD, R.color.oc_text_muted).apply { text = if (value == null) "›" else "$value ›" })
        }

    /** Takes the remaining width in a row. */
    fun grow(view: View) = view.apply {
        layoutParams = LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f)
    }
}
