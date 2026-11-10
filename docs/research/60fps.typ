#import "../oculibre.typ": *

#show: oculibre-doc.with(
  title: [60 fps on Android],
  subtitle: [What third-party apps can (and can't) get from the camera],
  revision: "1.0",
  date: datetime(year: 2026, month: 11, day: 10),
)

#remark(title: [Question])[The stock camera app records 1080p60: why couldn't Oculibre? Research notes based on two
real devices and public sources (listed at the end).]

= Summary <summary>

- *Solved for our two phones (@lab):* simply requesting an *unlisted `[60,60]` AE range in a normal
  session* gives a real 60 fps on both the Redmi Note 9 Pro (60.5 fps) and the Galaxy S23 Ultra
  (59.8 fps), with normal auto-exposure. No vendor key is needed.
- Neither phone _lists_ 60 fps in the standard Camera2 metadata (`aeAvailableTargetFpsRanges` stops at
  30). The vendor tables (@vendor) reveal that the HAL supports it. Oculibre uses them as a hint, and
  *measures the real camera rate while streaming*.
- The *constrained high-speed (slow-motion) mode* (capture 120 fps, keep 1 in 2) stays as a fallback.
  It is much darker (exposure ≤ 1/120 s, ISO often capped): usable on the S23, unusable indoors on the
  Redmi.
- Manual exposure (AE off + 16.7 ms frame duration) does *not* work on the Redmi (stays at 30 fps), as
  reported elsewhere.

= Reading `adb shell dumpsys media.camera` <reading>

The dump prints the static metadata (`CameraCharacteristics`) of every camera the HAL knows,
*including cameras hidden from apps* (see the Redmi notes in @devices). Arrays are printed four values
per line, so tuples spill across lines: always count from the `int32[N]` size. In the repository, use
`scripts/device-report.py`: it removes personal data and summarises these keys.

== `android.control.aeAvailableTargetFpsRanges`

A flat list of `[min, max]` pairs. Each pair is a valid value for `CONTROL_AE_TARGET_FPS_RANGE` in a
*normal* capture session.
- `min == max`: fixed frame rate (what you want for streaming).
- `min < max`: variable, the auto-exposure may slow down in low light.

```
int32[10]
[15 15 7 24 ]      →  [15,15] [7,24]
[24 24 8 30 ]      →  [24,24] [8,30]
[30 30 ]           →  [30,30]
```

If no pair reaches 60, a normal session cannot _officially_ run at 60 fps (but see @lab). Related
constraint: `StreamConfigurationMap.getOutputMinFrameDuration(size)` must be ≤ 16.7 ms for 60 fps at
that size.

== `android.control.availableHighSpeedVideoConfigurations`

A flat list of *5-tuples* `(width, height, fps_min, fps_max, batch_size_max)`, only usable in a
_constrained high-speed_ session (`createConstrainedHighSpeedCaptureSession`).
- For each fixed range `[N, N]` there is a variable `[30, N]`: the variable one is meant for
  *preview only*, the fixed one for *recording*.
- `batch_size_max`: how many frames each request burst carries (`createHighSpeedRequestList`).
- Constraints: at most 2 output surfaces (preview + video), limited controls, exposure ≤ 1/N s.

```
int32[50]
[1920 1080 30 120 ] [4 1920 1080 120 ] [120 2 1920 1080 ] ...
→ (1920,1080, 30,120, 4)  (1920,1080, 120,120, 2)  (1920,1080, 30,240, 8) ...
```

== Other useful keys

