#import "../oculibre.typ": *

#show: oculibre-doc.with(
  title: [Oculibre on Windows],
  subtitle: [Virtual camera, decoding, USB, installer and signing: the choices for \#17],
  revision: "0.1",
  date: datetime(year: 2026, month: 11, day: 17),
)

#remark(title: [Question])[What does Oculibre need on Windows 10 and 11 to be what it is on Linux: a phone that every
app sees as a webcam, installed without a terminal? Most streamers are on Windows. Notes as of
November 2026; points marked _to test_ need a real Windows computer.]

= Summary <summary>

- *Both Windows 10 and 11*: on Steam (October 2026) Windows 11 is 72.5 % and Windows 10 still
  21 %, a year after its end of support. Streamers and gamers overlap with this population.
- *Two virtual cameras, as planned in `ARCHITECTURE.typ`*:
  + a *DirectShow source filter*: works on Windows 10 and 11, seen by OBS, Zoom, Discord and
    Chromium browsers; not by the Windows Camera app;
  + a *Media Foundation virtual camera* (`MFCreateVirtualCamera`, Windows 11 only): a real
    camera for the system, seen by the Camera app and Media Foundation apps.

  Both are COM DLLs, *written in Rust*, that receive the pictures from Oculibre through *shared
  memory*. No kernel driver, so no driver signing.
- *Decoding*: Windows' own H.264 decoder (Media Foundation, D3D11 hardware decoding). It is
  licensed with Windows, so we ship no FFmpeg and avoid the patent question (`video-decoding.typ`).
- *Installer*: Tauri's NSIS or MSI installer, per machine (the cameras are registered in the
  system registry), firewall rule for Wi-Fi, `adb` included for USB.
- *Signing*: SignPath Foundation (free for open source) once the repository is public. Azure
  Artifact Signing is closed to individual developers in France.
- *Order*: the engine on Windows first (CLI, decoding), then the DirectShow camera, then the
  Media Foundation one, then the app and the installer.

= Who uses what <who>

#table(
  columns: (auto, auto, 1fr),
  table.header([Windows], [Steam, Oct. 2026], [What it means]),
  [11], [72.5 %], [both cameras available],
  [10], [21.1 %], [DirectShow camera only; no more security updates since October 2025],
)

The Steam survey is opt-in and its monthly swings can be large, but the trend is clear: Windows 10
cannot be dropped in v1.

= Virtual camera <vcam>

== DirectShow source filter (Windows 10 and 11)

