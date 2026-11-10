# Architecture

This document describes the target architecture. It will evolve as the prototype is built.

## Principles

1. **Low latency over everything.** Raw H.264 over a single TCP connection, no RTSP/GStreamer buffering. Drop frames until the next keyframe rather than accumulating delay.
2. **Old hardware first.** Android 5.0 (API 21) minimum. Zero-copy capture. ChaCha20 instead of AES (no AES acceleration on old ARMv7).
3. **Private by default.** Pairing by QR code, Noise encryption on Wi-Fi, no cloud, no telemetry.
4. **No proprietary dependency.** Must be publishable on F-Droid (no Google Play Services).
5. **One protocol, documented.** [`protocol.md`](protocol.md) is the contract between the phone and the desktop.

## Data path

```
Camera2 ─► encoder input Surface ─► MediaCodec (H.264 HW) ─► framing ─► [Noise] ─► TCP
                                                                                    │
                                                         Wi-Fi  /  ADB (USB)  /  USB tethering
                                                                                    │
TCP ─► [Noise] ─► parser ─► FFmpeg decode ─► NV12/YUY2 ─► shared ring buffer ─► virtual camera
```

## Android (sender) — `android/`

| Concern | Choice | Why |
|---|---|---|
| Language | Kotlin + coroutines | Standard, readable pipeline code |
| minSdk | 21 (Android 5.0) | The project's main differentiator |
| Capture | **Camera2** directly (not CameraX) | Full manual control; CameraX's minSdk may rise. Fallback for `LEGACY` hardware level |
| Zero-copy | `MediaCodec.createInputSurface()` as a Camera2 target | No frame ever goes through app code |
| Encoding | H.264 Constrained Baseline, no B-frames | Universal, lowest latency. HEVC later as an option |
| Keyframes | `PARAMETER_KEY_REQUEST_SYNC_FRAME` on connect / loss | Fast recovery |
| Bitrate | Adjustable on the fly (`PARAMETER_KEY_VIDEO_BITRATE`) | Adapts to Wi-Fi quality |
| Background | Foreground service (type `camera`) + wakelock | Required on recent Android |
| Thermal | `PowerManager.getCurrentThermalStatus()` (API 29+), battery temperature below | Auto-lower resolution/fps; protects the battery |
| Discovery | `NsdManager` (mDNS) | One-tap connection |
| Pairing | ZXing Android Embedded (QR scan) | Works without Play Services. **The PC shows the QR, the phone scans it** (the PC may have no camera — that's the point) |
| Crypto | noise-java | Noise handshake + ChaCha20-Poly1305 |
| UI | Classic Views or minimal Compose, 2 screens | Small APK |

## Transport

| Mode | Phase | Notes |
|---|---|---|
| USB via ADB (`adb reverse`) | 1 | Simplest to start; requires developer mode |
| Wi-Fi (TCP) | 2 | Encrypted by default |
| USB tethering (RNDIS) | 4 | No developer mode needed; the Wi-Fi code path works as is |
| SRT / QUIC | Later | Only if TCP jitter on Wi-Fi is a real problem |

## Desktop (receiver) — `desktop/`

Single Rust binary.

| Concern | Crate / tool |
|---|---|
| Async network | `tokio` |
| Discovery | `mdns-sd` |
| Encryption | `snow` (Noise) |
| Decoding | `ffmpeg-next` (libavcodec, LGPL build — **do not enable GPL components**). Optional HW decode (VAAPI, D3D11VA, VideoToolbox) |
| GUI | Tauri, tray icon: QR code, device list, settings, preview |
| CLI | For Linux users and headless setups |

Planned workspace crates: `protocol`, `core`, `cli`, `app-tauri`, `vcam-linux`.

## Virtual camera backends — `vcam/`

All behind a single Rust trait, fed by a shared-memory ring buffer.

| OS | Technique | Language | Notes |
|---|---|---|---|
| Linux | v4l2loopback | Rust (in `desktop/`) | Easiest. PipeWire camera later (no kernel module) |
| Windows 10 | DirectShow source filter (based on Softcam, MIT) | C++ (COM DLL) | Only seen by DirectShow apps |
| Windows 11 | Media Foundation virtual camera (`MFCreateVirtualCamera`) | C++ (COM DLL) | Seen by every app, including the Camera app and new Teams |
| macOS | CoreMediaIO Camera Extension (sink stream) | Swift | Needs a paid Apple Developer account + notarization |

**No kernel driver on Windows** (would require EV code signing).

## Known risks

- MediaCodec bugs differ per vendor (color formats, keyframes, SPS/PPS handling) → a fleet of real old devices is mandatory.
- Old phones plugged in 24/7 while encoding get hot → battery swelling risk → thermal management is a feature, not a bonus.
- Audio (virtual microphone) is a project of its own on Windows and macOS → postponed after v1.
- macOS distribution costs 99 $/year.

## Quality

- CI: GitHub Actions matrix (Ubuntu, Windows, macOS) + APK build, `clippy`, `ktlint`.
- Fuzzing of the protocol parser with `cargo-fuzz` (the desktop parses network input).
- Glass-to-glass latency measurement: film a stopwatch shown on screen, compare both values.