#table(
  columns: (auto, 1fr),
  table.header([Key], [Why it matters]),
  [#tag("android.info.supportedHardwareLevel")], [`LEGACY` / `LIMITED` / `FULL` / `LEVEL_3`. Below `FULL`, manual controls are not guaranteed.],
  [#tag("android.request.availableCapabilities")], [Must contain `CONSTRAINED_HIGH_SPEED_VIDEO` (= 9) for the high-speed path. `MANUAL_SENSOR` (= 1) is needed for manual exposure / frame duration.],
  [#tag("android.request.availableSessionKeys")], [Keys set when the session is created, not per request (often includes the fps range).],
  [`samsung.*`, `com.xiaomi.*`, `org.codeaurora.*`, `org.quic.*`], [Vendor tags. They may reveal capabilities hidden from the standard keys (@vendor).],
)

= Our two devices <devices>

== Redmi Note 9 Pro (Snapdragon 720G, MIUI, Android 12)

#table(
  columns: (auto, 1fr, 1fr),
  table.header([], [Back cameras], [Other cameras]),
  [Normal AE ranges], [`[15,15] [7,24] [24,24] [8,30] [30,30]`: *max 30*], [`[15,15] [8,30] [30,30]`: *max 30*],
  [High-speed], [1080p: 120 (batch 4) · 720p: 120 (batch 4), 240 (batch 8) · 720×480: 120, 240 · 640×480: 120], [720p, 720×480, 640×480: 120],
)

8 camera entries in the dump. Apps see fewer: Qualcomm/Xiaomi builds hide auxiliary cameras from
packages that are not on an allowlist (`vendor.camera.aux.packagelist`).

*Slow-motion 60 fps in Oculibre:* very dark indoors, only the light bulb was visible.

== Samsung Galaxy S23 Ultra (One UI, Android 17)

#table(
  columns: (auto, 1fr, 1fr),
  table.header([], [Main sensor], [Other cameras]),
  [Normal AE ranges], [`[10,10] [7,11] [11,11] [7,15] [15,15] [24,24] [7,30] [30,30]`: *max 30*], [same, *max 30*],
  [High-speed], [1080p: 120 (batch 2), 240 (batch 4) · 1920×824: 120 · 720p: 120, 240], [1080p, 1920×824, 720p: 120],
  [Vendor tag], [#tag("samsung.android.scaler.availableHighSpeedVideoConfigurations"): same sizes, 120 and 240], [],
)

*Slow-motion 60 fps in Oculibre:* good picture. The larger sensor and Samsung's tuning (higher ISO
allowed) make up for the 1/120 s cap.

Neither vendor lists 60 fps for normal sessions, *even Samsung*.

= Why the stock app gets 60 fps <stock>

- The stock camera talks to the HAL through *vendor-private extensions* (vendor tags, private session
  modes, sometimes a privileged system API). Manufacturers reserve high frame rates, some lenses
  (telephoto) and some processing (HDR, stabilisation) for their own app or for selected partners.
- Samsung's _Galaxy Camera Ecosystem_ gives selected partners a *Samsung Camera SDK* (HDR, Super Night,
  VDIS…), but frame rates are not part of it, and the SDK is not open to everyone. Requests from
  developers on Samsung's forum to unlock 60 fps for third-party apps have gone unanswered for years.
- Some devices, notably some Google Pixels, do let third-party apps record at 60 fps: Open Camera
  gained 4K60 on the Pixel 4, and Blackmagic Camera offers 60 fps only on "selected handsets".
- Manual exposure (AE off) has been seen to cap at 30 fps on some devices, even when 60 fps is listed.

Our first conclusion was that no app logic could reach 60 fps on these phones. The vendor tables
(@vendor) and the lab (@lab) proved it wrong: the HAL accepts a range it does not list.

= Why high-speed mode is darker <darker>

At a fixed 120 fps, each frame is exposed for *at most 8.3 ms* (1/120 s): one stop less light than a
real 60 fps (16.7 ms). On top of that, vendors often cap ISO / analog gain in high-speed mode (it is
tuned for slow-motion in daylight) and ignore exposure compensation. Dropping 1 frame in 2 throws away
half of the captured light. The S23 compensates with sensor size and gain; the Redmi cannot.

= Options for Oculibre <options>

