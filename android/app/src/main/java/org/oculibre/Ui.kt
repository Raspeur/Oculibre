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

    /** Takes the remaining width in a row. */
    fun grow(view: View) = view.apply {
        layoutParams = LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f)
    }
}
