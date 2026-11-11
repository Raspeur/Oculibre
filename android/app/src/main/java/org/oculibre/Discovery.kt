package org.oculibre

import android.content.Context
import android.net.nsd.NsdManager
import android.net.nsd.NsdServiceInfo
import android.os.Build
import java.net.Inet4Address
import java.net.InetAddress
import java.util.concurrent.CountDownLatch
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.TimeUnit

/**
 * Finds the paired computer on the local network (mDNS / DNS-SD `_oculibre-host._tcp`, protocol
 * §2), for when its address changed. The announcement's TXT `k` is the start of the computer's
 * certificate fingerprint: it only picks the right computer; the TLS handshake still checks the
 * whole fingerprint.
 */
object Discovery {
    const val SERVICE = "_oculibre-host._tcp"
    /** Shortest `k` accepted (64 bits of the fingerprint). */
    private const val MIN_KEY = 16

    /** Does an announcement with TXT `k` = [key] come from the computer pinned as [fingerprint]? */
    fun matches(key: String?, fingerprint: String): Boolean {
        val k = key?.trim()?.lowercase() ?: return false
        return k.length >= MIN_KEY && fingerprint.lowercase().startsWith(k)
    }

    /**
     * Looks for the computer whose fingerprint is [fingerprint] for at most [timeoutMs]; returns its
     * IPv4 address, or null. Blocks: call it off the main thread.
     */
    fun find(context: Context, fingerprint: String, timeoutMs: Long): String? {
        val nsd = context.getSystemService(Context.NSD_SERVICE) as NsdManager
        val found = LinkedBlockingQueue<NsdServiceInfo>()
        val discovery = object : NsdManager.DiscoveryListener {
            override fun onServiceFound(info: NsdServiceInfo) {
                found.offer(info)
            }

            override fun onServiceLost(info: NsdServiceInfo) {}
            override fun onDiscoveryStarted(serviceType: String) {}
            override fun onDiscoveryStopped(serviceType: String) {}
            override fun onStartDiscoveryFailed(serviceType: String, errorCode: Int) {}
            override fun onStopDiscoveryFailed(serviceType: String, errorCode: Int) {}
        }
        val deadline = System.currentTimeMillis() + timeoutMs
        try {
            nsd.discoverServices(SERVICE, NsdManager.PROTOCOL_DNS_SD, discovery)
        } catch (e: Exception) {
            return null
        }
        try {
            while (true) {
                val left = deadline - System.currentTimeMillis()
                if (left <= 0) return null
                val info = found.poll(left, TimeUnit.MILLISECONDS) ?: return null
                // One resolution at a time (older Android versions refuse parallel ones).
                val resolved = resolve(nsd, info, deadline - System.currentTimeMillis()) ?: continue
                val key = resolved.attributes["k"]?.let { String(it, Charsets.UTF_8) }
                if (matches(key, fingerprint)) return ipv4(resolved) ?: continue
            }
        } finally {
            runCatching { nsd.stopServiceDiscovery(discovery) }
        }
    }

    @Suppress("DEPRECATION") // resolveService: its replacement needs API 34
    private fun resolve(nsd: NsdManager, info: NsdServiceInfo, timeoutMs: Long): NsdServiceInfo? {
        if (timeoutMs <= 0) return null
        val done = CountDownLatch(1)
        var result: NsdServiceInfo? = null
        nsd.resolveService(info, object : NsdManager.ResolveListener {
            override fun onServiceResolved(resolved: NsdServiceInfo) {
                result = resolved
                done.countDown()
            }

            override fun onResolveFailed(info: NsdServiceInfo, errorCode: Int) {
                done.countDown()
            }
        })
        done.await(timeoutMs, TimeUnit.MILLISECONDS)
        return result
    }

    @Suppress("DEPRECATION") // getHost: hostAddresses needs API 34
    private fun ipv4(info: NsdServiceInfo): String? {
        val addresses: List<InetAddress> = if (Build.VERSION.SDK_INT >= 34) {
            info.hostAddresses
        } else {
            listOfNotNull(info.host)
        }
        // Direct (on this phone's network) first, then a home or office one, a VPN's last.
        val subnets = Networks.local()
        return addresses.filterIsInstance<Inet4Address>()
            .mapNotNull { it.hostAddress?.let { ip -> ip to it.isSiteLocalAddress } }
            .sortedWith(compareBy({ Networks.route(it.first, subnets).ordinal }, { !it.second }))
            .firstOrNull()?.first
    }
}
