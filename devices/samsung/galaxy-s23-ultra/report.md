# samsung Galaxy S23 Ultra

| | |
|---|---|
| Model | SM-S918B (codename dm3q) |
| Android | 17 (API 37), build CP2A.260605.016 |
| SoC | QTI SM8550 |

## Camera capabilities

Generated from [`camera-android17-cp2a-260605-016.txt`](camera-android17-cp2a-260605-016.txt). "Max normal fps" is the highest frame
rate an app can request in a regular session; "high-speed" modes are slow-motion sessions
(darker, see [docs/research/60fps.typ](../../../docs/research/60fps.typ)).

| Camera ID | Facing | HW level | Max normal fps | Fixed-rate ranges | High-speed (fixed) |
|---|---|---|---|---|---|
| 0 | BACK | LIMITED | **30** | 10, 15, 24, 30 | 1920×1080@120, 1920×1080@240, 1920×824@120, 1280×720@120, 1280×720@240 |
| 1 | FRONT | FULL | **30** | 10, 15, 24, 30 | 1920×1080@120, 1920×824@120, 1280×720@120 |
| 2 (hidden) | BACK | LIMITED | **30** | 10, 15, 24, 30 | 1920×1080@120, 1920×824@120, 1280×720@120 |
| 3 | FRONT | FULL | **30** | 10, 15, 24, 30 | 1920×1080@120, 1920×824@120, 1280×720@120 |
| 4 (hidden) | FRONT | FULL | **30** | 10, 15, 24, 30 | 1920×1080@120, 1920×824@120, 1280×720@120 |
| 20 (hidden) | BACK | LEVEL_3 | **30** | 10, 11, 15, 24, 30 | 1920×1080@120, 1920×1080@240, 1920×824@120, 1280×720@120, 1280×720@240 |
| 21 (hidden) | BACK | FULL | **30** | 10, 15, 24, 30 | — |
| 23 (hidden) | BACK | FULL | **30** | 10, 11, 15, 24, 30 | 1920×1080@120, 1920×1080@240, 1920×824@120, 1280×720@120, 1280×720@240 |
| 52 (hidden) | BACK | LIMITED | **30** | 10, 15, 24, 30 | — |
| 54 (hidden) | BACK | LIMITED | **30** | 10, 15, 24, 30 | — |
| 56 (hidden) | BACK | LEVEL_3 | **30** | 10, 11, 15, 24, 30 | 1920×1080@120, 1920×1080@240, 1920×824@120, 1280×720@120, 1280×720@240 |
| 58 (hidden) | BACK | LIMITED | **30** | 10, 15, 24, 30 | 1920×1080@120, 1920×824@120, 1280×720@120 |
| 90 (hidden) | FRONT | FULL | **30** | 10, 15, 24, 30 | 1920×1080@120, 1920×824@120, 1280×720@120 |

## Oculibre results

- Oculibre version: Android app 0.1.0, protocol 0.1 (2026-11-11)
- Modes offered by the app:
- 720p30 / 1080p30 / 4K30:
- 60 fps (normal or high-speed) and picture quality: **1080p60 works** through the unlisted
  `[60,60]` AE range ("unlisted capture"): the camera delivers 59–61 fps and the PC shows 58–61 fps.
- Latency (glass-to-glass), USB / Wi-Fi: see below, **1080p60: ~112 ms USB, ~103 ms Wi-Fi**
- Temperature after 30 min:
- Notes: the H.264 bitrate measured over USB went from 11 to 34 Mbit/s, well above the
  12 Mbit/s planned for 1080p60 (to investigate).

### Latency, 1080p60, back camera

| Link | Measured by the PC (capture → virtual camera) | Glass-to-glass (stopwatch) | Display share |
|---|---|---|---|
| USB (`adb reverse`) | 82 ms | 112 ms | 30 ms |
| Wi-Fi (`--insecure-lan`) | 92 ms | 103 ms | 11 ms |

