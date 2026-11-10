# Security policy

Oculibre streams a live camera feed: security issues are taken seriously.

## Reporting a vulnerability

**Please do not open a public issue.** Use GitHub's private vulnerability reporting ("Security" tab → "Report a vulnerability") on this repository.

You can expect a first answer within 7 days.

## Scope

- The wire protocol and its parser (`docs/protocol.md`, `desktop/`)
- Pairing and Noise handshake
- The Android app (permissions, exposed services)
- Virtual camera drivers/extensions

## Threat model (summary)

- Attacker on the same local network (shared flat, school or corporate Wi-Fi) who tries to watch the stream, inject frames, or crash the desktop client.
- Out of scope: an attacker with physical or root access to the phone or the computer.
