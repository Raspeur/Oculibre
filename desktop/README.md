# Desktop (receiver)

The computer side of Oculibre: a Rust workspace that receives the phone's video, decodes it and
feeds a virtual camera. Linux and Windows.

| Crate | What it is |
|---|---|
| `core` → `oculibre-core` | The engine (library): sessions, TLS and pairing, discovery, decoding, virtual camera, preview, per-phone settings, bench. It never prints: front-ends get `Event`s |
| `app/src-tauri` → `oculibre-app` | **The desktop app** (Tauri 2 + React, see [`app`](app)): what users install |
| `cli` → binary `oculibre` | The engine in a terminal, for development and `oculibre bench` |
| `protocol` → `oculibre-protocol` | **Protocol 0.3** ([`docs/protocol.typ`](../docs/protocol.typ)): preamble, header, messages, reader with the §14 limits, clock sync, `auto` rotation. Shared test vectors in [`tests/protocol/vectors`](../tests/protocol/vectors), fuzz target in `protocol/fuzz` |
| `vcam-shm` | Windows: the shared memory the engine writes each picture into, read by both cameras below |
| `vcam-dshow` | Windows: the DirectShow camera "Oculibre", a COM DLL (64 and 32-bit) seen by OBS, Zoom, Discord, browsers… |
| `vcam-mf` | Windows 11: the Media Foundation camera, a media source DLL run by Windows' Frame Server, plus `oculibre-camera.exe` that registers it. Seen by every app, the Windows Camera app included |

On Linux the virtual camera is v4l2loopback (`core/src/vcam.rs`, `/dev/video10`). Why two cameras on
Windows, and how they work: [`docs/research/windows.typ`](../docs/research/windows.typ).

## Decoding

**Linux**: FFmpeg (libavcodec, LGPL) with **hardware decoding** when the computer can (VA-API: AMD,
Intel; NVDEC: NVIDIA with its proprietary driver, not tested yet), FFmpeg's software decoder
otherwise, OpenH264 (BSD-2-Clause) as a last resort. Both are the system's libraries, loaded at run
time (`dlopen`), not linked: without FFmpeg Oculibre uses OpenH264, without either it still starts
and says what to install. Oculibre ships no H.264 code of its own (patents: see
[`docs/research/video-decoding.typ`](../docs/research/video-decoding.typ)).

**Windows**: Windows' own H.264 decoder (Media Foundation, `core/src/decoder_mf.rs`), on the
graphics card through Direct3D 11, in software in a virtual machine; OpenH264 if its DLL is
installed.

`--decoder auto|vaapi|nvdec|software|openh264`; the decoder in use is printed when the stream starts.

**Which decoder will this computer use, and why?**

```sh
cd desktop && cargo run --release -p oculibre-core --example decoder_check
```

