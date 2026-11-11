# Oculibre

**Turn any Android phone — even an old one — into a webcam for Linux, Windows and macOS.**
Free, open source, private by default. No ads, no watermark, no paid "Pro" tier.

> 🇫🇷 *Oculibre transforme un smartphone Android, même ancien, en webcam pour PC. Gratuit, open source, chiffré par défaut. Les contributions en français sont bienvenues.*

> ⚠️ **Status: early, Linux only.** The phone streams up to 4K or 1080p60 to a Linux virtual webcam,
> with about 60–110 ms of latency, over USB or over Wi-Fi (encrypted, after pairing with a QR code).
> Windows and macOS are planned. See [Try it](#try-it-linux-usb).

---

## Why another phone-as-webcam project?

Free and open source alternatives already exist (scrcpy, VCamdroid, Plugcam, Mobile-Webcam…), and they are great. But almost all of them are **Windows-only or Linux-only**, and require **Android 7, 9 or even 12+**.

Oculibre focuses on what is still missing:

| | Oculibre goal |
|---|---|
| **Old phones** | Android 5.0+ (API 21). Give a second life to the phone sleeping in your drawer. |
| **Every desktop OS** | One client for Linux, Windows and macOS, with native virtual cameras. |
| **Private by default** | QR-code pairing + end-to-end encryption on Wi-Fi. Nobody else on the network can watch your stream. No cloud, ever. |
| **"Station" mode** | Built to stay plugged in 24/7: auto-start, screen off, auto-reconnect, **thermal management** to protect the battery. |
| **Full camera control from the PC** | Lens choice (ultra-wide, tele), zoom, tap-to-focus, exposure, torch. |

If your phone runs Android 14+ and supports the native USB webcam mode, Oculibre will tell you — you may not need it at all.

## How it works

```
 Android phone                                     Computer
┌──────────────────────────┐                   ┌──────────────────────────────┐
│ Camera2 ──► MediaCodec   │   H.264 over TCP  │ Rust: decode (OpenH264)      │
│  (zero-copy Surface)     │ ════════════════► │        ──► virtual camera    │
│ TLS on Wi-Fi, QR pairing │  Wi-Fi  or  USB   │ Linux   : v4l2loopback       │
│ mDNS + QR pairing        │                   │ Windows : DirectShow / MF    │
└──────────────────────────┘                   │ macOS   : CMIO Extension     │
                                               └──────────────────────────────┘
```

Details in [`docs/ARCHITECTURE.typ`](docs/ARCHITECTURE.typ) and the wire format in [`docs/protocol.typ`](docs/protocol.typ).

## Try it (Linux, USB)

1. Phone: enable **Developer options → USB debugging**, plug it in. Build and install the app:
   `cd android && ./gradlew installDebug` (Android SDK needed).
2. PC: `bash scripts/setup-dev.sh --vcam` once, then `bash scripts/vcam-load.sh` (v4l2loopback) and
   `cd desktop && cargo run --release`.
3. Press **Start** in the app (address `127.0.0.1`). The phone shows up as the **Oculibre** webcam
   in Firefox, Chrome, OBS, Zoom…

More options (rotation, mirror, Wi-Fi, `oculibre bench`) in [`desktop/README.md`](desktop/README.md).

The documentation is written in [Typst](https://typst.app). Build the PDFs with `bash scripts/build-docs.sh` (they land in `docs/build/`).

## Repository layout

| Path | Content | Language |
|---|---|---|
| [`android/`](android/) | The phone app (sender) | Kotlin |
| [`desktop/`](desktop/) | Desktop core, CLI and Tauri app (receiver) | Rust |
| [`vcam/windows-dshow/`](vcam/windows-dshow/) | DirectShow virtual camera (Windows 10) | C++ |
| [`vcam/windows-mf/`](vcam/windows-mf/) | Media Foundation virtual camera (Windows 11) | C++ |
| [`vcam/macos/`](vcam/macos/) | CoreMediaIO Camera Extension | Swift |
| [`docs/`](docs/) | Architecture, protocol spec, roadmap, research (Typst) | — |

## Roadmap

1. **Phase 1** ✅ — H.264 over TCP through ADB (USB), received by a Rust CLI writing to v4l2loopback (Linux).
2. **Phase 2** — Wi-Fi: QR pairing, TLS encryption and mDNS discovery ✅, adaptive bitrate.
3. **Phase 3** — Tauri desktop app, then Windows (DirectShow, then Media Foundation).
4. **Phase 4** — Station mode, thermal management, test campaign on old devices.
5. **Phase 5** — macOS, audio (virtual microphone), advanced camera controls.

Full roadmap: [`docs/ROADMAP.typ`](docs/ROADMAP.typ).

## Contributing

Looking for people interested in Android (Kotlin, Camera2, MediaCodec), video (H.264, FFmpeg), Rust, and system programming (virtual cameras on Windows/macOS/Linux).

**You can help without writing code:** testing on old phones is extremely valuable. See [`CONTRIBUTING.md`](CONTRIBUTING.md).

## License

[MIT](LICENSE).
