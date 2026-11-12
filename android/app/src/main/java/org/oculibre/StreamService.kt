package org.oculibre

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.net.wifi.WifiManager
import android.os.Build
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.os.PowerManager
import android.os.SystemClock
import java.util.concurrent.CopyOnWriteArraySet

/**
 * Foreground service that owns the [StreamSession], so streaming survives the screen
 * turning off or another app coming to the front.
 *
 * - foreground service of type `camera` (required to keep the camera in the background);
 * - partial wake lock (CPU keeps encoding with the screen off) and Wi-Fi lock;
 * - ongoing notification with a Stop action;
 * - when the computer is lost (not when it refuses or stops the stream), a new session is tried
 *   after 1, 2, 4… up to 10 s, for at most [GIVE_UP_MS]; Stop cancels it.
 *
 * The UI talks to it through [start], [stop] and [observe] (same process, no binding needed).
 */
class StreamService : Service() {

    /** Where the service is, for the screen. */
    enum class Phase { IDLE, CONNECTING, LIVE, SEARCHING }

    /**
     * What the UI shows: [status] is a technical line (English), [phase] what to say to people,
     * [live] the quality being streamed ("1080p · 60").
     */
    data class Snapshot(
        val running: Boolean,
        val status: String?,
        val phase: Phase = if (running) Phase.CONNECTING else Phase.IDLE,
        val live: String? = null,
    )

    private class Request(val host: String, val port: Int, val config: StreamConfig)