- A COM DLL registered in the registry. Each app that opens the camera loads it *into its own
  process* (OBS, Zoom, Discord, Chrome…). The DLL reads the pictures that Oculibre writes in a
  shared memory block (same user session: `Local\` names are enough).
- *Softcam* (MIT, C++) does exactly this: the filter DLL plus a small "sender" API. It is the base
  named in `ARCHITECTURE.typ`. OBS's own virtual camera works the same way.
- *Seen by*: DirectShow apps, which means most desktop video apps and Chromium-based browsers (Chrome,
  Edge, Discord, Teams in the browser; _to test_: the new Teams, which is WebView2).
- *Not seen by*: the Windows Camera app and apps that use only Media Foundation.
- *32-bit apps* load only 32-bit DLLs: ship a 32-bit and a 64-bit filter (Softcam builds both).

== Media Foundation virtual camera (Windows 11)

- `MFCreateVirtualCamera` (Windows 11, build 22000+) registers a camera with the *Frame Server*.
  Apps then find it like a real camera. Our part is a COM *media source* DLL, plus the app that
  calls `MFCreateVirtualCamera` and `Start`.
- The DLL is not loaded by the apps but by two Windows services (Frame Server and Frame Server
  Monitor, in `svchost.exe`, account LocalService). Two consequences:
  + the DLL must be readable by those services (installed under `Program Files`, registered in
    `HKLM`), otherwise `Start` fails with "access denied";
  + the shared memory crosses a session boundary: names in `Global\`, with a security descriptor
    that lets both the service and the user in. A normal user cannot create `Global\` objects, so
    the *source creates them* and Oculibre opens them.
- Lifetime: `Session` (exists while Oculibre runs: what we want) or `System` (stays registered).
  Access: `CurrentUser` (no administrator) or `AllUsers`.
- Working samples: Microsoft's `Windows-Camera` samples, and `VCamSample` (smourier, C++, MIT), which
  serves NV12 and RGB32 from the GPU when the app gives a Direct3D device, and from the CPU otherwise.
- _To test_: whether DirectShow apps (OBS, Zoom) also see a Media Foundation virtual camera. If
  they do, on Windows 11 the MF camera alone could be enough. If they don't, register both, and
  check that no app shows "Oculibre" twice.

== Measured in the VM (2026-11-18) <measured>

- *The Media Foundation camera works*, in Rust (`vcam-mf`): an `IMFActivate` the Frame Server
  creates, a live media source with one NV12 stream (1920×1080, 1280×720 at 30/60, 640×360), samples
  from the Frame Server's allocator (memory, no Direct3D), the waiting picture without Oculibre.
  30 pictures per second through the Frame Server.
- *Windows names it "Oculibre (Caméra virtuelle Windows)"*: it adds that suffix (in the system's
  language) to every virtual camera.
- *DirectShow apps see it too* (FFmpeg's `dshow` input read it at 30 per second, as YUY2: Windows
  converts). So on Windows 11 the Media Foundation camera alone would reach every app.
- *But one app at a time*: with two apps on it, only one gets pictures. Windows 11 lets several apps
  share a camera only if the user turns "Allow multiple apps to use the camera" on. The DirectShow
  filter has no such limit (each app loads its own), which matters for streamers (OBS + Discord).
- *Lifetime "system"* keeps the camera registered after the program that made it ends, and after
  reboots: the installer registers it once for every user (`oculibre-camera register`, access "all
  users", as administrator); Oculibre itself no longer registers anything.
- *Decision*: Windows 11 gets both cameras: "Oculibre" (DirectShow: OBS, Discord, Zoom, Chrome,
  several at once) and "Oculibre (Caméra virtuelle Windows)" (Media Foundation: the Camera app and
  apps that see only it). Windows 10: DirectShow only.
- The Media Foundation block is `Global\OculibreVirtualCameraMF`, made by the source (in the
  service, which may create global objects) with a security descriptor that lets logged-in users
  write; Oculibre opens it once it exists.

== What Oculibre writes

A shared memory block that we define ourselves, read by both DLLs:

- a header: magic, version, width, height, format, frame counter, timestamp;
- the picture in *NV12*, the native format of Media Foundation and of the hardware decoder. The
  DirectShow filter converts to what the app asks for (YUY2/RGB) if needed;
- a named event signalled at each new picture;
- "no picture yet": the camera shows a still picture ("Waiting for the phone"), so that apps do
  not freeze or drop the camera while the phone is away (same rule as on Linux: the camera keeps
  one shape and never disappears).

On the Rust side this is a new `Sink` next to `vcam.rs` (v4l2). It reuses the rotation/fit code
of `yuv.rs` and writes NV12.

== Which language for the DLLs <language>

*Decided (2026-11-18): Rust*, with the `windows` crate (Microsoft's own bindings), whose
`#[implement]` macro writes the COM plumbing (reference counting, `QueryInterface`). One language
and one toolchain (Cargo) for the whole receiver, and the shared memory format is defined once.

- The DLLs are crates of the workspace with `crate-type = ["cdylib"]` exporting `DllGetClassObject`,
  `DllCanUnloadNow`, `DllRegisterServer`, `DllUnregisterServer`.
- Softcam (DirectShow) and VCamSample (Media Foundation) are the *references* we read, not code we
  vendor. If we port parts of them, their MIT notices go with those parts.
- Care points:
  - a panic must never unwind across a COM call. Every exported method catches it and returns an
    `HRESULT`, because a crash takes down the app that loaded us (DirectShow) or the Frame Server
    service (Media Foundation);
  - no Rust runtime cost: no async, no threads that outlive the object, no allocation in the hot
    path (frames are copied from the shared memory into the app's buffer);
  - the 32-bit DirectShow DLL: target `i686-pc-windows-msvc`;
  - DirectShow has no Rust "base classes": the filter, its output pin and the `IAMStreamConfig`
    format negotiation are written by hand, following Softcam.

= Decoding <decoding>

- *Media Foundation H.264 decoder* (an MFT, Windows 7+), with *D3D11 hardware decoding* (DXVA) on
  AMD, Intel and NVIDIA through the same API, through the `windows` crate. Output NV12, which is what
  the cameras want: no conversion on the way.
- *Windows "N" editions* (Europe, without media): no H.264 decoder until the user installs the
  Media Feature Pack. Oculibre then explains it (new problem code). Fallback: Cisco's OpenH264
  DLL, downloaded at first use with consent (Cisco pays the patent licence for its own binaries;
  `openh264` crate, `libloading`).
- Not on Windows: FFmpeg (we would have to ship and license it, `video-decoding.typ`).

= USB, network, startup <system>

- *USB*: `adb.exe` (Android platform-tools) shipped with Oculibre, as scrcpy does. _To check_: the
  terms for redistributing Google's binaries. Many phones also need their maker's USB driver
  (Samsung, Google's for Pixels). Xiaomi phones often work with Windows' built-in WinUSB. Oculibre
  must tell "no driver" apart from "USB debugging off" (problem codes). \#21 (USB without developer
  mode) would need WinUSB/libusb anyway.
- *Wi-Fi*: Windows Defender Firewall blocks incoming connections on 47100. The installer adds a
  rule for private networks only. The first `--pair` on a "public" network must say why the phone
  cannot connect.
- *mDNS*: `mdns-sd` works on Windows. _To test_: coexistence with Windows' own mDNS responder on
  port 5353.
- *Tray and autostart*: Tauri does both on Windows (notification area, `Run` registry key).
- *Single instance*: already done (plugin, named pipe on Windows).

= Installer and signing <installer>

- Tauri builds *NSIS* (`.exe`) or *WiX* (`.msi`) installers and brings the WebView2 runtime if
  missing. *Per-machine* install (administrator once): the DLLs go to `Program Files`, the
  DirectShow filters are registered in `HKLM` (`regsvr32` 32 and 64 bit), the firewall rule is added.
  Uninstall removes all of it.
- *Code signing*: without it, SmartScreen shows "Windows protected your PC" and many users stop
  there.
  - *SignPath Foundation*: free for open source projects (OSI licence, MFA for every member). The
    certificate names SignPath Foundation as publisher. Needs the *public* repository: plan it for
    when the repo goes public.
  - *Azure Artifact Signing*: public certificates for individual developers only in the USA and
    Canada (organisations: also EU, UK).
  - Before that: unsigned test builds for us and testers.

= How we develop and test it <dev>

- *Build*: everything is Rust. On Windows: the MSVC toolchain (Visual Studio Build Tools); in CI:
  GitHub Actions `windows-latest`. From Linux, `cargo xwin` can cross-build too.
- *Development VM*: `win11-dev` (libvirt user session: UEFI + Secure Boot, TPM 2.0, 8 GB, 6 CPUs,
  SATA disk, network through passt with host port 2222 → guest SSH 22), driven from Linux over SSH.
- *Test*: a Windows 11 VM (virt-manager: UEFI, Secure Boot, TPM through `swtpm`) for the cameras,
  the installer and Wi-Fi/Tailscale. There is no GPU in the VM, so decoding is software there.
  Plus a real PC (the one with the RTX 2060) for hardware decoding, NVDEC/D3D11, and USB phones.
  And a Windows 10 VM for the DirectShow-only path.
- *Apps to try*: OBS, Discord, Zoom, Teams (new), Chrome/Edge (Meet), Firefox, the Camera app,
  Skype if still around.

= Plan <plan>

+ *Engine on Windows*: `oculibre-core` and the CLI build and run on Windows (TLS, mDNS, sessions,
  `adb` path). New `Sink` trait implementations: a file/preview sink first. Decoder: Media
  Foundation (software, then D3D11).
+ *DirectShow camera*: Rust COM DLL (following Softcam), shared memory format and its writer. Seen by OBS, Discord,
  Chrome on Windows 10 and 11.
+ *Media Foundation camera* (Windows 11): Rust media source (following VCamSample), `MFCreateVirtualCamera` from the app,
  the Camera app and the new Teams.
+ *Desktop app*: Windows-specific problems (camera not registered, N edition, firewall, USB
  driver), tray, autostart.
+ *Installer*, firewall rule, `adb`; then test the matrix (Windows 10/11 × apps); signing when
  public.

= Sources <sources>

- `IMFVirtualCamera`, `MFCreateVirtualCamera`:
  #link("https://learn.microsoft.com/windows/win32/api/mfvirtualcamera/nn-mfvirtualcamera-imfvirtualcamera")
- VCamSample (Windows 11 virtual camera sample): #link("https://github.com/smourier/VCamSample")
- Softcam (DirectShow virtual camera library): #link("https://github.com/tshino/softcam")
- Microsoft Windows-Camera samples: #link("https://github.com/microsoft/Windows-Camera")
- Steam survey October 2026 (Windows 11 72.51 %, Windows 10 21.11 %):
  #link("https://www.digitalcitizen.life/linux-gains-steam-market-share-as-october-2026-hardware-survey-shows-major-changes/")
- SignPath Foundation: #link("https://signpath.org/")
- Azure Artifact Signing availability: #link("https://learn.microsoft.com/azure/artifact-signing/quickstart")
