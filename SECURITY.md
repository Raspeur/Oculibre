# Security policy

Oculibre streams a live camera feed: security issues are taken seriously.

## Reporting a vulnerability

**Please do not open a public issue.** Use GitHub's private vulnerability reporting ("Security" tab → "Report a vulnerability") on this repository.

You can expect a first answer within 7 days.

## Scope

- The wire protocol and its parser (`docs/protocol.typ`, `desktop/`)
- Pairing (QR code, one-time token) and the TLS channel of Wi-Fi connections (`docs/protocol.typ` §13)
- The Android app (permissions, exposed services)
- Virtual camera drivers/extensions

## Threat model (summary)

- Attacker on the same local network (shared flat, school or corporate Wi-Fi) who tries to watch the stream, inject frames, or crash the desktop client.
- Defences: plaintext only on loopback (USB through `adb reverse`); on any network, TLS 1.3 (or 1.2 with ECDHE + AEAD suites only) with mutual authentication by pinned self-signed keys, no certificate authority. The computer only accepts phones paired with a one-time token (128 bits, 10 minutes, single use) and checks the phone's `HELLO` before revealing anything about itself; the phone only talks to the computer whose key fingerprint it pinned from the QR code. The phone's private key stays in the Android Keystore; the computer's is in `~/.config/oculibre/tls-key.der` (mode 600).
- Known limits: with TLS 1.2 (Android 5–9) the certificates travel in clear, so an observer can recognise a phone across sessions (not see the video). A pairing link can be opened by any app or web page: the phone asks the user to confirm and shows the key fingerprint to compare with the computer's.
- Out of scope: an attacker with physical or root access to the phone or the computer.
