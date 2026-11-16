package org.oculibre

/**
 * What can go wrong, as the phone tells it to people (design/mockups/PhoneProblems): a code
 * shared with the computer's catalogue (design/mockups/Problems) when it is the same problem,
 * a level for the look, and whether it blocks Start (nothing can work until it is fixed).
 * Titles and texts are string resources `problem_<code>_title` / `problem_<code>_text`.
 *
 * Codes: 1xx connection, 2xx pairing and security, 3xx the phone's camera.
 * Pure Kotlin (no Android class): unit-tested on the JVM.
 */
enum class Problem(val code: Int, val level: Level, val blocking: Boolean = false) {
    /** The computer was lost: looking for it again (first minute). */
    SEARCHING(101, Level.PROGRESS),
    /** Still not found after a minute, or the phone gave up looking. */
    NOT_FOUND(102, Level.WARNING),
    /** The computer and the phone speak incompatible protocol versions. */
    UPDATE_NEEDED(105, Level.ERROR),
    /** The computer already receives another phone. */
    COMPUTER_BUSY(106, Level.WARNING),
    /** The computer refused this phone, or is not the paired one (one of them reinstalled). */
    NOT_RECOGNISED(203, Level.ERROR),
    /** Another app holds the camera. */
    CAMERA_BUSY(301, Level.ERROR),
    /** The phone is overheating while live. */
    HOT(304, Level.WARNING),
    /** Android withdrew the camera permission. */
    CAMERA_PERMISSION(305, Level.ERROR, blocking = true),
    /** The camera or the encoder failed for another reason. */
    CAMERA_ERROR(306, Level.ERROR),
    /** Anything else (protocol error…): the technical detail goes with it. */
    UNEXPECTED(199, Level.ERROR),
    ;

    enum class Level { PROGRESS, INFO, WARNING, ERROR }

    companion object {
        /** How long [SEARCHING] lasts before it becomes [NOT_FOUND]. */
        const val SEARCHING_MS = 60_000L

        fun ofCode(code: Int): Problem? = values().firstOrNull { it.code == code }

        /**
         * The computer's BYE reason (protocol §7.4) → what to tell people; null when it is not a
         * problem (the computer stopped the stream on purpose).
         */
        fun fromBye(reason: String): Problem? = when (reason) {
            "user_stop", "shutdown" -> null
            "unpaired" -> NOT_RECOGNISED
            "version" -> UPDATE_NEEDED
            "busy" -> COMPUTER_BUSY
            else -> UNEXPECTED
        }

        /** CameraDevice.StateCallback.onError's number → the problem. */
        fun fromCameraError(error: Int): Problem = when (error) {
            1, 2 -> CAMERA_BUSY // ERROR_CAMERA_IN_USE, ERROR_MAX_CAMERAS_IN_USE
            else -> CAMERA_ERROR // disabled by policy, device or service error
        }

        /** CameraAccessException.getReason() → the problem (other numbers than above). */
        fun fromCameraAccess(reason: Int): Problem = when (reason) {
            4, 5 -> CAMERA_BUSY // CAMERA_IN_USE, MAX_CAMERAS_IN_USE
            else -> CAMERA_ERROR
        }
    }
}
