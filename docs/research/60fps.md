# 60 fps on Android: what third-party apps can (and can't) get

> Status: research notes, November 2026. Based on two real devices and public sources (listed at the end).
> Question: *the stock camera app records 1080p60, why can't Oculibre?*

## TL;DR

- **Solved for our two phones (§7):** simply requesting an **unlisted `[60,60]` AE range in a normal session** gives a real 60 fps on both the Redmi Note 9 Pro (60.5 fps) and the Galaxy S23 Ultra (59.8 fps), with normal auto-exposure. No vendor key is needed.
- Neither phone *lists* 60 fps in the standard Camera2 metadata (`aeAvailableTargetFpsRanges` stops at 30). The vendor tables (§6) are what reveal that the HAL supports it. Oculibre uses them as a hint, and **measures the real camera rate while streaming**.
- The **constrained high-speed (slow-motion) mode** (capture 120 fps, keep 1 in 2) stays as a fallback. It is much darker (exposure ≤ 1/120 s, ISO often capped): usable on the S23, unusable indoors on the Redmi.
- Manual exposure (AE off + 16.7 ms frame duration) does **not** work on the Redmi (stays at 30 fps), as reported elsewhere.

## 1. Reading `adb shell dumpsys media.camera`

The dump prints the static metadata (`CameraCharacteristics`) of every camera the HAL knows, **including cameras hidden from apps** (see the Redmi notes in §2). Arrays are printed four values per line, so tuples spill across lines: always count from the `int32[N]` size.

### `android.control.aeAvailableTargetFpsRanges`

A flat list of `[min, max]` pairs. Each pair is a valid value for `CONTROL_AE_TARGET_FPS_RANGE` in a **normal** capture session.
- `min == max` → fixed frame rate (what you want for streaming).
- `min < max` → variable: the auto-exposure may slow down in low light.

```
int32[10]
[15 15 7 24 ]      →  [15,15] [7,24]
[24 24 8 30 ]      →  [24,24] [8,30]
[30 30 ]           →  [30,30]
```

**If no pair reaches 60, a normal session cannot officially run at 60 fps.**

Related constraint: `StreamConfigurationMap.getOutputMinFrameDuration(size)` must be ≤ 16.7 ms for 60 fps at that size.

### `android.control.availableHighSpeedVideoConfigurations`

A flat list of **5-tuples**: `(width, height, fps_min, fps_max, batch_size_max)`. These are only usable in a *constrained high-speed* session (`createConstrainedHighSpeedCaptureSession`).
- For each fixed range `[N, N]` there is a variable `[30, N]`. The variable one is meant for **preview only**, the fixed one for **recording**.
- `batch_size_max`: how many frames each request burst carries (`createHighSpeedRequestList`).
- Constraints of the mode: at most 2 output surfaces (preview + video), limited controls, and exposure ≤ 1/N s.

```
int32[50]
[1920 1080 30 120 ] [4 1920 1080 120 ] [120 2 1920 1080 ] ...
→ (1920,1080, 30,120, 4)  (1920,1080, 120,120, 2)  (1920,1080, 30,240, 8) ...
```

### Other useful keys

| Key | Why it matters |
|---|---|
| `android.info.supportedHardwareLevel` | `LEGACY` / `LIMITED` / `FULL` / `LEVEL_3`. Below `FULL`, manual controls are not guaranteed. |
| `android.request.availableCapabilities` | Must contain `CONSTRAINED_HIGH_SPEED_VIDEO` (= 9) for the high-speed path. `MANUAL_SENSOR` (= 1) is needed for manual exposure / frame duration. |
| `android.request.availableSessionKeys` | Keys that must be set when the session is created, not per request (often includes the fps range). |
| `samsung.*`, `com.xiaomi.*`, `org.codeaurora.*`… | Vendor tags. Sometimes reveal capabilities hidden from the standard keys. |

## 2. Our two devices

### Redmi Note 9 Pro (Snapdragon 720G, MIUI)

| | Back cameras | Other cameras |
|---|---|---|
| Normal AE ranges | `[15,15] [7,24] [24,24] [8,30] [30,30]`: **max 30** | `[15,15] [8,30] [30,30]`: **max 30** |
| High-speed | 1080p: 120 (batch 4) · 720p: 120 (batch 4), 240 (batch 8) · 720×480: 120, 240 · 640×480: 120 | 720p, 720×480, 640×480: 120 |

