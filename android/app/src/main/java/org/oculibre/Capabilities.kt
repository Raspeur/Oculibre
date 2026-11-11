package org.oculibre

import android.annotation.TargetApi
import android.content.Context
import android.hardware.camera2.CameraCharacteristics
import android.hardware.camera2.CameraManager
import android.hardware.camera2.CameraMetadata
import android.hardware.camera2.params.StreamConfigurationMap
import android.media.MediaCodec
import android.media.MediaCodecInfo
import android.media.MediaCodecInfo.CodecProfileLevel
import android.media.MediaCodecList
import android.media.MediaFormat
import android.os.Build
import android.util.Range
import android.util.Size

/** A quality preset offered to the user. */
data class QualityMode(
    val id: String,
    val label: String,
    val width: Int,
    val height: Int,
    val fps: Int,
    val bitrate: Int,
    /** H.264 level needed for this size × frame rate. */
    val avcLevel: Int,
)

/**
 * A preset and, if the phone can't do it, why (`missing == null` means supported).
 *
 * [highSpeedFps] > 0: the camera only offers this frame rate through its constrained
 * high-speed (slow-motion) mode — e.g. capture 120 fps, encoder keeps 1 frame in 2 for 60 fps.
 * Exposure is then capped at 1/[highSpeedFps] s: darker, noisier picture indoors.
 */
data class ModeSupport(
    val mode: QualityMode,
    val missing: String?,
    val highSpeedFps: Int = 0,
    /** Normal session at a rate the standard metadata hides, but vendor tables advertise. */
    val unlisted: Boolean = false,
) {
    val supported: Boolean get() = missing == null
    /** Protocol name of the capture path: normal, unlisted or high_speed. */
    val path: String get() = when {
        highSpeedFps > 0 -> "high_speed"
        unlisted -> "unlisted"
        else -> "normal"
    }
    val id: String get() = if (highSpeedFps > 0) "${mode.id}-hs" else mode.id
    val label: String
        get() = if (highSpeedFps > 0) "${mode.label} · slow-motion mode (needs light)" else mode.label
}

/** One camera and the presets it can stream. */
class CameraCapabilities(
    val id: String,
    /** back, front or external. */
    val facing: String,
    /** Degrees clockwise (CameraCharacteristics.SENSOR_ORIENTATION). */
    val sensorOrientation: Int,
    val modes: List<ModeSupport>,
)

class DeviceCapabilities(
    val encoderName: String?,
    /** Highest H.264 Baseline level the encoder advertises (0 if none). */
    val baselineMaxLevel: Int,
    /** Every camera apps can open; the default one (back first) comes first. */
    val cameras: List<CameraCapabilities>,
) {
    /** The camera offered in the app's quality picker. */
    val main: CameraCapabilities get() = cameras.first()

    /** Capabilities catalogue sent to the computer (docs/protocol.typ, Capabilities). */
    fun catalogue(): Map<String, Any?> = linkedMapOf(
        "cameras" to cameras.map { camera ->
            linkedMapOf(
                "id" to camera.id,
                "facing" to camera.facing,
                "sensor_orientation" to camera.sensorOrientation.toLong(),
                "modes" to camera.modes.filter { it.supported }.map { s ->
                    linkedMapOf<String, Any?>(
                        "width" to s.mode.width.toLong(),
                        "height" to s.mode.height.toLong(),
                        "fps" to s.mode.fps.toLong(),
                        "path" to s.path,
                    ).apply { if (s.highSpeedFps > 0) put("capture_fps", s.highSpeedFps.toLong()) }
                },
                "controls" to emptyList<String>(),
            )
        },
        "codecs" to listOf("h264"),
    )

    /**
     * The supported mode a `set_mode` command asks for. [path] null: the first path offered for
     * that size and rate (a normal session before the slow-motion one).
     */
    fun find(cameraId: String, width: Int, height: Int, fps: Int, path: String?): Pair<CameraCapabilities, ModeSupport>? {
        val camera = cameras.firstOrNull { it.id == cameraId } ?: return null
        val mode = camera.modes.firstOrNull {
            it.supported && it.mode.width == width && it.mode.height == height && it.mode.fps == fps &&
                (path == null || it.path == path)
        } ?: return null
        return camera to mode
    }
}

/** Exact `[fps, fps]` AE range if available, else the range ending at `fps` with the highest floor. */
fun fpsRangeFor(ranges: Array<Range<Int>>?, fps: Int): Range<Int>? {
    if (ranges == null) return null
    return ranges.firstOrNull { it.lower == fps && it.upper == fps }
        ?: ranges.filter { it.upper == fps }.maxByOrNull { it.lower }
}

