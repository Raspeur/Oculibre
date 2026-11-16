#import "../oculibre.typ": *

#show: oculibre-doc.with(
  title: [Linux package formats],
  subtitle: [Why Oculibre ships a `.deb` per release, and not (yet) a Flatpak, a Snap or an AppImage],
  revision: "1.0",
  date: datetime(year: 2026, month: 11, day: 16),
)

#remark(title: [Question])[Issue \#18 asks that anyone can install Oculibre on Linux without a terminal. Which package
formats can deliver *a working webcam*, not only a window that starts? Notes as of Ubuntu 26.04,
Flatpak 1.16 and Tauri 2.]

= Summary <summary>

- *What a format must bring*, besides the app itself:
  + the `v4l2loopback` kernel module, set up and loaded at each boot (the virtual camera);
  + a way to fix it from the app (root helper + polkit action, problem codes 401–405);
  + an H.264 decoder, with licences and patents we accept (`video-decoding.typ`);
  + `adb`, for phones over USB.
- *Distribution packages (`.deb`, later `.rpm`) are the only format that brings all four*: the
  package manager installs the module (signed on Ubuntu 26.04, DKMS elsewhere), our files go in
  `/usr/lib` and `/usr/share/polkit-1`, FFmpeg and OpenH264 stay the distribution's.
- *Flatpak and Snap cannot load a kernel module* and cannot ship a polkit action: the camera would
  still need the terminal, which is what \#18 removes.
- *AppImage* runs outside any sandbox but installs nothing: no module, no helper, and it would have
  to bundle FFmpeg (licence and patents) and WebKitGTK (fragile).
- *Decision (2026-11-16)*: `.deb` per release (Ubuntu 26.04, Ubuntu 24.04, Debian 13,
  `scripts/build-deb.sh`); Fedora builds from source with RPM Fusion (`desktop/README.md`). Look
  again at Flatpak when the virtual camera no longer needs a kernel module (PipeWire, below).

= Formats compared <compared>

#table(
  columns: (auto, 1fr, 1fr, 1fr, 1fr),
  table.header([], [`.deb` per release], [Flatpak], [Snap], [AppImage]),
  [Virtual camera module], [#done dependency (signed or DKMS)], [#fail outside the sandbox: the user installs it],
  [#fail same], [#fail same],
  [One-click fix (polkit)], [#done helper + action in `/usr`], [#fail no polkit action, no `pkexec` in the sandbox],
  [#fail same], [#open generic `pkexec` prompt, script on a user path],
  [H.264 decoding], [#done the system's FFmpeg / OpenH264], [#open runtime codecs extension (to check)],
  [#open must bundle], [#fail must bundle FFmpeg],
  [`adb` (USB)], [#done recommended package], [#open bundled, `--device=all`], [#open bundled, interfaces],
  [#open bundled or host's],
  [Updates], [#open none until we have an apt repository], [#done Flathub], [#done Snap Store],
  [#fail manual],
  [Distributions], [Ubuntu, Debian (one build each)], [all], [mostly Ubuntu], [most, if WebKitGTK cooperates],
)

= Flatpak <flatpak>

- *Kernel module*: a Flatpak cannot install or load one. `v4l2loopback` would have to come from the
  host's package manager, by hand, and the module options (`exclusive_caps=1`, label) be written in
  `/etc/modprobe.d` by hand too.
- *One-click fix*: `pkexec` and our polkit action live on the host. Reaching the host from the
  sandbox (`flatpak-spawn --host`) needs the `org.freedesktop.Flatpak` permission, which escapes the
  sandbox entirely; Flathub refuses it except for development tools.
- *The camera device*: writing to `/dev/video*` needs `--device=all` (also needed by `adb` for USB).
  Accepted on Flathub for camera apps, but the sandbox then protects little.
- *Decoding*: the Freedesktop runtime ships FFmpeg without H.264; H.264 comes from an extension
  (`org.freedesktop.Platform.codecs-extra`). To check: which decoders it really enables, and whether
  its libavcodec major matches what we load.
- *Where Flatpak becomes right*: if the virtual camera is a *PipeWire* video node instead of a kernel
  module (the "Later: PipeWire camera" task of \#18), the sandbox only needs the PipeWire socket. No
  module, no root, no polkit: then a Flatpak on Flathub reaches every distribution with updates.
  Limit to measure first: which apps see PipeWire cameras (Firefox, Chrome, OBS do through the camera
  portal or a flag; older Zoom, Discord, Skype only see `/dev/video*`).

= Snap <snap>

Same wall as Flatpak: a strictly confined snap cannot load kernel modules or ship a polkit action;
the `camera` interface is for reading cameras, not creating one. A classic (unconfined) snap needs a
manual review by Canonical and still cannot pull the module as a dependency. Mostly Ubuntu users,
who already get the `.deb`.

= AppImage <appimage>

- *Nothing installed*: an AppImage is one file the user runs. It cannot depend on
  `linux-modules-v4l2loopback-generic` or `v4l2loopback-dkms`, so the module is the user's job.
- *One-click fix*: possible only half way: `pkexec` on a script inside the mounted AppImage gives
  the generic "authentication is needed to run `/tmp/.mount_…`" prompt, not our explained, translated
  action, and still cannot install a missing module.
- *Decoding*: the host's libavcodec has a different major version on each release, and ours is fixed
  at build time (ABI, see `video-decoding.typ`). So an AppImage must bundle FFmpeg: our own minimal
  LGPL build, its licence and source, and Oculibre then distributes an H.264 decoder (patents). Or
  OpenH264 only, from Cisco's binary downloaded at first use.
- *WebKitGTK*: Tauri's AppImage bundles WebKitGTK and its dependencies (≈ 80 MB) and is known to
  break on releases whose Mesa or glibc differ from the build machine's (blank window, EGL errors).

= What it means for users <users>

- *Ubuntu 26.04*: open the `.deb` (App Center, or `sudo apt install ./Oculibre_….deb`), nothing else. Tested on a clean VM.
- *Ubuntu 24.04, Debian 13*: same, plus a key enrolment at the next boot when Secure Boot is on
  (DKMS, problem 403).
- *Fedora, Arch, others*: build from source (`desktop/README.md`); an `.rpm` and an AUR package are
  easy additions later, the hard parts (module, helper, polkit) being the same files.
- *Updates*: the `.deb` does not update itself. Later: an apt repository (signed), or a PPA for
  Ubuntu, so that new versions arrive with the system's updates.