8 camera entries in the dump. Apps see fewer: Qualcomm/Xiaomi builds hide auxiliary cameras from packages that are not on an allowlist (`vendor.camera.aux.packagelist`).

**Result in Oculibre:** 1080p60 via high-speed → **very dark indoors**. Only the light bulb was visible.

### Samsung Galaxy S23 Ultra (One UI)

| | Main sensor | Other cameras |
|---|---|---|
| Normal AE ranges | `[10,10] [7,11] [11,11] [7,15] [15,15] [24,24] [7,30] [30,30]`: **max 30** | same, **max 30** |
| High-speed | 1080p: 120 (batch 2), 240 (batch 4) · 1920×824: 120 · 720p: 120, 240 | 1080p, 1920×824, 720p: 120 |
| Vendor tag | `samsung.android.scaler.availableHighSpeedVideoConfigurations`: same sizes, 120 and 240 | |

**Result in Oculibre:** 1080p60 via high-speed → **good picture**. The larger sensor and Samsung's tuning (higher ISO allowed) make up for the 1/120 s cap.

### Conclusion on the data

Neither vendor lists 60 fps for normal sessions, **even Samsung**. We wrongly assumed Samsung would. The "60 fps" that works on the S23 is the slow-motion path.

## 3. Why the stock app can do 60 fps and we can't

- The stock camera talks to the HAL through **vendor-private extensions** (vendor tags, private session modes, sometimes a privileged system API). Manufacturers reserve high frame rates, some lenses (telephoto) and some processing (HDR, stabilisation) for their own app or for selected partners.
- Samsung confirms this indirectly. Its *Galaxy Camera Ecosystem* gives selected partners a **Samsung Camera SDK** (HDR, Super Night, VDIS…), but frame rates are not part of it, and the SDK is not open to everyone. Requests from developers on Samsung's forum to unlock 60 fps for third-party apps have gone unanswered for years.
- Some devices, notably some Google Pixels, do let third-party apps record at 60 fps: Open Camera gained 4K60 on the Pixel 4, and Blackmagic Camera offers 60 fps only on "selected handsets". We have not checked a Pixel dump ourselves.
- Even with the normal 60 fps range, there is a documented caveat: manual exposure (AE off) has been seen to cap at 30 fps on some devices.

**For Oculibre: on most non-Pixel phones, the public API simply does not offer 60 fps with normal exposure.** No app logic fixes that without root.

## 4. Why high-speed mode is darker

At a fixed 120 fps, each frame can be exposed for **at most 8.3 ms** (1/120 s). That is one stop less light than a real 60 fps (16.7 ms max). On top of that, vendors often:
- cap ISO / analog gain in high-speed mode (the mode is tuned for slow-motion in daylight);
- ignore or limit exposure compensation.

Dropping 1 frame in 2 throws away half of the captured light. The S23 compensates with sensor size and gain. The Redmi cannot.

## 5. Options for Oculibre

| Option | Picture | Effort | Verdict |
|---|---|---|---|
| **A. Normal 60 fps** when listed (some Pixels and others) | Normal | Done | ✅ Keep: automatic |
| **B. High-speed 120 → 60, drop 1 in 2** (current) | Dark on mid-range | Done | ✅ Keep, **labelled honestly** ("slow-motion mode, needs light") |
| **C. High-speed 120 → 60, *sum* each pair of frames** (GPU, between camera and encoder) | ≈ +1 stop of light vs B: recovers the light B throws away | Medium (OpenGL ES pipeline) | 🔬 Worth prototyping |
| **D. Try an *unlisted* `[60,60]` range or manual frame duration (16.7 ms) in a normal session** | Normal, if the HAL accepts | Small (a probe) | 🔬 Test: undefined behaviour, may be ignored or error |
| **E. Vendor tags** (e.g. `samsung.android.control.*`), read via `CameraCharacteristics.Key(name, type)` (API 29+) | Normal, if a hidden 60 fps exists | Small to probe, fragile | 🔬 Inspect the dumps first |
| **F. Root / build.prop tweaks** | — | — | ❌ Out of scope for a public app |
| **G. Android 14+ native USB webcam mode** | Vendor-defined | None | ➖ Suggest it when available (already in the roadmap) |

