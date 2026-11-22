package org.oculibre

/**
 * Adaptive bitrate: lowers the encoder's target when the network cannot carry it, and raises it
 * back slowly when it can (protocol §flow). Pure Kotlin, no Android class: JVM unit tests.
 *
 * Signals, given for every encoded frame by the drain thread:
 * - `dropped`: the send queue was full, the frame was dropped (the drop policy starts);
 * - `queueHigh`: the queue is at least half full. Held for [HIGH_FOR_MS] it means the network is
 *   at its limit even before anything is dropped.
 *
 * Lowering is fast (×0.7 on a drop, ×0.85 on a queue that stays high, at most every
 * [LOWER_EVERY_MS]: the encoder needs a few frames to follow). Raising waits [CALM_MS] without
 * trouble, then +10 % every [RAISE_EVERY_MS]; near the level that was too much last time, only
 * every [PROBE_EVERY_MS], so a network at its limit is not hit again and again.
 */
class BitrateControl(ceiling: Int) {
    /** The mode's bitrate (or the one the computer asked for): never above. */
    var ceiling: Int = ceiling
        private set

    /** What the encoder should aim at now. */
    var target: Int = ceiling
        private set

    private var floor = floorOf(ceiling)
    private var lastLower = Long.MIN_VALUE / 2
    private var lastRaise = Long.MIN_VALUE / 2
    private var lastTrouble = Long.MIN_VALUE / 2
    private var highSince = -1L
    /** The target at the last drop: raising stays careful near it. */
    private var tooMuch = Int.MAX_VALUE

    /** A new mode or a bitrate asked by the computer: start again from it. */
    @Synchronized
    fun reset(ceiling: Int) {
        this.ceiling = ceiling
        target = ceiling
        floor = floorOf(ceiling)
        lastLower = Long.MIN_VALUE / 2
        lastRaise = Long.MIN_VALUE / 2
        lastTrouble = Long.MIN_VALUE / 2
        highSince = -1L
        tooMuch = Int.MAX_VALUE
    }

    /** One encoded frame. Returns the new target when it changed (give it to the encoder), else null. */
    @Synchronized
    fun observe(nowMs: Long, dropped: Boolean, queueHigh: Boolean): Int? {
        if (queueHigh) {
            if (highSince < 0) highSince = nowMs
        } else {
            highSince = -1L
        }
        val stuck = highSince >= 0 && nowMs - highSince >= HIGH_FOR_MS
        if (dropped || stuck) {
            lastTrouble = nowMs
            if (nowMs - lastLower < LOWER_EVERY_MS) return null
            if (dropped) tooMuch = target
            return set((target * if (dropped) DROP_FACTOR else HIGH_FACTOR).toInt(), nowMs, lower = true)
        }
        if (target >= ceiling || nowMs - lastTrouble < CALM_MS) return null
        val next = maxOf((target * RAISE_FACTOR).toInt(), target + MIN_STEP)
        val every = if (next > tooMuch * NEAR) PROBE_EVERY_MS else RAISE_EVERY_MS
        if (nowMs - lastRaise < every || nowMs - lastLower < every) return null
        return set(next, nowMs, lower = false)
    }

    /** The target is below the ceiling: the network is the limit. */
    val lowered: Boolean
        @Synchronized get() = target < ceiling

    private fun set(value: Int, nowMs: Long, lower: Boolean): Int? {
        val clamped = value.coerceIn(floor, ceiling)
        if (lower) lastLower = nowMs else lastRaise = nowMs
        if (lower) highSince = -1L // give the encoder time before judging the queue again
        if (clamped == target) return null
        target = clamped
        return clamped
    }

    companion object {
        const val HIGH_FOR_MS = 500L
        const val LOWER_EVERY_MS = 1000L
        const val CALM_MS = 4000L
        const val RAISE_EVERY_MS = 1000L
        const val PROBE_EVERY_MS = 10_000L
        private const val DROP_FACTOR = 0.7
        private const val HIGH_FACTOR = 0.85
        private const val RAISE_FACTOR = 1.1
        /** Raising above this share of the level that was too much is a probe. */
        private const val NEAR = 0.9
        private const val MIN_STEP = 100_000
        /** Never below 500 kbit/s nor an eighth of the mode's bitrate: blurrier than that is useless. */
        private fun floorOf(ceiling: Int) = minOf(ceiling, maxOf(500_000, ceiling / 8))
    }
}
