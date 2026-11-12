#import "../oculibre.typ": *

#show: oculibre-doc.with(
  title: [Decoding H.264 on the computer],
  subtitle: [FFmpeg, libavcodec and OpenH264: linking, licences, patents and packaging],
  revision: "1.0",
  date: datetime(year: 2026, month: 11, day: 12),
)

#remark(title: [Question])[The receiver decodes the phone's H.264 video with FFmpeg (hardware when it can) and keeps
OpenH264 as a last resort (\#11). What does each choice mean for the packages we will ship (\#17,
\#18, \#19), for the licence of Oculibre (MIT) and for patents? Notes as of FFmpeg 8 on Ubuntu 26.04;
not legal advice.]

= Summary <summary>

- *Today, FFmpeg is not inside Oculibre*: the binary (5 MB) is linked to the system's
  `libavcodec.so.62` and `libavutil.so.60`. OpenH264 *is* inside: it is compiled from source into the
  binary.
- *Consequence 1, it does not start without FFmpeg*: if `libavcodec` is missing, or has another
  major version, the program cannot even start, although OpenH264 would be enough to work.
- *Consequence 2, one build per FFmpeg major version*: a binary built against FFmpeg 8
  (`libavcodec62`) does not run with FFmpeg 6 (Ubuntu 24.04) or 7 (Debian 13).
- *Licences*: Oculibre stays MIT. Distribution packages that *depend* on the system's FFmpeg
  redistribute nothing. A package that *bundles* FFmpeg (AppImage, Windows) must ship an LGPL build of
  our own with its licence and source, not the GPL build of the distributions.
- *Patents*: H.264 is covered by patents (Via LA pool). Cisco pays the royalties only for *its own*
  OpenH264 binaries, not for a copy compiled from source like ours. Windows and macOS ship a licensed
  decoder: use it there.

= What we use today <today>

#table(
  columns: (1fr, 2fr, 1.3fr, 0.9fr),
  table.header([Decoder], [How it reaches the binary], [Licence], [In our binary?]),
  [FFmpeg (`libavcodec`): VA-API, NVDEC, software],
  [`ffmpeg-sys-next` 8: Rust declarations generated at build time by `bindgen` from the installed
   headers (hence `libavcodec-dev`, `libclang-dev`), *dynamic link* to `libavcodec.so.62` found by
   `pkg-config`],
  [LGPL-2.1+, *GPL-2+* as built by Debian/Ubuntu (`--enable-gpl`, `--enable-libx264`)],
  [No: loaded from `/usr/lib` at start],
  [OpenH264 (Cisco)],
  [`openh264` 0.9 crate, feature `source` (default): the C++ sources are compiled and linked
   *statically* (hence `libstdc++` at run time)],
  [BSD-2-Clause],
  [Yes],
)