object Capabilities {

    val MODES = listOf(
        QualityMode("720p30", "720p · 30 fps", 1280, 720, 30, 4_000_000, CodecProfileLevel.AVCLevel31),
        QualityMode("1080p30", "1080p · 30 fps", 1920, 1080, 30, 8_000_000, CodecProfileLevel.AVCLevel4),
        QualityMode("720p60", "720p · 60 fps", 1280, 720, 60, 6_000_000, CodecProfileLevel.AVCLevel32),
        QualityMode("1080p60", "1080p · 60 fps", 1920, 1080, 60, 12_000_000, CodecProfileLevel.AVCLevel42),
        QualityMode("2160p30", "4K · 30 fps", 3840, 2160, 30, 25_000_000, CodecProfileLevel.AVCLevel51),
    )

    const val DEFAULT_MODE_ID = "1080p30"

    private class AvcEncoder(
        val name: String,
        val video: MediaCodecInfo.VideoCapabilities,
        val baselineMaxLevel: Int,
        val hardware: Boolean,
    )

    /** Back camera first, else the first camera. */
    fun pickCamera(cameraManager: CameraManager): String {
        val ids = cameraManager.cameraIdList
        if (ids.isEmpty()) throw IllegalStateException("No camera found")
        return ids.firstOrNull { id ->
            cameraManager.getCameraCharacteristics(id)
                .get(CameraCharacteristics.LENS_FACING) == CameraMetadata.LENS_FACING_BACK
        } ?: ids[0]
    }

    fun facingName(lensFacing: Int?): String = when (lensFacing) {
        CameraMetadata.LENS_FACING_FRONT -> "front"
        CameraMetadata.LENS_FACING_BACK -> "back"
        else -> "external"
    }

    /**
     * Asks every camera and the H.264 encoder what they can really do. Takes ~100 ms per camera:
     * call off the UI thread.
     */
    fun probe(context: Context): DeviceCapabilities {
        val cameraManager = context.getSystemService(Context.CAMERA_SERVICE) as CameraManager
        val main = pickCamera(cameraManager)
        val encoder = findAvcEncoder()
        val ids = listOf(main) + cameraManager.cameraIdList.filter { it != main }
        val cameras = ids.mapNotNull { id ->
            // A camera that cannot even describe itself is left out (never the main one).
            runCatching { probeCamera(cameraManager, id, encoder) }
                .getOrElse { if (id == main) throw it else null }
        }
        return DeviceCapabilities(encoder?.name, encoder?.baselineMaxLevel ?: 0, cameras)
    }

    private fun probeCamera(cameraManager: CameraManager, id: String, encoder: AvcEncoder?): CameraCapabilities {
        val chars = cameraManager.getCameraCharacteristics(id)
        val map = chars.get(CameraCharacteristics.SCALER_STREAM_CONFIGURATION_MAP)
        val sizes = map?.getOutputSizes(MediaCodec::class.java)?.toSet() ?: emptySet()
        val fpsRanges = chars.get(CameraCharacteristics.CONTROL_AE_AVAILABLE_TARGET_FPS_RANGES)
        return CameraCapabilities(
            id = id,
            facing = facingName(chars.get(CameraCharacteristics.LENS_FACING)),
            sensorOrientation = chars.get(CameraCharacteristics.SENSOR_ORIENTATION) ?: 0,
            modes = MODES.flatMap { mode -> check(mode, chars, map, sizes, fpsRanges, encoder) },
        )
    }

    private fun check(
        mode: QualityMode,
        chars: CameraCharacteristics,
        map: StreamConfigurationMap?,
        sizes: Set<Size>,
        fpsRanges: Array<Range<Int>>?,
        encoder: AvcEncoder?,
    ): List<ModeSupport> {
        val size = Size(mode.width, mode.height)
        if (map == null || size !in sizes) {
            return listOf(ModeSupport(mode, "camera can't output ${mode.width}×${mode.height}"))
        }
        if (encoder == null) return listOf(ModeSupport(mode, "no H.264 encoder"))
        if (!encoder.video.areSizeAndRateSupported(mode.width, mode.height, mode.fps.toDouble())) {
            return listOf(ModeSupport(mode, "H.264 encoder can't do it"))
        }

        val minFrameNs = map.getOutputMinFrameDuration(MediaCodec::class.java, size)
        val fastEnough = minFrameNs <= 1_000_000_000L / mode.fps + 1_000L
        if (mode.fps <= 30) {
            return listOf(
                if (fastEnough) {
                    ModeSupport(mode, null)
                } else {
                    ModeSupport(mode, "camera limited to ${1_000_000_000L / minFrameNs} fps at this size")
                },
            )
        }

        // High frame rates. Two possible paths, both offered when available:
        // 1. Normal session at [fps, fps]: officially listed, or hidden but advertised in vendor
        //    tables (Qualcomm, Samsung). Verified on Redmi Note 9 Pro and Galaxy S23 Ultra.
        // 2. Slow-motion (constrained high-speed) session, decimated by the encoder: darker.
        val result = mutableListOf<ModeSupport>()
        val listed = fastEnough && fpsRangeFor(fpsRanges, mode.fps) != null
        if (listed || vendorAdvertises(chars, size, mode.fps)) {
            result += ModeSupport(mode, null, unlisted = !listed)
        }
        highSpeedFpsFor(chars, map, size, mode.fps)?.let { result += ModeSupport(mode, null, it) }
        if (result.isEmpty()) {
            result += ModeSupport(mode, "camera doesn't expose ${mode.fps} fps to apps")
        }
        return result
    }

