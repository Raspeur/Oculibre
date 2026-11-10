# Oculibre wire protocol — v0 (DRAFT)

> **Status: draft, not implemented.** Everything here may change until v1. Breaking changes bump `PROTOCOL_VERSION`.

The **phone** is the client (it connects), the **desktop** is the server (it listens). This works the same for Wi-Fi, USB tethering and USB via ADB (`adb reverse tcp:47100 tcp:47100` lets the phone reach the desktop through the cable; supported since Android 5.0).

- Default TCP port: `47100` (to be confirmed)
- mDNS service type: `_oculibre._tcp`
- All integers are **big-endian**.

## 1. Pairing (QR code)

The desktop displays a QR code containing a URI:

```
oculibre://pair?v=0&h=<host>&p=<port>&k=<base64url desktop static public key>&t=<base64url 16-byte pairing token>
```

- `k`: the desktop's long-term X25519 public key.
- `t`: a one-time token, valid ~2 minutes, proving the phone actually scanned the screen.
- After a successful pairing, both sides store each other's static public key. Later connections need no QR.

## 2. Handshake (encryption)

Pattern: **`Noise_IK_25519_ChaChaPoly_BLAKE2s`**

- `IK` because the phone (initiator) already knows the desktop's static key from the QR code.
- The first handshake payload from the phone carries `HELLO` (see below) and, on first pairing, the token `t`.
- The desktop rejects unknown phone keys unless a valid pairing token is presented.
- ChaCha20-Poly1305 is chosen because old ARMv7 CPUs have no AES acceleration.

**Unencrypted mode** is allowed only over USB (`adb`) and must be explicitly enabled on the desktop. The framing below is identical in both modes.

Noise transport messages are limited to 65 535 bytes: in encrypted mode, each frame is split into chunks, each sent as `u16 length || ciphertext`.

## 3. Framing

Every message starts with a fixed 16-byte header:

| Offset | Size | Field | Description |
|---|---|---|---|
| 0 | 1 | `type` | Message type (see §4) |
| 1 | 1 | `flags` | Bit field (see below) |
| 2 | 2 | `reserved` | Must be 0 |
| 4 | 4 | `length` | Payload length in bytes (max 16 MiB) |
| 8 | 8 | `pts_us` | Presentation timestamp in µs (sender's monotonic clock), 0 if N/A |

Flags:

| Bit | Name | Meaning |
|---|---|---|
| 0 | `KEYFRAME` | Video frame is an IDR |
| 1 | `CONFIG` | Payload is codec config (SPS/PPS) |
| 2–7 | — | Reserved, must be 0 |

## 4. Message types

| Value | Name | Direction | Payload |
|---|---|---|---|
| `0x01` | `HELLO` | phone → desktop | CBOR: `{proto: 0, app: "x.y.z", device: "model", android: 21, cameras: [...]}` |
| `0x02` | `HELLO_ACK` | desktop → phone | CBOR: `{proto: 0, accepted: bool, reason?: str}` |
| `0x10` | `VIDEO` | phone → desktop | H.264 Annex B access unit (one full frame, start codes included) |
| `0x20` | `CONTROL` | desktop → phone | CBOR: `{id: u32, cmd: str, args: {...}}` |
| `0x21` | `CONTROL_RESULT` | phone → desktop | CBOR: `{id: u32, ok: bool, error?: str}` |
| `0x30` | `STATS` | both | CBOR: fps, bitrate, dropped frames, temperature, thermal status |
| `0x40` | `PING` | both | 8-byte nonce |
| `0x41` | `PONG` | both | Same 8-byte nonce |
| `0xFF` | `BYE` | both | CBOR: `{reason: str}` |

Unknown types must be **ignored** (skip `length` bytes), so that minor additions stay backward compatible.

## 5. Control commands (`CONTROL.cmd`)

| `cmd` | `args` |
|---|---|
| `request_keyframe` | — |
| `set_stream` | `{width, height, fps, bitrate}` |
| `set_bitrate` | `{bitrate}` |
| `select_camera` | `{id}` (from `HELLO.cameras`) |
| `set_zoom` | `{ratio}` |
| `focus` | `{mode: "auto"\|"continuous"\|"manual", x?, y?, distance?}` |
| `set_exposure` | `{compensation}` |
| `set_torch` | `{on: bool}` |

The phone may refuse any command it does not support (`ok: false`).

## 6. Rules

- On connection (after `HELLO_ACK`), the phone sends a `CONFIG` frame then a `KEYFRAME`.
- **Drop policy (sender):** if the socket send buffer is saturated, drop non-key frames until the next keyframe and request one from the encoder. Latency must never accumulate.
- The desktop sends `request_keyframe` after any decode error.
- Either side closes the connection if no message is received for 5 s; `PING` is sent every 1 s when idle.
- The desktop must validate every length and field before use; the parser is fuzzed.

## Open questions

- Keep the phone as the client everywhere (`adb reverse`), or let the desktop connect over USB (`adb forward`)?
- Do we need a separate UDP/SRT channel for Wi-Fi, or is TCP + drop policy enough?
- Audio message type (`0x11`?) once audio is in scope.