## 6. Findings in vendor tags (November 2026)

The standard keys say "30 fps max" on both phones, but the **vendor tags say otherwise**.

### Redmi Note 9 Pro: Qualcomm tables list 1080p60

On the back cameras, the Qualcomm (CamX) HAL publishes two tables:

| Tag | Layout | Excerpt |
|---|---|---|
| `org.quic.camera2.customhfrfps.info.CustomHFRFpsTable` | `(width, height, fps)` | `2304×1728@60`, `2160×1080@60`, `1920×1440@60`, **`1920×1080@60`**, `1600×1200@60`, `1560×720@60`, `1440×1080@60`… |
| `org.quic.camera2.streamBasedFPS.info.StreamBasedFPSTable` | `(width, height, fps_min, fps_max)` | `2304×1728 [30,60] [60,60]`, `2160×1080 [30,60] [60,60]`, `1920×1440 [30,60] [60,60]`… |

So **the sensor pipeline supports real 60 fps** (normal session, normal exposure). Xiaomi filters it out of
`aeAvailableTargetFpsRanges`. Related Qualcomm keys: `org.quic.camera.manualExposure.disableFPSLimits`
(request), `org.codeaurora.qcamera3.sessionParameters.dynamicFPSConfig` (session parameter).

### Galaxy S23 Ultra: Samsung "effect" range and recording session keys

| Tag | Value | Meaning (inferred) |
|---|---|---|
| `samsung.android.control.effectAeAvailableTargetFpsRanges` | `[10,30] [24,60]` on **every** camera | AE ranges allowed when Samsung effects / video modes are active: up to 60 fps |
| `samsung.android.request.availableSessionKeys` | includes `samsung.android.control.recordingMaxFps`, `recordingMinFps`, `shootingMode`, `recordingExtraMode`, `cameraClient`, `samsung.android.unihal.videoMode`… | Samsung's own app very likely selects 60 fps through these **session parameters** |
| `samsung.android.control.availableHalFrcFps` | `240` | HAL frame-rate conversion for slow-motion |

Vendor keys can be used from an app: `CaptureRequest.Key(name, type)` is public since API 29, and session parameters
are set with `SessionConfiguration.setSessionParameters()` (API 28). Whether the HAL honours them for a
third-party package is unknown: Samsung may check `cameraClient` / the caller.

## 7. Lab results (Oculibre lab screen, back camera, 1080p, real H.264 encoder as target)

Measured from `SENSOR_TIMESTAMP` over 2 s after a 1.5 s AE warm-up, in the same dim room. Full logs in each
device's `report.md`.

| Strategy | Redmi Note 9 Pro (Android 12) | Galaxy S23 Ultra (Android 17) |
|---|---|---|
| S0 reference `[30,30]` | 30.0 fps · 16.7 ms · ISO 1599 | 30.0 fps · 30.0 ms · ISO 34857 |
| **S1 `[60,60]` per request** | **60.5 fps** · 8.3 ms · ISO 3224 | **59.8 fps** · 16.7 ms · ISO 31364 |
| S1b `[30,60]` per request | 60.5 fps · 8.3 ms · ISO 3221 | 33.3 fps (AE slows down in low light) |
| S2 `[60,60]` as session parameter | 60.5 fps | 59.8 fps |
| S3a Samsung `recordingMin/MaxFps` 24–60 + `[24,60]` | key not available | 25.0 fps (variable, low light) |
| S3b Samsung `recordingMin/MaxFps` 60–60 + `[60,60]` | key not available | **session error** (`Broken pipe`), camera closed |
| S4a/b Qualcomm `disableFPSLimits` + `[60,60]` | 60.5 fps (no difference vs S1) | not run (camera closed by S3b) |
| S5 manual, AE off, 16.7 ms | **30.0 fps** (ignored) | not run |

Takeaways:
- **S1 is the strategy**: a fixed `[60,60]` per request. Variable ranges (`[30,60]`, `[24,60]`) let the AE drop the frame rate in low light.
- Samsung's private session keys are not needed, and `recordingMinFps = 60` breaks session configuration.
- On the Redmi, the AE caps exposure at 8.3 ms at 60 fps and doubles the ISO: brighter than slow-motion mode, noisier than 30 fps.
- The lab now reopens the camera for each strategy, so one crash no longer stops the run.

