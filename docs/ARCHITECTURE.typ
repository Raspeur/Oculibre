#import "oculibre.typ": *

#show: oculibre-doc.with(
  title: [Architecture],
  subtitle: [How an image travels from the phone to the virtual camera],
  revision: "0.3",
  date: datetime(year: 2026, month: 11, day: 12),
)

This document describes the target architecture. It evolves as the project is built; the wire format
itself is specified in #link("protocol.typ")[`protocol.typ`].

= Principles

+ *Low latency over everything.* One byte stream, length-prefixed messages, no RTSP/GStreamer
  buffering. Drop frames until the next keyframe rather than accumulating delay.
+ *Old hardware first.* Android 5.0 (API 21) minimum. Zero-copy capture. ChaCha20 rather than AES
  (no AES acceleration on old ARMv7 CPUs).
+ *Private by default.* No cloud, no telemetry; encryption and pairing on any network.
+ *No proprietary dependency.* Publishable on F-Droid (no Google Play Services). The Android app uses
  platform APIs only: no library fetched by Gradle; small vendored sources under a licence compatible
  with MIT are allowed, pinned, in their own folder with their licence (first case: ZXing's QR
  decoder, \#16).
+ *One documented protocol.* #link("protocol.typ")[`protocol.typ`] is the contract between the phone
  and the desktop.

= Data path

```
PHONE   Camera2 ─► encoder input Surface ─► MediaCodec (H.264, hardware)
                                                   │
                     protocol messages ◄───────────┘
                             │
                     [secure channel] ─► TCP
                                          │  USB (adb reverse) · Wi-Fi · USB tethering
PC                   [secure channel] ◄───┘
                             │
          message parser ─► H.264 decode ─► rotate / mirror ─► YUYV ─► virtual camera
```

= Android (camera side) — `android/`

#table(
  columns: (auto, 1.2fr, 1fr),
  table.header([Concern], [Choice], [Why]),
  [Language], [Kotlin, threads], [Readable pipeline code, no dependency],
  [minSdk], [21 (Android 5.0)], [The project's main differentiator],
  [Capture], [*Camera2* directly (not CameraX)], [Full control; CameraX's minSdk may rise. Fallback for `LEGACY` devices],
  [Zero-copy], [`MediaCodec.createInputSurface()` as Camera2 target], [No frame ever goes through app code],
  [Encoding], [H.264 Baseline when supported, no B-frames], [Universal, lowest latency. HEVC later],
  [Quality], [`Capabilities.kt` probes camera + encoder], [Only offers modes the phone really supports],
  [60 fps], [Unlisted `[60,60]` AE range, slow-motion fallback], [See #link("research/60fps.typ")[`research/60fps.typ`]],
  [Keyframes], [`PARAMETER_KEY_REQUEST_SYNC_FRAME`], [Fast recovery after loss],
  [Background], [Foreground service (type `camera`), wake lock, Wi-Fi lock], [Streams with the screen off],
  [Battery], [Battery-optimisation exemption popup, MIUI shortcut], [Vendors kill background apps],
  [Thermal (planned)], [`getCurrentThermalStatus()` (API 29+), battery temperature below], [Auto-lower quality, protect the battery],
  [Discovery], [`NsdManager` (mDNS), direct route first], [No IP address to type],
  [Pairing], [QR code `oculibre://pair`, opened by the camera app today; in-app scan with a vendored decoder (\#16)], [The PC shows the QR, the phone scans it],
  [UI], [Views built in code], [Small APK],
)

= Transport

#table(
  columns: (auto, auto, 1fr),
  table.header([Mode], [Phase], [Notes]),
  [USB via ADB (`adb reverse`)], [1], [Simplest; requires developer mode],
  [Wi-Fi (TCP)], [2], [TLS with paired phones only (protocol 0.3); plaintext only on loopback (\#5)],
  [USB tethering (RNDIS)], [4], [No developer mode; a real network, so encrypted],
  [SRT / QUIC], [later], [Only if TCP jitter on Wi-Fi is a real problem],
)

= Desktop (host side) — `desktop/`

Rust workspace: `protocol` (the wire format), `core` (the engine, a library without user interface,
\#13) and `cli` (the `oculibre` command). The desktop app (Tauri, \#15) will be a second front-end
over `core`: the engine reports what happens as events and never prints.

#table(
  columns: (auto, 1fr),
  table.header([Concern], [Crate / tool]),
  [Network], [`std::net` today, `tokio` when several connections are needed],
  [Protocol], [`protocol` crate: parser/encoder, test vectors, `cargo-fuzz` target],
  [Decoding], [FFmpeg (the system's libavcodec, linked dynamically: LGPL, or GPL as built by Debian/Ubuntu) with hardware decoding (VA-API, NVDEC), its software decoder otherwise; `openh264` (Cisco, BSD, built from source into the binary) as a last resort or without FFmpeg. Decoding and output run on two threads (\#11). Licences, patents, packaging: #link("research/video-decoding.typ")[`research/video-decoding.typ`]],
  [Image], [rotation, mirroring, I420 → YUYV conversion],
  [Discovery], [`mdns-sd`],
  [Encryption], [`rustls`: TLS 1.3 (1.2 for Android 5–9), pinned self-signed keys, see protocol §13 and #link("research/secure-channel.typ")[`research/secure-channel.typ`]],
  [GUI (planned)], [Tauri + React + TypeScript (\#15), tray icon: QR code, devices, preview, rotation, settings; mockups in `design/mockups/`],
  [CLI], [`oculibre`: for Linux users and headless setups],
)

= Virtual camera backends — `vcam/`

All behind one Rust trait.

#table(
  columns: (auto, 1fr, auto, 1fr),
  table.header([OS], [Technique], [Language], [Notes]),
  [Linux], [v4l2loopback], [Rust (in `desktop/`)], [Working. PipeWire camera later (no kernel module)],
  [Windows 10], [DirectShow source filter (based on Softcam, MIT)], [C++ (COM DLL)], [Only seen by DirectShow apps],
  [Windows 11], [Media Foundation virtual camera (`MFCreateVirtualCamera`)], [C++ (COM DLL)], [Seen by every app, including the Camera app and new Teams],
  [macOS], [CoreMediaIO Camera Extension], [Swift], [Needs a paid Apple Developer account + notarisation],
)

#remark[No kernel driver on Windows: it would require EV code signing.]

= Known risks

- MediaCodec bugs differ per vendor (colour formats, keyframes, SPS/PPS): a fleet of real old devices
  is mandatory, see `devices/`.
- Vendors hide camera features from third-party apps (frame rates, lenses): see
  #link("research/60fps.typ")[`research/60fps.typ`].
- Old phones plugged in 24/7 while encoding get hot: thermal management is a feature, not a bonus.
- Audio (virtual microphone) is a project of its own on Windows and macOS: after v1.
- macOS distribution costs 99 \$/year.

= Quality

- CI: GitHub Actions matrix (Ubuntu, Windows, macOS) + APK build, `clippy`, `ktlint`.
- Protocol test vectors shared by the Kotlin and Rust implementations; fuzzing of the Rust parser.
- Glass-to-glass latency: measured from `PING`/`PONG` clock sync (protocol §11), cross-checked by
  filming a stopwatch.
