# Security policy

Oculibre streams a live camera feed: security issues are taken seriously.

## Reporting a vulnerability

**Please do not open a public issue.** Use GitHub's private vulnerability reporting ("Security" tab → "Report a vulnerability") on this repository.

We will give you an answer as fast as we can. Please give us 90 days to ship a fix before publishing
details; we credit reporters in the advisory unless you prefer otherwise.

## Supported versions

Oculibre is in beta: only the latest release gets security fixes.

## Scope

- The wire protocol and its parser (`docs/protocol.typ`, `desktop/`)
- Pairing (QR code, one-time token) and the TLS channel of Wi-Fi connections (`docs/protocol.typ` §13)
- The Android app (permissions, exposed services, the pairing link `oculibre://pair`)
- The virtual cameras: on Linux the root helper run through polkit (`vcam-setup`); on Windows the
  DirectShow DLL that every app using the camera loads into its own process, the Media Foundation
  source loaded by Windows' Frame Server service, and the shared memory they read (the global block
  is writable by logged-in users)
- The packages and installers (`.deb` maintainer scripts, the Windows installer and what it
  registers, the `adb` it ships)

## Threat model (summary)

- Attacker on the same local network (shared flat, school or corporate Wi-Fi) who tries to watch the stream, inject frames, or crash the desktop client.
- Defences: plaintext only on loopback (USB through `adb reverse`); on any network, TLS 1.3 (or 1.2 with ECDHE + AEAD suites only) with mutual authentication by pinned self-signed keys, no certificate authority. The computer only accepts phones paired with a one-time token (128 bits, 10 minutes, single use) and checks the phone's `HELLO` before revealing anything about itself; the phone only talks to the computer whose key fingerprint it pinned from the QR code. The phone's private key stays in the Android Keystore; the computer's is in `~/.config/oculibre/tls-key.der` (mode 600) on Linux, `%APPDATA%\Oculibre\tls-key.der` on Windows (the user's profile).
- Known limits: with TLS 1.2 (Android 5–9) the certificates travel in clear, so an observer can recognise a phone across sessions (not see the video). A pairing link can be opened by any app or web page: the phone asks the user to confirm and shows the key fingerprint to compare with the computer's.
- USB through `adb` needs USB debugging on the phone, which gives the computer wide access to it: the apps say so, and this way is temporary (Android Open Accessory, #21).
- Out of scope: an attacker with physical, administrator or root access to the phone or the computer; another program of the same user on the computer (it can already read the camera).
