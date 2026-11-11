#import "oculibre.typ": *

#show: oculibre-doc.with(
  title: [Roadmap],
  subtitle: [From a Linux prototype to a cross-platform webcam],
  revision: "0.3",
  date: datetime(year: 2026, month: 11, day: 10),
)

Estimates are for one person at about 8–10 h/week (side project). #done done · #open to do.

#set list(marker: none, indent: 0pt, body-indent: 0pt)

= Phase 0 — Foundations

- #done Repository, license, architecture, protocol draft
- #done Device database and sanitised camera reports (`devices/`, `scripts/device-report.py`)
- #open Gather 4–5 *old* test phones from different vendors (Samsung, Xiaomi, Huawei, a Nexus/Pixel…), ideally Android 5 → 8
- #open Validate Camera2 → MediaCodec on each of them

= Phase 1 — Linux MVP (USB)

- #done Android: Camera2 → MediaCodec (input Surface) → TCP
- #done Android: quality picker showing only what the phone supports (720p → 4K)
- #done Android: real 60 fps through an unlisted AE range, slow-motion fallback, measured camera rate (#link("research/60fps.typ")[`research/60fps.typ`])
- #done Android: lab screen for frame-rate strategies
- #done Android: foreground service, keeps streaming with the screen off; battery-optimisation exemption
- #done Android: drop policy (drop until next keyframe when the network lags)
- #done Rust CLI: TCP → H.264 decode (OpenH264) → v4l2loopback, resolution read back from the driver
- #done Transport via `adb reverse`
- #done #link("protocol.typ")[`protocol.typ`] v0.1 written
- #done Protocol v0.1 implemented: Rust `protocol` crate + test vectors + `cargo-fuzz` target, Android, CLI
- #done Host: `--rotate 0|90|180|270|auto` and `--mirror`
- #done Live latency in the console: phone + network + PC, camera vs shown frame rate
- #done Glass-to-glass latency measured and documented (30 and 60 fps)
- #done Automatic bench: `oculibre bench` walks every camera × mode through the protocol 0.2 `set_mode`
  command and writes the results into the phone's `devices/` report
- 🎯 _Usable on Linux_

= Phase 2 — Wi-Fi and security

- #done mDNS discovery: the computer announces `_oculibre-host._tcp` (`mdns-sd`), a paired phone
  finds it again when its address changes (`NsdManager`)
- #done QR pairing: the desktop shows `oculibre://pair?…`, the phone's camera app opens it in Oculibre
  (also sent over USB by `adb`), one-time token, confirmation with the key fingerprint
- #done Secure channel: TLS 1.3 / 1.2 (ECDHE + AEAD) with pinned self-signed keys, mandatory on any
  network (protocol 0.3 §13, #link("research/secure-channel.typ")[`research/secure-channel.typ`])
- #open Adaptive bitrate from host statistics

= Phase 3 — Desktop app and Windows

- #open Tauri app (tray icon, QR, devices, preview, rotation, settings)
- #open Camera controls from the PC (mode, lens, zoom, focus, exposure, torch)
- #open Windows DirectShow backend (Softcam-based)
- #open Windows 11 Media Foundation backend
- #open MSI installer, code signing (SignPath Foundation)
- 🎯 _v1: Linux + Windows, video only_

= Phase 4 — Station mode and old devices

- #open Auto-start, screen off, auto-reconnect; host dials the phone (protocol §2)
- #open Thermal management (auto-lower resolution/fps), charging advice
- #open USB tethering (RNDIS) mode: no developer mode needed
- #open Detect Android 14+ native USB webcam and suggest it
- #open Test campaign + device compatibility table
- #open F-Droid release

= Phase 5 — Beyond v1

- #open macOS CoreMediaIO Camera Extension
- #open Audio (virtual microphone on 3 OS)
- #open HEVC option, PipeWire camera backend
- #open Several phones on one PC (multi-camera)
