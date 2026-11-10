# Oculibre

**Turn any Android phone — even an old one — into a webcam for Linux, Windows and macOS.**
Free, open source, private by default. No ads, no watermark, no paid "Pro" tier.

> 🇫🇷 *Oculibre transforme un smartphone Android, même ancien, en webcam pour PC. Gratuit, open source, chiffré par défaut. Les contributions en français sont bienvenues.*

> ⚠️ **Status: early design phase.** Nothing is usable yet. The protocol and architecture are being specified — see [`docs/`](docs/).

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
│ Camera2 ──► MediaCodec   │   H.264 over TCP  │ Rust core: decode (FFmpeg)   │
│  (zero-copy Surface)     │ ════════════════► │        ──► virtual camera    │
│ Noise encryption         │  Wi-Fi  or  USB   │ Linux   : v4l2loopback       │
│ mDNS + QR pairing        │                   │ Windows : DirectShow / MF    │
└──────────────────────────┘                   │ macOS   : CMIO Extension     │
                                               └──────────────────────────────┘
```

Details in [`docs/ARCHITECTURE.typ`](docs/ARCHITECTURE.typ) and the wire format in [`docs/protocol.typ`](docs/protocol.typ).

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

1. **Phase 1** — H.264 over TCP through ADB (USB), received by a Rust CLI writing to v4l2loopback (Linux).
2. **Phase 2** — Wi-Fi, mDNS discovery, QR pairing, Noise encryption.
3. **Phase 3** — Tauri desktop app, then Windows (DirectShow, then Media Foundation).
4. **Phase 4** — Station mode, thermal management, test campaign on old devices.
5. **Phase 5** — macOS, audio (virtual microphone), advanced camera controls.

Full roadmap: [`docs/ROADMAP.typ`](docs/ROADMAP.typ).

## Contributing

Looking for people interested in Android (Kotlin, Camera2, MediaCodec), video (H.264, FFmpeg), Rust, and system programming (virtual cameras on Windows/macOS/Linux).

**You can help without writing code:** testing on old phones is extremely valuable. See [`CONTRIBUTING.md`](CONTRIBUTING.md).

## License

[MIT](LICENSE).