- *Measured by the PC*: average latency shown by `oculibre` (§11 of the protocol: capture time on
  the phone brought to the PC's clock by `PING`/`PONG`, until the frame is written to v4l2loopback).
  Round trip: about 2 ms over USB, about 4 ms over Wi-Fi.
- *Glass-to-glass*: a millisecond stopwatch shown on the PC screen, filmed by the phone, with the
  virtual camera displayed next to it; difference between the two stopwatches on a screenshot.
- *Display share*: what the PC measurement does not include, i.e. the app showing the virtual camera
  and the screen refresh (one or two frames at 60 Hz). It varies with the moment the frame falls in
  the screen refresh, which explains 11 ms on one side and 30 ms on the other.
- Wi-Fi only costs about 10 ms more than USB (PC measurement); the glass-to-glass figures of both
  links are within the spread of the method.


# Labs Result 

Oculibre lab — 60 fps tests
samsung SM-S918B · Android 17 (API 37) · build CP2A.260605.016
Camera 0 · 1920x1080 · HW level LEVEL_3
Standard AE ranges: [10, 10] [7, 11] [11, 11] [7, 15] [15, 15] [24, 24] [7, 30] [30, 30]
Session keys: aeTargetFpsRange, videoStabilizationMode, testPatternMode, overrideResourceCostValidation, enableMFNR, EnableSHDR, EnableMFHDR, HDRMode, SnapshotHDRMode, numPCRsBeforeStreamOn, EnableMCXMasterCb, McxYuvCallbackInfo, McxRawCallbackInfo, HDRVideoMode, dynamicFPSConfig, RawCbSourceType, ExtendedMaxZoom, EnableVSR, enableMCTFwithReferenceFrame, enableQLL, contour_version, enableStatsVisualizer, inSensorSHDRMode, EnableInsensorZoom, EnableQHDR, EnableAutoHDR, numHDRexposure, HDRModePreference, EnableAFBracketing, InduceEventType, InduceErrorType, InduceSessionName, InducePipelineName, InduceNodeName, InducePortId, InduceRequestId, InduceErrorCode, InduceDelayTime, enableSecureMode, EnableAFFocusMap, EnableIdealRAW, EnableSalinet, EnableXHDR, AICameraMode, BlurMode, EnableVIULL, HorizonLevelControl, EnableXCFAOptimization
Samsung effect AE ranges: [10,30] [24,60]
Qualcomm StreamBasedFPS 1080p: [30,60] [60,60]
Qualcomm CustomHFR 1080p: 60 fps

Strategy                                   | result
-------------------------------------------+------------------------------------------
S0  reference AE[30,30]                    | 30,0 fps · exp 20,0ms ISO 2690 · frame 33,3ms · AE[30, 30]
S1  AE[60,60] per request                  | 57,6 fps · exp 10,0ms ISO 5665 · frame 16,7ms · AE[60, 60]  <== 60 fps!
S1b AE[30,60] per request                  | 33,3 fps · exp 30,0ms ISO 2008 · frame 30,1ms · AE[30, 60]
S2  AE[60,60] session parameter            | 57,6 fps · exp 10,0ms ISO 8367 · frame 16,7ms · AE[60, 60]  <== 60 fps!
S3a Samsung recordingFps 24-60, AE[24,60]  | 33,3 fps · exp 30,0ms ISO 4599 · frame 30,1ms · AE[24, 60]
S3b Samsung recordingFps 60-60, AE[60,60]  | camera open failed: getCameraCharacteristics:1596: Unable to retrieve camera characteristics for unknown device 0: No such file or directory (-2)
S4a Qualcomm disableFPSLimits, AE[60,60]   | camera open failed: getCameraCharacteristics:1596: Unable to retrieve camera characteristics for unknown device 0: No such file or directory (-2)
S4b same as S4a, session parameters        | camera open failed: getCameraCharacteristics:1596: Unable to retrieve camera characteristics for unknown device 0: No such file or directory (-2)
S5  manual AE off, frameDuration 16.7ms    | camera open failed: getCameraCharacteristics:1596: Unable to retrieve camera characteristics for unknown device 0: No such file or directory (-2)

Done. Tap "Copy results" and paste them into devices/<brand>/<model>/report.md

