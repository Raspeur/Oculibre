# samsung Galaxy S23 Ultra

| | |
|---|---|
| Model | SM-S918B (codename dm3q) |
| Android | 17 (API 37), build CP2A.260605.016 |
| SoC | QTI SM8550 |

## Camera capabilities

Generated from [`camera-android17-cp2a-260605-016.txt`](camera-android17-cp2a-260605-016.txt). "Max normal fps" is the highest frame
rate an app can request in a regular session; "high-speed" modes are slow-motion sessions
(darker, see [docs/research/60fps.md](../../../docs/research/60fps.md)).

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

