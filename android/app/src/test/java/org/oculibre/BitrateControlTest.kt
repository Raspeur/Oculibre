package org.oculibre

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class BitrateControlTest {
    private val ceiling = 20_000_000

    /** Frames every 16 ms (60 fps) from [from] to [to] ms; the last change, or null. */
    private fun BitrateControl.run(from: Long, to: Long, dropped: Boolean = false, high: Boolean = false): Int? {
        var last: Int? = null
        var t = from
        while (t < to) {
            observe(t, dropped, high)?.let { last = it }
            t += 16
        }
        return last
    }

    @Test
    fun aFreeNetworkKeepsTheModesBitrate() {
        val c = BitrateControl(ceiling)
        assertNull(c.run(0, 30_000))
        assertEquals(ceiling, c.target)
        assertFalse(c.lowered)
    }

    @Test
    fun aDropLowersAtOnceThenAtMostOnceASecond() {
        val c = BitrateControl(ceiling)
        assertEquals(14_000_000, c.observe(0, dropped = true, queueHigh = true))
        assertNull(c.observe(500, dropped = true, queueHigh = true))
        assertEquals(9_800_000, c.observe(1000, dropped = true, queueHigh = true))
        assertTrue(c.lowered)
    }

    @Test
    fun neverBelowTheFloor() {
        val c = BitrateControl(ceiling)
        c.run(0, 60_000, dropped = true)
        assertEquals(ceiling / 8, c.target)
        val small = BitrateControl(2_000_000)
        small.run(0, 60_000, dropped = true)
        assertEquals(500_000, small.target)
    }

    @Test
    fun aQueueThatStaysHighLowersGently() {
        val c = BitrateControl(ceiling)
        assertNull(c.run(0, 400, high = true)) // a keyframe passing: not yet
        c.observe(400, dropped = false, queueHigh = false)
        assertNull(c.run(416, 800, high = true))
        assertEquals(17_000_000, c.run(800, 1400, high = true))
    }

    @Test
    fun raisesBackAfterCalmCarefullyNearWhatWasTooMuch() {
        val c = BitrateControl(ceiling)
        c.observe(0, dropped = true, queueHigh = true) // 14 M, too much was 20 M
        c.observe(1000, dropped = true, queueHigh = true) // 9.8 M, too much was 14 M
        assertNull(c.run(1016, 1000 + BitrateControl.CALM_MS - 16))
        // Calm: +10 % a second up to 90 % of 14 M…
        c.run(1000 + BitrateControl.CALM_MS, 1000 + BitrateControl.CALM_MS + 3100)
        assertEquals(11_858_000, c.target) // 9.8 → 10.78 → 11.858
        // …then the next step (13.04 M > 12.6 M) waits for a probe.
        val beforeProbe = c.target
        c.run(8200, 14_000)
        assertEquals(beforeProbe, c.target)
        c.run(14_000, 19_000)
        assertTrue(c.target > beforeProbe)
    }

    @Test
    fun resetStartsFromTheNewBitrate() {
        val c = BitrateControl(ceiling)
        c.run(0, 5000, dropped = true)
        c.reset(8_000_000)
        assertEquals(8_000_000, c.target)
        assertFalse(c.lowered)
        assertEquals(5_600_000, c.observe(5016, dropped = true, queueHigh = true))
    }
}