    private var session: StreamSession? = null
    /** What the user started: kept to try again when the computer is lost. */
    private var request: Request? = null
    /** Failed attempts in a row, and since when the computer is missing (elapsedRealtime ms). */
    private var attempts = 0
    private var lostSince = 0L
    /** When the current session started: one that lasted is a fresh start (no backoff). */
    private var sessionStartedAt = 0L
    private val retry = Runnable { request?.let { if (session == null) startSession(it) } }
    private var wakeLock: PowerManager.WakeLock? = null
    private var wifiLock: WifiManager.WifiLock? = null

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent?.action == ACTION_STOP) {
            main.removeCallbacks(retry)
            request = null
            val current = session
            if (current != null) current.stop() else shutdown(null)
            return START_NOT_STICKY
        }

        val request = pending
        pending = null
        if (request == null || session != null) {
            if (session == null) shutdown(null)
            return START_NOT_STICKY
        }

        try {
            startInForeground(request)
        } catch (e: Exception) {
            // e.g. Android 12+ refusing a foreground service started from the background.
            publish(Snapshot(false, "Cannot start the streaming service: ${e.message}"))
            stopSelf()
            return START_NOT_STICKY
        }
        acquireLocks()

        this.request = request
        attempts = 0
        lostSince = 0L
        publish(Snapshot(true, "Connecting to ${request.host}:${request.port}…"))
        startSession(request)
        // Not sticky: if Android kills us, don't restart a stream nobody asked for.
        return START_NOT_STICKY
    }

    private fun startSession(request: Request) {
        val newSession = StreamSession(applicationContext, request.host, request.port, request.config, sessionListener)
        session = newSession
        sessionStartedAt = SystemClock.elapsedRealtime()
        newSession.start()
    }

    /** The computer was lost: try again later, keeping the notification and the locks. */
    private fun scheduleRetry(error: String?) {
        val now = SystemClock.elapsedRealtime()
        if (now - sessionStartedAt >= LASTED_MS || lostSince == 0L) {
            // It had been streaming (or this is the first loss): start counting again.
            attempts = 0
            lostSince = now
        }
        if (now - lostSince >= GIVE_UP_MS) {
            request = null
            shutdown("Stopped: the computer could not be reached for ${GIVE_UP_MS / 60_000} minutes")
            return
        }
        val delay = minOf(1000L shl attempts.coerceAtMost(4), MAX_DELAY_MS)
        attempts++
        val why = error?.let { " ($it)" } ?: ""
        publish(Snapshot(true, "Looking for the computer — trying again in ${delay / 1000} s$why", Phase.SEARCHING))
        main.postDelayed(retry, delay)
    }

    private val sessionListener = object : StreamSession.Listener {
        override fun onStatus(session: StreamSession, text: String) {
            main.post { if (this@StreamService.session === session) publish(current.copy(running = true, status = text)) }
        }

        override fun onLive(session: StreamSession, width: Int, height: Int, fps: Int) {
            main.post {
                if (this@StreamService.session !== session) return@post
                val lines = minOf(width, height)
                val size = if (lines >= 2160) "4K" else "${lines}p"
                publish(current.copy(running = true, phase = Phase.LIVE, live = "$size · $fps"))
            }
        }

        override fun onStopped(session: StreamSession, error: String?, retry: Boolean) {
            main.post {
                if (this@StreamService.session !== session) return@post
                this@StreamService.session = null
                if (retry && request != null) {
                    scheduleRetry(error)
                } else {
                    request = null
                    shutdown(if (error == null) "Stopped" else "Stopped: $error")
                }
            }
        }
    }

    private fun shutdown(status: String?) {
        main.removeCallbacks(retry)
        releaseLocks()
        if (Build.VERSION.SDK_INT >= 24) {
            stopForeground(STOP_FOREGROUND_REMOVE)
        } else {
            @Suppress("DEPRECATION")
            stopForeground(true)
        }
        publish(Snapshot(false, status ?: current.status))
        stopSelf()
    }

    override fun onDestroy() {
        main.removeCallbacks(retry)
        request = null
        session?.stop()
        session = null
        releaseLocks()
        super.onDestroy()
    }

    // ------------------------------------------------------------------ foreground + locks

    private fun startInForeground(request: Request) {
        val cfg = request.config
        val manager = getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        if (Build.VERSION.SDK_INT >= 26) {
            manager.createNotificationChannel(
                NotificationChannel(CHANNEL_ID, "Streaming", NotificationManager.IMPORTANCE_LOW).apply {
                    description = "Shown while the phone is used as a webcam"
                },
            )
        }
        val immutable = if (Build.VERSION.SDK_INT >= 23) PendingIntent.FLAG_IMMUTABLE else 0
        val openApp = PendingIntent.getActivity(
            this, 0, Intent(this, MainActivity::class.java), immutable or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val stopIntent = PendingIntent.getService(
            this, 1, Intent(this, StreamService::class.java).setAction(ACTION_STOP),
            immutable or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val builder = if (Build.VERSION.SDK_INT >= 26) {
            Notification.Builder(this, CHANNEL_ID)
        } else {
            @Suppress("DEPRECATION")
            Notification.Builder(this)
        }
        @Suppress("DEPRECATION") // addAction(int, …) is the only variant on API 21–22
        val notification = builder
            .setSmallIcon(android.R.drawable.presence_video_online)
            .setContentTitle("Oculibre is streaming")
            .setContentText("${cfg.width}×${cfg.height} · ${cfg.fps} fps → ${request.host}")
            .setOngoing(true)
            .setContentIntent(openApp)
            .addAction(0, "Stop", stopIntent)
            .build()

        if (Build.VERSION.SDK_INT >= 29) {
            startForeground(NOTIFICATION_ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_CAMERA)
        } else {
            startForeground(NOTIFICATION_ID, notification)
        }
    }

    private fun acquireLocks() {
        val power = getSystemService(Context.POWER_SERVICE) as PowerManager
        wakeLock = power.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "oculibre:stream").apply {
            setReferenceCounted(false)
            acquire()
        }
        val wifi = applicationContext.getSystemService(Context.WIFI_SERVICE) as WifiManager
        val mode = if (Build.VERSION.SDK_INT >= 29) {
            WifiManager.WIFI_MODE_FULL_LOW_LATENCY
        } else {
            @Suppress("DEPRECATION")
            WifiManager.WIFI_MODE_FULL_HIGH_PERF
        }
        wifiLock = wifi.createWifiLock(mode, "oculibre:stream").apply {
            setReferenceCounted(false)
            acquire()
        }
    }

    private fun releaseLocks() {
        wakeLock?.let { if (it.isHeld) it.release() }
        wakeLock = null
        wifiLock?.let { if (it.isHeld) it.release() }
        wifiLock = null
    }

    companion object {
        private const val ACTION_STOP = "org.oculibre.action.STOP"
        private const val CHANNEL_ID = "streaming"
        private const val NOTIFICATION_ID = 1
        /** A session that lasted this long was streaming: the next loss starts the count again. */
        private const val LASTED_MS = 10_000L
        private const val MAX_DELAY_MS = 10_000L
        /** Then the phone stops looking, so as not to drain the battery with the computer off. */
        private const val GIVE_UP_MS = 10 * 60_000L

        private val main = Handler(Looper.getMainLooper())
        private val observers = CopyOnWriteArraySet<(Snapshot) -> Unit>()

        @Volatile
        private var pending: Request? = null

        @Volatile
        var current = Snapshot(running = false, status = null)
            private set

        fun start(context: Context, host: String, port: Int, config: StreamConfig) {
            pending = Request(host, port, config)
            publish(Snapshot(true, "Starting…"))
            val intent = Intent(context, StreamService::class.java)
            if (Build.VERSION.SDK_INT >= 26) context.startForegroundService(intent) else context.startService(intent)
        }

        fun stop(context: Context) {
            context.startService(Intent(context, StreamService::class.java).setAction(ACTION_STOP))
        }

        /** Registers [observer] (called on the main thread) and immediately gives it the current state. */
        fun observe(observer: (Snapshot) -> Unit) {
            observers += observer
            observer(current)
        }

        fun removeObserver(observer: (Snapshot) -> Unit) {
            observers -= observer
        }

        private fun publish(snapshot: Snapshot) {
            current = snapshot
            main.post { observers.forEach { it(snapshot) } }
        }
    }
}
