package org.oculibre

import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.hardware.usb.UsbAccessory
import android.hardware.usb.UsbManager
import android.os.Build
import android.os.ParcelFileDescriptor
import java.io.FileInputStream
import java.io.FileOutputStream
import java.io.IOException
import java.net.InetAddress
import java.net.InetSocketAddress
import java.net.ServerSocket
import java.net.Socket
import kotlin.concurrent.thread

/**
 * USB without developer mode (#21): the computer switched this phone to Android Open Accessory
 * mode (desktop/core/src/aoa.rs) and the user accepted "Open Oculibre". The protocol then runs
 * over the accessory's two USB bulk endpoints, in plaintext like the cable through adb (§13).
 *
 * The session wants a [Socket] (read timeouts, send buffer, close that wakes blocked reads), which
 * the accessory's file streams are not. So [open] relays the accessory to a loopback socket pair
 * made for the session alone: the listening socket accepts only the session's own connection,
 * then closes.
 */
object UsbAccessoryLink {
    /** What the computer sends when switching (res/xml/accessory_filter.xml). */
    private const val MANUFACTURER = "Oculibre"
    private const val MODEL = "Oculibre"

    /** Android's accessory driver moves at most 16 KiB per request: never read less. */
    private const val BUFFER = 16 * 1024
    private const val LOOPBACK = "127.0.0.1"
    private const val ACCEPT_TIMEOUT_MS = 2000

    /** Broadcast (to this app only) with the answer to [askPermission]. */
    const val ACTION_PERMISSION = "org.oculibre.action.USB_PERMISSION"

    /** Asked once per plug: the prompt is showing, or the user said no. */
    @Volatile private var asked = false

    /** The computer's accessory, plugged in, allowed or not; null without one. */
    fun find(context: Context): UsbAccessory? {
        val usb = context.getSystemService(Context.USB_SERVICE) as? UsbManager ?: return null
        val found = usb.accessoryList?.firstOrNull { it.manufacturer == MANUFACTURER && it.model == MODEL }
        if (found == null) asked = false // unplugged: the next plug may ask again
        return found
    }

    /** The user allowed the app to use it ("Open Oculibre", or [askPermission]). */
    fun allowed(context: Context, accessory: UsbAccessory): Boolean =
        (context.getSystemService(Context.USB_SERVICE) as UsbManager).hasPermission(accessory)

    /**
     * Shows Android's prompt to allow the app to use the accessory, when "Open Oculibre" was not
     * accepted. The answer comes as [ACTION_PERMISSION] (StreamService tries again on a yes).
     * Once per plug, so that attempts every few seconds don't bring it back after a no.
     */
    fun askPermission(context: Context, accessory: UsbAccessory) {
        if (asked) return
        asked = true
        val usb = context.getSystemService(Context.USB_SERVICE) as UsbManager
        // Mutable: Android adds the answer to it. Explicit (this package): required for that.
        val flags = (if (Build.VERSION.SDK_INT >= 31) PendingIntent.FLAG_MUTABLE else 0) or
            PendingIntent.FLAG_UPDATE_CURRENT
        val answer = Intent(ACTION_PERMISSION).setPackage(context.packageName)
        usb.requestPermission(accessory, PendingIntent.getBroadcast(context, 0, answer, flags))
    }

    /** Opens [accessory]; returns the session's end of the relay. */
    fun open(context: Context, accessory: UsbAccessory): Socket {
        val usb = context.getSystemService(Context.USB_SERVICE) as UsbManager
        // null: still held by the previous session (Android lets it go once its read returns).
        val fd = usb.openAccessory(accessory) ?: throw IOException("The USB accessory cannot be opened")
        try {
            ServerSocket(0, 1, InetAddress.getByName(LOOPBACK)).use { server ->
                server.soTimeout = ACCEPT_TIMEOUT_MS
                val session = Socket()
                session.tcpNoDelay = true
                session.connect(InetSocketAddress(LOOPBACK, server.localPort), ACCEPT_TIMEOUT_MS)
                var relay: Socket
                while (true) {
                    relay = server.accept()
                    if (relay.port == session.localPort) break
                    relay.close() // another app that raced to the port: never let it in
                }
                relay.tcpNoDelay = true
                start(fd, relay)
                return session
            }
        } catch (e: IOException) {
            runCatching { fd.close() }
            throw e
        }
    }

    /** Two threads copy bytes each way; either one ending closes everything. */
    private fun start(fd: ParcelFileDescriptor, relay: Socket) {
        val fromComputer = FileInputStream(fd.fileDescriptor)
        val toComputer = FileOutputStream(fd.fileDescriptor)
        fun end() {
            runCatching { relay.close() }
            runCatching { fd.close() }
        }
        thread(name = "oculibre-usb-in", isDaemon = true) {
            val buf = ByteArray(BUFFER)
            try {
                val out = relay.getOutputStream()
                while (true) {
                    // -1 also when the computer ends the session (a zero-length packet).
                    val n = fromComputer.read(buf)
                    if (n < 0) break
                    if (n > 0) out.write(buf, 0, n)
                }
            } catch (_: IOException) {
            } finally {
                end()
            }
        }
        thread(name = "oculibre-usb-out", isDaemon = true) {
            val buf = ByteArray(BUFFER)
            try {
                val input = relay.getInputStream()
                while (true) {
                    val n = input.read(buf)
                    if (n < 0) break
                    toComputer.write(buf, 0, n)
                }
            } catch (_: IOException) {
            } finally {
                end()
            }
        }
    }
}