    /**
     * Vendor tables showing the HAL can run [size] at [fps] in a normal session although the
     * standard key hides it. Vendor keys are readable from API 29.
     */
    private fun vendorAdvertises(chars: CameraCharacteristics, size: Size, fps: Int): Boolean {
        if (Build.VERSION.SDK_INT < 29) return false
        val qtiStream = readIntArray(chars, "org.quic.camera2.streamBasedFPS.info.StreamBasedFPSTable")
        if (qtiStream != null && qtiStream.toList().chunked(4).any {
                it.size == 4 && it[0] == size.width && it[1] == size.height && it[3] >= fps
            }
        ) {
            return true
        }
        val qtiHfr = readIntArray(chars, "org.quic.camera2.customhfrfps.info.CustomHFRFpsTable")
        if (qtiHfr != null && qtiHfr.toList().chunked(3).any {
                it.size == 3 && it[0] == size.width && it[1] == size.height && it[2] >= fps
            }
        ) {
            return true
        }
        val samsung = readIntArray(chars, "samsung.android.control.effectAeAvailableTargetFpsRanges")
        return samsung != null && samsung.toList().chunked(2).any { it.size == 2 && it[1] >= fps }
    }

    @TargetApi(29)
    fun readIntArray(chars: CameraCharacteristics, name: String): IntArray? = runCatching {
        chars.get(CameraCharacteristics.Key(name, IntArray::class.java))
    }.getOrNull()

    /** Smallest fixed high-speed rate that is a multiple of [fps] (e.g. 120 for 60), or null. */
    private fun highSpeedFpsFor(
        chars: CameraCharacteristics,
        map: StreamConfigurationMap,
        size: Size,
        fps: Int,
    ): Int? {
        if (Build.VERSION.SDK_INT < 23) return null
        val capabilities = chars.get(CameraCharacteristics.REQUEST_AVAILABLE_CAPABILITIES) ?: return null
        if (!capabilities.contains(CameraMetadata.REQUEST_AVAILABLE_CAPABILITIES_CONSTRAINED_HIGH_SPEED_VIDEO)) {
            return null
        }
        if (map.highSpeedVideoSizes.none { it == size }) return null
        return map.getHighSpeedVideoFpsRangesFor(size)
            .filter { it.lower == it.upper && it.upper > fps && it.upper % fps == 0 }
            .minOfOrNull { it.upper }
    }

    private fun findAvcEncoder(): AvcEncoder? {
        val avc = MediaFormat.MIMETYPE_VIDEO_AVC
        val encoders = MediaCodecList(MediaCodecList.REGULAR_CODECS).codecInfos
            .filter { info -> info.isEncoder && info.supportedTypes.any { it.equals(avc, ignoreCase = true) } }
            .mapNotNull { info ->
                val caps = runCatching { info.getCapabilitiesForType(avc) }.getOrNull()
                    ?: return@mapNotNull null
                val video = caps.videoCapabilities ?: return@mapNotNull null
                val baselineMax = caps.profileLevels
                    .filter { it.profile == CodecProfileLevel.AVCProfileBaseline }
                    .maxOfOrNull { it.level } ?: 0
                AvcEncoder(info.name, video, baselineMax, isHardware(info))
            }
        return encoders.firstOrNull { it.hardware } ?: encoders.firstOrNull()
    }

    private fun isHardware(info: MediaCodecInfo): Boolean {
        if (Build.VERSION.SDK_INT >= 29) return info.isHardwareAccelerated
        val name = info.name.lowercase()
        return !(name.startsWith("omx.google.") || name.startsWith("c2.android.") || name.contains(".sw."))
    }
}
