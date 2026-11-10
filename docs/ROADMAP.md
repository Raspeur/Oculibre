# Roadmap

Estimates are for one person at ~8–10 h/week (side project).

## Phase 0 — Foundations
- [x] Repository, license, architecture, protocol draft
- [ ] Gather 4–5 old test phones from different vendors (Samsung, Xiaomi, Huawei, a Nexus/Pixel…), ideally Android 5 → 8
- [ ] Validate Camera2 → MediaCodec on each of them (a throwaway MJPEG/preview test is fine)

## Phase 1 — Linux MVP (USB)
- [ ] `docs/protocol.md` frozen as v0
- [x] Android: Camera2 → MediaCodec (input Surface) → TCP
- [ ] Android: foreground service (keep streaming screen off)
- [ ] Rust `protocol` crate + `cargo-fuzz` target
- [x] Rust CLI: TCP → H.264 decode (OpenH264) → v4l2loopback
- [x] Transport via `adb reverse`
- [ ] Glass-to-glass latency measurement documented
- 🎯 *Usable on Linux*

## Phase 2 — Wi-Fi and security
- [ ] mDNS discovery (`NsdManager` / `mdns-sd`)
- [ ] QR pairing (desktop shows, phone scans)
- [ ] Noise IK handshake + ChaCha20-Poly1305
- [ ] Drop policy + adaptive bitrate

## Phase 3 — Desktop app and Windows
- [ ] Tauri app (tray icon, QR, devices, preview, settings)
- [ ] Windows DirectShow backend (Softcam-based)
- [ ] Windows 11 Media Foundation backend
- [ ] MSI installer, code signing (SignPath Foundation)
- 🎯 *v1: Linux + Windows, video only*

## Phase 4 — Station mode and old devices
- [ ] Auto-start, screen off, auto-reconnect
- [ ] Thermal management (auto-lower resolution/fps), charging advice
- [ ] USB tethering (RNDIS) mode — no developer mode needed
- [ ] Detect Android 14+ native USB webcam and suggest it
- [ ] Test campaign + device compatibility table
- [ ] F-Droid release

## Phase 5 — Beyond v1
- [ ] macOS CoreMediaIO Camera Extension
- [ ] Audio (virtual microphone on 3 OS)
- [ ] Advanced controls (lens, focus, exposure, torch)
- [ ] HEVC option, PipeWire camera backend