#table(
  columns: (1.6fr, 1fr, auto, 1.2fr),
  table.header([Option], [Picture], [Effort], [Verdict]),
  [*A.* Normal 60 fps when listed], [Normal], [Done], [#done Automatic],
  [*B.* High-speed 120 → 60, drop 1 in 2], [Dark on mid-range], [Done], [#done Kept as fallback, labelled "slow-motion mode (needs light)"],
  [*C.* High-speed 120 → 60, _sum_ each pair of frames on the GPU], [≈ +1 stop vs B], [Medium], [#open Only if D fails on other phones],
  [*D.* Unlisted `[60,60]` range in a normal session], [Normal], [Small], [#done *Works on both phones* (@lab): used when vendor tables advertise 60 fps],
  [*E.* Vendor keys (`samsung.android.control.*`, `org.quic.*`)], [Normal, if honoured], [Small, fragile], [#done Tested: not needed, Samsung's 60–60 breaks the session],
  [*F.* Root / build.prop tweaks], [—], [—], [#fail Out of scope for a public app],
  [*G.* Android 14+ native USB webcam mode], [Vendor-defined], [None], [#open Suggest it when available],
)

= Findings in vendor tags <vendor>

== Redmi Note 9 Pro: Qualcomm tables list 1080p60

On the back cameras, the Qualcomm (CamX) HAL publishes two tables:

#table(
  columns: (1.3fr, auto, 1.4fr),
  table.header([Tag], [Layout], [Excerpt]),
  [#tag("org.quic.camera2.customhfrfps.info.CustomHFRFpsTable")], [`(w, h, fps)`], [`2304×1728@60`, `2160×1080@60`, `1920×1440@60`, *`1920×1080@60`*, `1600×1200@60`, `1560×720@60`, `1440×1080@60`…],
  [#tag("org.quic.camera2.streamBasedFPS.info.StreamBasedFPSTable")], [`(w, h, min, max)`], [`2304×1728 [30,60] [60,60]`, `2160×1080 [30,60] [60,60]`, `1920×1440 [30,60] [60,60]`…],
)

So *the sensor pipeline supports real 60 fps* (normal session, normal exposure); Xiaomi filters it out of
`aeAvailableTargetFpsRanges`. Related Qualcomm keys: #tag("org.quic.camera.manualExposure.disableFPSLimits")
(request), #tag("org.codeaurora.qcamera3.sessionParameters.dynamicFPSConfig") (session parameter). The S23,
also a Qualcomm SoC, publishes the same tables.

== Galaxy S23 Ultra: Samsung "effect" range and recording session keys

#table(
  columns: (1.3fr, 1.2fr, 1.2fr),
  table.header([Tag], [Value], [Meaning (inferred)]),
  [#tag("samsung.android.control.effectAeAvailableTargetFpsRanges")], [`[10,30] [24,60]` on *every* camera], [AE ranges allowed when Samsung effects / video modes are active: up to 60 fps],
  [#tag("samsung.android.request.availableSessionKeys")], [includes `recordingMaxFps`, `recordingMinFps`, `shootingMode`, `recordingExtraMode`, `cameraClient`, `unihal.videoMode`…], [Samsung's own app very likely selects 60 fps through these session parameters],
  [#tag("samsung.android.control.availableHalFrcFps")], [`240`], [HAL frame-rate conversion for slow-motion],
)

Vendor keys can be used from an app: `CaptureRequest.Key(name, type)` is public since API 29, and
session parameters are set with `SessionConfiguration.setSessionParameters()` (API 28).

= Lab results <lab>

Oculibre lab screen, back camera, 1080p, real H.264 encoder as target. Measured from
`SENSOR_TIMESTAMP` over 2 s after a 1.5 s AE warm-up, in the same dim room. Full logs in each device's
`report.md` under `devices/`.

#table(
  columns: (1.5fr, 1fr, 1fr),
  table.header([Strategy], [Redmi Note 9 Pro (Android 12)], [Galaxy S23 Ultra (Android 17)]),
  [S0 reference `[30,30]`], [30.0 fps · 16.7 ms · ISO 1599], [30.0 fps · 30.0 ms · ISO 34857],
  [*S1 `[60,60]` per request*], [#done *60.5 fps* · 8.3 ms · ISO 3224], [#done *59.8 fps* · 16.7 ms · ISO 31364],
  [S1b `[30,60]` per request], [60.5 fps · 8.3 ms · ISO 3221], [33.3 fps (AE slows down in low light)],
  [S2 `[60,60]` as session parameter], [60.5 fps], [59.8 fps],
  [S3a Samsung `recordingMin/MaxFps` 24–60 + `[24,60]`], [key not available], [25.0 fps (variable, low light)],
  [S3b Samsung `recordingMin/MaxFps` 60–60 + `[60,60]`], [key not available], [#fail session error (`Broken pipe`), camera closed],
  [S4a/b Qualcomm `disableFPSLimits` + `[60,60]`], [60.5 fps (same as S1)], [not run (camera closed by S3b)],
  [S5 manual, AE off, 16.7 ms], [#fail 30.0 fps (ignored)], [not run],
)

Takeaways:
- *S1 is the strategy*: a fixed `[60,60]` per request. Variable ranges (`[30,60]`, `[24,60]`) let the AE
  drop the frame rate in low light.
- Samsung's private session keys are not needed, and `recordingMinFps = 60` breaks session configuration.
- On the Redmi, the AE caps exposure at 8.3 ms at 60 fps and doubles the ISO: brighter than slow-motion
  mode, noisier than 30 fps.
- The lab now reopens the camera for each strategy, so one crash no longer stops the run.

= Next experiments <next>

+ #done Vendor tags on both phones (@vendor).
+ #done 60 fps strategies in the lab (@lab).
+ #open *Measure the darkness instead of guessing.* Read `SENSOR_EXPOSURE_TIME` and `SENSOR_SENSITIVITY`
  from `CaptureResult`; when they sit at their limits, warn the user ("too dark for 60 fps, use 30").
+ #open Re-run the lab on the S23 (S4, S5 were not run) and on more phones, especially old ones and
  Pixels.
+ #open Option C (GPU summing of frame pairs) only if some phones accept neither S1 nor a usable
  slow-motion mode.

= Sources <sources>

- AOSP camera metadata, tuple layout of `availableHighSpeedVideoConfigurations`:
  #link("https://android.googlesource.com/platform/hardware/interfaces/+/android17-release/camera/metadata/3.2/types.hal")[hardware/interfaces, camera metadata `types.hal`]
- `StreamConfigurationMap` javadoc, fixed vs `[30, max]` ranges, preview vs recording:
  #link("https://code.yawk.at/android/android-9.0.0_r35/android/hardware/camera2/params/StreamConfigurationMap.java")[StreamConfigurationMap.java (Android 9)]
- 60 fps in a normal session, min frame duration, manual-exposure caveat:
  #link("https://kiwix.lema.org/stackoverflow.com_en_all_2023-11/a/43646443")[Stack Overflow answer (archive)]
- Third-party apps stuck at 30 fps, dump showing only 120/240 high-speed modes:
  #link("https://forum.fairphone.com/t/why-does-the-camera-api-only-expose-up-to-30-fps/128891")[Fairphone forum]
- Samsung developers asking to unlock 1080p60 for third-party apps:
  #link("https://forum.developer.samsung.com/t/allow-1080p-60fps-in-any-3rd-party-camera-apps/10653")[Samsung Developer Forum],
  #link("https://forum.developer.samsung.com/t/galaxy-s22-ultra-maximum-video-resolution-and-fps-from-camera-api/21218/2")[S22 Ultra thread]
- Samsung Galaxy Camera Ecosystem / Camera SDK for selected vendors:
  #link("https://r2.community.samsung.com/t5/CamCyclopedia/Samsung-Galaxy-Camera-Ecosystem/ba-p/13958899")[Samsung Community]
- Manufacturers may restrict high frame rates to the stock app:
  #link("https://opencamera.org.uk/devices.html")[Open Camera device notes]
- Open Camera brings 4K60 to the Pixel 4:
  #link("https://androidcentral.com/open-camera-app-brings-4k60-video-recording-pixel-4")[Android Central]
- Blackmagic Camera frame rates on selected handsets:
  #link("https://9to5google.com/2024/09/13/blackmagic-camera-1-3-adds-more-recording-framerates-on-some-pixels-and-more/")[9to5Google]
- CameraX 1.5 high-speed API:
  #link("https://android-developers.googleblog.com/2025/10/high-speed-capture-and-slow-motion.html")[Android Developers Blog]
- Auxiliary cameras hidden behind a package allowlist:
  #link("https://git.evolution-x.org/Evolution-X-Devices/device_xiaomi_stone/commit/890f65768a18999534b8e62fae4b004f48698e5c")[Xiaomi device tree commit]