To build: `sudo apt install build-essential pkg-config libavcodec-dev libavutil-dev libclang-dev`
(or `bash ../scripts/setup-dev.sh`); without FFmpeg at all: `cargo build --no-default-features`
(OpenH264 only). `--features oculibre-core/bundled-openh264` compiles OpenH264 in, for personal
builds only (not covered by Cisco's patent licence). AMD's own driver stack (`/opt/amdgpu`) is found
automatically; check VA-API with `vainfo`.

### Fedora (no package yet)

Fedora ships FFmpeg without H.264 (`ffmpeg-free`) and Mesa's VA-API driver without H.264 either:
Oculibre would then fall back to OpenH264, or find no decoder at all. Take the full versions from
[RPM Fusion](https://rpmfusion.org/Configuration), with the virtual camera module, then build
Oculibre from source:

```sh
sudo dnf install https://mirrors.rpmfusion.org/free/fedora/rpmfusion-free-release-$(rpm -E %fedora).noarch.rpm
sudo dnf swap ffmpeg-free ffmpeg --allowerasing                   # FFmpeg with H.264
sudo dnf swap mesa-va-drivers mesa-va-drivers-freeworld           # AMD/Intel hardware decoding
sudo dnf install v4l2loopback                                     # the virtual camera module
sudo dnf install gcc pkgconf-pkg-config clang-devel ffmpeg-devel  # to build
```

Then `cargo run --release -p oculibre-core --example decoder_check` should say
`Decoder: VA-API (hardware)` (or `FFmpeg (software)` without a graphics card).

## Use it (Linux, command line)

```sh
bash ../scripts/vcam-load.sh     # once per boot (or --persist)
cargo run --release              # in this folder
```

Then press **Start** in the phone app. The phone appears as a webcam named **Oculibre**
in Firefox, Chrome, Discord, Zoom, OBS… (it shows up once the first frame has arrived).

- USB (USB debugging on): `adb reverse` is done automatically, nothing to type on the phone.
- Wi-Fi, encrypted (TLS with pinned keys, see [`docs/research/secure-channel.typ`](../docs/research/secure-channel.typ)):
  - once per phone: `cargo run --release -- --pair` shows a QR code. Scan it from the Oculibre app or
    the phone's camera app (or, phone plugged in over USB, it is sent to the phone directly), check that
    the code shown on the phone is the one printed here, tap **Pair**, then **Start**;
  - then: `cargo run --release -- --wifi`, and **Start** in the app. The computer announces itself on
    the network (mDNS): if its address changes, the phone finds it again.
  - `oculibre pairings` lists the paired phones, `oculibre unpair <name>` forgets one. Without `--wifi`
    only USB (loopback) connections are accepted; plaintext is never accepted from the network.
- The phone can Stop/Start: the program keeps waiting for it.
- Rotation: by default (`--rotate auto`) the picture is upright however you hold the phone.
  The virtual camera stays landscape, like every webcam: a portrait picture gets black bars on the
  sides. `--portrait` makes it portrait instead. `--rotate 0|90|180|270` fixes the rotation;
  `--mirror` flips left ↔ right.
- An app that has the virtual camera open (OBS, ffplay, a browser tab) locks its size: Oculibre then
  fits every picture into that size (black bars, never stretched).
- `cargo run --release -- --help` for all options.

## Measure a phone (`oculibre bench`)

```sh
cargo run --release -- bench     # then Start in the app (any quality)
```

The PC switches the phone through every camera and quality it offers (about 15 s each), measures the
real frame rate, the latency (phone + network + PC), the bitrate and the dropped frames, prints a table
and writes it into `devices/<brand>/<model>/report.md` (found through its "Model" line; run
`python3 scripts/device-report.py` first for a new phone). USB and Wi-Fi (`--wifi`) results get
their own section, replaced at each run. `--seconds`, `--warmup`, `--report <path>`, `--no-report`.
Needs app 0.2 or newer.

## Windows

Build on Windows itself (the DLLs and the installer are Windows-only):

1. Install [Rust](https://rustup.rs) with the MSVC toolchain (it offers the Visual Studio Build
   Tools), [Node.js](https://nodejs.org) LTS, and Git.
2. `rustup target add i686-pc-windows-msvc` (the 32-bit DirectShow DLL, for 32-bit apps), and
   `cargo install --locked cargo-about --features cli` (the licences file, see below).
3. From `desktop/app`: `npm install`, then `npm run tauri dev` to run the app, or
   `npm run tauri build` for the installer (`target/release/bundle/nsis/Oculibre_<version>_x64-setup.exe`).
   The build downloads Google's `adb` (pinned version and checksum, `app/src-tauri/windows/fetch-adb.ps1`).

The installer (NSIS, `app/src-tauri/windows/installer-hooks.nsh`) registers both cameras, adds a
firewall rule for the app (phones connect on port 47100 and find it by mDNS) and ships `adb` for USB.
In development, register the cameras by hand from an administrator terminal:
- `regsvr32 target\release\oculibre_vcam_dshow.dll`, `regsvr32 target\release\oculibre_vcam_mf.dll`,
then `target\release\oculibre-camera.exe register` (`remove` undoes it).
- `cargo run -p oculibre-vcam-shm --example pattern` feeds them a moving test pattern without a phone;
- `cargo run -p oculibre-vcam-mf --example mf_check` reads the Media Foundation camera like an app
would (rate, size, one picture).

Binaries are linked statically to the C runtime (`.cargo/config.toml`): a fresh Windows needs no
Visual C++ redistributable.

## Packages (`.deb`)

```sh
bash ../scripts/build-deb.sh                 # Ubuntu 26.04, Ubuntu 24.04, Debian 13
bash ../scripts/build-deb.sh ubuntu-24.04    # one release
```

One package per release, each built in a Docker container of that release (needs Docker), then
installed on a clean container of the same release to check that its dependencies resolve and no
library is missing. Packages land in `target/deb/` (`Oculibre_<version>_<release>_amd64.deb`).
Why one per release: FFmpeg changes its binary interface at each major version (Ubuntu 24.04:
libavcodec 60, Debian 13: 61, Ubuntu 26.04: 62), and Oculibre loads the one it was built against.

| Release | Decoding recommended | Virtual camera module |
|---|---|---|
| Ubuntu 26.04 | `libavcodec62`, or `libopenh264-8` | signed package (`linux-modules-v4l2loopback-generic`), else DKMS |
| Ubuntu 24.04 | `libavcodec60` (its OpenH264 2.4 has another ABI) | signed, with the 7.0 kernel (HWE, the desktop default): its module package provides `v4l2loopback-dkms`; DKMS only on the 6.8 kernel (Secure Boot: key enrolled once) |
| Debian 13 | `libavcodec61`, or `libopenh264-8` | DKMS (`v4l2loopback-dkms` + `linux-headers-amd64`, which Debian's dkms does not pull): with Secure Boot, its key must be enrolled once (`mokutil --import /var/lib/dkms/mok.pub`, problem 403) |

No Flatpak, Snap or AppImage for now: none of them can bring the virtual camera's kernel module
(see [`docs/research/linux-packages.typ`](../docs/research/linux-packages.typ)).

For quick local tests, `cd app && npm run tauri build -- --bundles deb` builds for this computer only.

## Licences of the components

Each package ships `THIRD-PARTY-LICENSES.html`, which the About screen opens ("Show the licences"):
every crate built into the app for that platform, the Rust standard library, the npm packages of
the page and the fonts, each with its licence text. It is generated at build time, never committed:
`npm run licences` (in `app/`, runs `app/scripts/licences.mjs`) → `target/licences/`. It needs
[cargo-about](https://github.com/EmbarkStudios/cargo-about):
`cargo install --locked cargo-about --features cli`. Its settings: [`about.toml`](about.toml).

Which licences may enter at all is checked in CI by [cargo-deny](https://github.com/EmbarkStudios/cargo-deny)
(`cargo deny check licenses`, list in [`deny.toml`](deny.toml)): a new dependency under another licence
fails the pull request. Adding a licence to the list is a decision, not a fix.

## Develop

```sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt
```

`cargo test` runs a simulated phone against the real session code (handshake, frames, keyframe
requests, `BYE`) and the protocol test vectors: no phone needed.

After a deliberate wire-format change, regenerate the shared vectors:
`cargo run -p oculibre-protocol --example gen_vectors`.

Fuzzing (nightly toolchain): `cargo install cargo-fuzz`, then `cd protocol && cargo +nightly fuzz run decode`.