Choice order (`--decoder auto`): VA-API (AMD, Intel), NVDEC (NVIDIA), FFmpeg software, OpenH264.
Measured on a 4K stream (\#11): OpenH264 10.6 ms per frame, FFmpeg software 5.0 ms, VA-API 5.2 ms
including the copy back to memory, with less CPU. FFmpeg is the default because of speed, not
because the receiver needs it: OpenH264 works for 1080p.

`ldd` on the release binary lists about 90 libraries, nearly all pulled by the distribution's
`libavcodec` (x264, x265, dav1d, libvpx, cairo…), not by Oculibre.

= Licences <licences>

- *FFmpeg* is LGPL-2.1-or-later by default and becomes GPL-2-or-later when built with
  `--enable-gpl`, which Debian and Ubuntu do (verified with `ffmpeg -buildconf`).
- *Linking dynamically to a library we don't ship* (a `.deb` with `Depends: libavcodec62`): we
  distribute only our MIT binary and source. MIT is compatible with both the LGPL and the GPL, so this is
  what every free player packaged by a distribution does.
- *Shipping FFmpeg with Oculibre* (AppImage, Flatpak without the runtime's codecs, Windows): we
  become a distributor of FFmpeg. Plan for:
  - our own *minimal LGPL build*: H.264 decoder, VA-API / NVDEC / D3D11 hardware decoding only,
    no `--enable-gpl`, no x264;
  - kept as a *separate shared library* that the user can replace (LGPL §6);
  - its licence text and the exact source (or a written offer) shipped with the package.
- *OpenH264* (BSD-2-Clause): keep the copyright notice in the "licences of the components"
  screen of the app (mockup `Settings`).

= Patents <patents>

- H.264 decoders are covered by patents licensed through a pool (Via LA, formerly MPEG LA). Linux
  distributions ship FFmpeg's H.264 decoder anyway, with different policies.
- *Cisco OpenH264*: Cisco pays the pool for the binaries *it builds and distributes*. A copy compiled
  from source (what the `source` feature does) is *not* covered. Firefox, for this reason, downloads
  Cisco's binary at first use. The `openh264` crate can do the same (feature `libloading`: loads
  Cisco's library at run time instead of compiling it in).
- *Hardware decoding* (VA-API, NVDEC) runs on the GPU vendor's decoder, but the driver packages
  still matter. To check: Fedora disabled H.264 in its Mesa VA-API driver and ships `ffmpeg-free`
  without some patented codecs; Oculibre must still work there (OpenH264).
- *Windows and macOS* ship an H.264 decoder licensed by Microsoft (Media Foundation, with the "N"
  editions needing the Media Feature Pack) and Apple (VideoToolbox). Using them avoids both shipping
  FFmpeg and the patent question.
- The phone side has the same question for *encoding*: it uses the phone's hardware encoder
  (`MediaCodec`), licensed with the device.

= Versions and ABI <abi>

FFmpeg breaks its binary interface at each major version (the number in `libavcodec.so.N`):

#table(
  columns: (auto, auto, auto),
  table.header([FFmpeg], [`libavcodec`], [Shipped by (examples)]),
  [6.x], [`.so.60`], [Ubuntu 24.04 LTS],
  [7.x], [`.so.61`], [Debian 13],
  [8.x], [`.so.62`], [Ubuntu 26.04 LTS (this computer)],
)

- `bindgen` writes the structure layouts (`AVFrame`, `AVCodecContext`…) of the version present at
  build time into our binary. Using another major version at run time would read the wrong fields:
  *never load a major version other than the one we were built for*.
- So a Linux package is built *per distribution release*, or bundles its own FFmpeg (AppImage).

= Proposals <proposals>

+ *Start without FFmpeg (\#18)*: load `libavcodec` ourselves at start (`dlopen`, only the major
  version we were built against) instead of linking it. Missing or another version: fall back to
  OpenH264 and tell the user, with the problem code of the mockups (`502`, "decoding by the
  processor"). The `.deb` then only *recommends* `libavcodec` instead of requiring it.
+ *Linux packages (\#18)*: `.deb` per Ubuntu/Debian release, linked to the system FFmpeg; AppImage or
  Flatpak with our minimal LGPL FFmpeg (or the Flatpak runtime's codecs extension, to check).
+ *Windows (\#17)*: decode with the OS (Media Foundation, D3D11 hardware decoding) first; FFmpeg
  LGPL bundle only if that falls short. Same idea with VideoToolbox on macOS (\#19).
+ *OpenH264*: keep compiling it in while developing; before publishing binaries, decide between
  the compiled copy and Cisco's binary loaded at run time (`libloading`), Firefox style.
+ *About screen*: list FFmpeg (LGPL or GPL, as found or shipped) and OpenH264 (BSD-2) with their
  notices.

= Sources <sources>

- FFmpeg licence and legal notes: #link("https://ffmpeg.org/legal.html")
- Cisco OpenH264 and its binary licence: #link("https://www.openh264.org/BINARY_LICENSE.txt"),
  #link("https://github.com/cisco/openh264")
- `openh264` crate (features `source`, `libloading`): #link("https://crates.io/crates/openh264")
- `ffmpeg-sys-next`: #link("https://crates.io/crates/ffmpeg-sys-next")
- Media Foundation H.264 decoder:
  #link("https://learn.microsoft.com/windows/win32/medfound/h-264-video-decoder")
- On this computer: `ldd target/release/oculibre`, `ffmpeg -buildconf`, `dpkg -l | grep libavcodec`.