## Next experiments

1. ~~Vendor tags on the S23~~: done, see §6.
2. **60 fps probe (in the app, "Lab" screen)**: done, results in §7. Original plan: Open a normal session at 1080p and try, one strategy at a time,
   measuring the **real** frame rate from `SENSOR_TIMESTAMP` deltas over 2 s, plus `SENSOR_EXPOSURE_TIME` / `SENSOR_SENSITIVITY`:
   - S1: AE target range `[60,60]` per request (unlisted);
   - S2: same range as a **session parameter** (`setSessionParameters`);
   - S3 (Samsung): session parameters `samsung.android.control.recordingMaxFps = 60` (+ `recordingMinFps`), with `[24,60]` then `[60,60]`;
   - S4 (Qualcomm): S1/S2 + `org.quic.camera.manualExposure.disableFPSLimits = 1`;
   - S5: AE off, `SENSOR_FRAME_DURATION = 16_666_666` ns (needs `MANUAL_SENSOR`).

   Record every result (device, strategy, measured fps, exposure, errors) in `devices/<brand>/<model>/report.md`.
3. **Measure the darkness instead of guessing.** Read `SENSOR_EXPOSURE_TIME` and `SENSOR_SENSITIVITY` from `CaptureResult` in high-speed mode. If they sit at their limits, warn the user ("too dark for 60 fps, use 30") instead of streaming a black picture.
4. **Option C prototype** if no strategy of experiment 2 works: camera → `SurfaceTexture` → GL shader summing 2 frames → encoder input surface.

## Sources

- AOSP camera metadata, tuple layout of `availableHighSpeedVideoConfigurations` (width, height, fps_min, fps_max, batch_size_max): [hardware/interfaces camera metadata types.hal](https://android.googlesource.com/platform/hardware/interfaces/+/android17-release/camera/metadata/3.2/types.hal)
- `StreamConfigurationMap` javadoc, fixed vs `[30, max]` ranges, preview vs recording: [StreamConfigurationMap.java (Android 9)](https://code.yawk.at/android/android-9.0.0_r35/android/hardware/camera2/params/StreamConfigurationMap.java)
- 60 fps in a normal session, min frame duration, manual-exposure caveat: [Stack Overflow answer (archive)](https://kiwix.lema.org/stackoverflow.com_en_all_2023-11/a/43646443)
- Third-party apps stuck at 30 fps, dump showing only 120/240 high-speed modes: [Fairphone forum](https://forum.fairphone.com/t/why-does-the-camera-api-only-expose-up-to-30-fps/128891)
- Samsung developers asking to unlock 1080p60 for third-party apps, framed as a policy question: [Samsung Developer Forum](https://forum.developer.samsung.com/t/allow-1080p-60fps-in-any-3rd-party-camera-apps/10653), [S22 Ultra thread](https://forum.developer.samsung.com/t/galaxy-s22-ultra-maximum-video-resolution-and-fps-from-camera-api/21218/2)
- Samsung Galaxy Camera Ecosystem / Camera SDK for selected vendors (no frame-rate feature listed): [Samsung Community](https://r2.community.samsung.com/t5/CamCyclopedia/Samsung-Galaxy-Camera-Ecosystem/ba-p/13958899)
- Open Camera device notes (manufacturers may restrict high frame rates to the stock app): [opencamera.org.uk/devices](https://opencamera.org.uk/devices.html)
- Open Camera brings 4K60 to the Pixel 4: [Android Central](https://androidcentral.com/open-camera-app-brings-4k60-video-recording-pixel-4)
- Blackmagic Camera frame rates on selected handsets / Pixels: [9to5Google](https://9to5google.com/2024/09/13/blackmagic-camera-1-3-adds-more-recording-framerates-on-some-pixels-and-more/)
- CameraX 1.5 high-speed API, support depends on manufacturer-populated `CamcorderProfile`: [Android Developers Blog](https://android-developers.googleblog.com/2025/10/high-speed-capture-and-slow-motion.html)
- Auxiliary cameras hidden behind a package allowlist (`vendor.camera.aux.packagelist`): [Xiaomi device tree commit](https://git.evolution-x.org/Evolution-X-Devices/device_xiaomi_stone/commit/890f65768a18999534b8e62fae4b004f48698e5c)
