package org.oculibre

import android.content.Context
import android.content.res.ColorStateList
import android.view.Gravity
import android.view.View
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.ProgressBar
import android.widget.TextView

/**
 * The alert of design/mockups/ui.css (.oc-alert) on the phone: an icon square (spinner, ! or ×),
 * a title, a text, the problem's code, and one or two buttons. Colours by level: progress grey,
 * info blue, warning orange, error red.
 */
class ProblemBanner(private val context: Context) {

    /** A button of the banner; [style] picks its colours. */
    class Action(val label: String, val style: Style = Style.PRIMARY, val onClick: () -> Unit)

    enum class Style { PRIMARY, SECONDARY, DANGER }

    private val mark: TextView = Ui.text(context, Ui.TEXT_BASE, display = true).apply {
        gravity = Gravity.CENTER
    }
    private val spinner = ProgressBar(context).apply {
        isIndeterminate = true
        indeterminateTintList = ColorStateList.valueOf(Ui.color(context, R.color.oc_text_2))
    }
    private val icon = FrameLayout(context).apply {
        val size = Ui.dp(context, 28f)
        layoutParams = LinearLayout.LayoutParams(size, size).apply { gravity = Gravity.TOP }
        addView(mark, FrameLayout.LayoutParams(size, size))
        val s = Ui.dp(context, 16f)
        addView(spinner, FrameLayout.LayoutParams(s, s, Gravity.CENTER))
    }
    private val title = Ui.text(context, Ui.TEXT_BASE, strong = true)
    private val text = Ui.text(context, Ui.TEXT_MD, R.color.oc_text_2).apply { setLineSpacing(0f, 1.35f) }
    private val code = Ui.text(context, 11f, R.color.oc_text_faint).apply { letterSpacing = 0.03f }
    private val actions = Ui.row(context, 8f).apply { setPadding(0, Ui.dp(context, 6f), 0, 0) }

    val view: LinearLayout = Ui.row(context, 12f).apply {
        gravity = Gravity.TOP
        val p = Ui.dp(context, 12f)
        setPadding(Ui.dp(context, 14f), p, Ui.dp(context, 14f), p)
        visibility = View.GONE
        addView(icon)
        addView(Ui.grow(Ui.column(context, 3f).apply {
            addView(title)
            addView(text)
            addView(code)
            addView(actions)
        }))
    }

    /** [code] null: an advice without a catalogue code (battery saving). */
    fun show(level: Problem.Level, code: String?, title: String, text: String, buttons: List<Action>) {
        val (fill, line, iconFill, ink) = when (level) {
            Problem.Level.PROGRESS -> listOf(R.color.oc_surface_3, R.color.oc_border_control, R.color.oc_surface_4, R.color.oc_text_2)
            Problem.Level.INFO -> listOf(R.color.oc_info_soft, R.color.oc_info_line, R.color.oc_info_icon, R.color.oc_info)
            Problem.Level.WARNING -> listOf(R.color.oc_accent_soft, R.color.oc_accent_line, R.color.oc_accent_icon, R.color.oc_accent_text)
            Problem.Level.ERROR -> listOf(R.color.oc_danger_soft, R.color.oc_danger_line, R.color.oc_danger_icon, R.color.oc_danger)
        }
        view.background = Ui.rounded(context, fill, Ui.RADIUS_MD, line)
        icon.background = Ui.rounded(context, iconFill, Ui.RADIUS_SM)
        val progress = level == Problem.Level.PROGRESS
        spinner.visibility = if (progress) View.VISIBLE else View.GONE
        mark.visibility = if (progress) View.GONE else View.VISIBLE
        mark.text = when (level) {
            Problem.Level.INFO -> "i"
            Problem.Level.ERROR -> "×"
            else -> "!"
        }
        mark.setTextColor(Ui.color(context, ink))
        this.title.text = title
        this.text.text = text
        this.code.text = code ?: ""
        this.code.visibility = if (code == null) View.GONE else View.VISIBLE
        actions.removeAllViews()
        buttons.forEach { actions.addView(button(it)) }
        actions.visibility = if (buttons.isEmpty()) View.GONE else View.VISIBLE
        // Read out when it appears or changes (TalkBack).
        view.contentDescription = listOfNotNull(title, text, code).joinToString(". ")
        if (view.visibility != View.VISIBLE) {
            view.visibility = View.VISIBLE
            view.announceForAccessibility(title)
        }
    }

    fun hide() {
        view.visibility = View.GONE
    }

    /** .oc-btn .oc-btn--sm in the three styles of the alerts. */
    private fun button(action: Action) = Ui.text(context, Ui.TEXT_MD, strong = true).apply {
        text = action.label
        gravity = Gravity.CENTER
        minHeight = Ui.dp(context, 36f)
        val p = Ui.dp(context, 14f)
        setPadding(p, 0, p, 0)
        when (action.style) {
            Style.PRIMARY -> {
                background = Ui.rounded(context, R.color.oc_accent, Ui.RADIUS_SM)
                setTextColor(Ui.color(context, R.color.oc_on_accent))
            }
            Style.SECONDARY -> {
                background = Ui.rounded(context, R.color.oc_surface_4, Ui.RADIUS_SM, R.color.oc_border_strong)
                setTextColor(Ui.color(context, R.color.oc_text))
            }
            Style.DANGER -> {
                background = Ui.rounded(context, android.R.color.transparent, Ui.RADIUS_SM, R.color.oc_danger_line)
                setTextColor(Ui.color(context, R.color.oc_danger))
            }
        }
        isClickable = true
        isFocusable = true
        setOnClickListener { action.onClick() }
    }
}
