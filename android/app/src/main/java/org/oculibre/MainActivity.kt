package org.oculibre

import android.Manifest
import android.app.Activity
import android.content.pm.PackageManager
import android.graphics.Typeface
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.text.InputType
import android.view.WindowManager
import android.widget.Button
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.TextView

/**
 * Phase-1 test screen: one address field, one Start/Stop button, one status line.
 * The UI is built in code on purpose (no AndroidX, no layout XML) to keep the spike minimal.
 */
class MainActivity : Activity() {

    private lateinit var hostInput: EditText
    private lateinit var button: Button
    private lateinit var status: TextView

    private var session: StreamSession? = null
    private val ui = Handler(Looper.getMainLooper())

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)

        val pad = (16 * resources.displayMetrics.density).toInt()

        val label = TextView(this).apply {
            text = "PC address (127.0.0.1 over USB with adb reverse, or the PC's IP over Wi-Fi)"
        }
        hostInput = EditText(this).apply {
            setText(DEFAULT_HOST)
            setSingleLine()
            inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_URI
        }
        button = Button(this).apply {
            text = "Start"
            setOnClickListener { toggle() }
        }
        status = TextView(this).apply {
            text = "Idle — port $PORT"
            typeface = Typeface.MONOSPACE
            setPadding(0, pad, 0, 0)
        }

        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            fitsSystemWindows = true
            setPadding(pad, pad, pad, pad)
            addView(label)
            addView(hostInput)
            addView(button)
            addView(status)
        }
        setContentView(root)
    }

    private fun toggle() {
        val current = session
        if (current == null) start() else current.stop()
    }

    private fun start() {
        if (Build.VERSION.SDK_INT >= 23 &&
            checkSelfPermission(Manifest.permission.CAMERA) != PackageManager.PERMISSION_GRANTED
        ) {
            requestPermissions(arrayOf(Manifest.permission.CAMERA), REQ_CAMERA)
            return
        }

        val host = hostInput.text.toString().trim().ifEmpty { DEFAULT_HOST }
        val newSession = StreamSession(this, host, PORT, StreamConfig(), listener)
        session = newSession
        button.text = "Stop"
        hostInput.isEnabled = false
        status.text = "Connecting to $host:$PORT…"
        newSession.start()
    }

    private val listener = object : StreamSession.Listener {
        override fun onStatus(session: StreamSession, text: String) {
            ui.post { if (this@MainActivity.session === session) status.text = text }
        }

        override fun onStopped(session: StreamSession, error: String?) {
            ui.post {
                if (this@MainActivity.session !== session) return@post
                this@MainActivity.session = null
                button.text = "Start"
                hostInput.isEnabled = true
                status.text = if (error == null) "Stopped" else "Stopped: $error"
            }
        }
    }

    override fun onRequestPermissionsResult(
        requestCode: Int,
        permissions: Array<out String>,
        grantResults: IntArray,
    ) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        if (requestCode != REQ_CAMERA) return
        if (grantResults.firstOrNull() == PackageManager.PERMISSION_GRANTED) {
            start()
        } else {
            status.text = "Camera permission denied"
        }
    }

    override fun onDestroy() {
        session?.stop()
        session = null
        super.onDestroy()
    }

    companion object {
        const val PORT = 47100
        const val DEFAULT_HOST = "127.0.0.1"
        private const val REQ_CAMERA = 1
    }
}
