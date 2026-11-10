# Redmi Redmi Note 9 Pro

| | |
|---|---|
| Model | Redmi Note 9 Pro (codename joyeuse) |
| Android | 12 (API 31), build SKQ1.211019.001 |
| SoC | QTI SM7125 |

## Camera capabilities

Generated from [`camera-android12-skq1-211019-001.txt`](camera-android12-skq1-211019-001.txt). "Max normal fps" is the highest frame
rate an app can request in a regular session; "high-speed" modes are slow-motion sessions
(darker, see [docs/research/60fps.typ](../../../docs/research/60fps.typ)).

| Camera ID | Facing | HW level | Max normal fps | Fixed-rate ranges | High-speed (fixed) |
|---|---|---|---|---|---|
| 0 | BACK | LEVEL_3 | **30** | 15, 24, 30 | 1920×1080@120, 1280×720@120, 1280×720@240, 720×480@120, 720×480@240, 640×480@120, 640×480@240 |
| 1 | FRONT | LEVEL_3 | **30** | 15, 30 | 1280×720@120, 720×480@120, 640×480@120 |
| 100 | BACK | LEVEL_3 | **30** | 15, 24, 30 | 1920×1080@120, 1280×720@120, 1280×720@240, 720×480@120, 720×480@240, 640×480@120, 640×480@240 |
| 101 | FRONT | LEVEL_3 | **30** | 15, 30 | 1280×720@120, 720×480@120, 640×480@120 |
| 20 (hidden) | BACK | LEVEL_3 | **24** | 15, 24 | — |
| 21 | BACK | LEVEL_3 | **30** | 15, 30 | — |
| 22 | BACK | LEVEL_3 | **30** | 15, 30 | — |
| 61 (hidden) | BACK | LEVEL_3 | **30** | 15, 24, 30 | 1920×1080@120, 1280×720@120, 1280×720@240, 720×480@120, 720×480@240, 640×480@120, 640×480@240 |

## Oculibre results

<!-- Fill in after testing. -->

- Oculibre version:
- Modes offered by the app:
- 720p30 / 1080p30 / 4K30:
- 60 fps (normal or high-speed) and picture quality:
- Latency (glass-to-glass), USB / Wi-Fi:
- Temperature after 30 min:
- Notes:

# Labs Result 
Oculibre lab — 60 fps tests
Xiaomi Redmi Note 9 Pro · Android 12 (API 31) · build SKQ1.211019.001
Camera 0 · 1920x1080 · HW level LEVEL_3
Standard AE ranges: [15, 15] [7, 24] [24, 24] [8, 30] [30, 30]
Session keys: aeTargetFpsRange
Qualcomm StreamBasedFPS 1080p: [30,60] [60,60]
Qualcomm CustomHFR 1080p: 60 fps

Strategy                                   | result
-------------------------------------------+------------------------------------------
S0  reference AE[30,30]                    | 30,0 fps · exp 16,7ms ISO 1599 · frame 33,3ms · AE[30, 30]
S1  AE[60,60] per request                  | 60,5 fps · exp 8,3ms ISO 3224 · frame 16,7ms · AE[60, 60]  <== 60 fps!
S1b AE[30,60] per request                  | 60,5 fps · exp 8,3ms ISO 3221 · frame 16,7ms · AE[30, 60]  <== 60 fps!
S2  AE[60,60] session parameter            | 60,5 fps · exp 8,3ms ISO 3221 · frame 16,7ms · AE[60, 60]  <== 60 fps!
S3a Samsung recordingFps 24-60, AE[24,60]  | key not available on this phone
S3b Samsung recordingFps 60-60, AE[60,60]  | key not available on this phone
S4a Qualcomm disableFPSLimits, AE[60,60]   | 60,5 fps · exp 8,3ms ISO 3218 · frame 16,7ms · AE[60, 60]  <== 60 fps!
S4b same as S4a, session parameters        | 60,5 fps · exp 8,3ms ISO 3218 · frame 16,7ms · AE[60, 60]  <== 60 fps!
S5  manual AE off, frameDuration 16.7ms    | 30,0 fps · exp 16,0ms ISO 1599 · frame 33,3ms · AE[30, 30]

Done. Tap "Copy results" and paste them into devices/<brand>/<model>/report.md
