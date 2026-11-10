#import "oculibre.typ": *

#show: oculibre-doc.with(
  title: [Oculibre protocol],
  subtitle: [Wire format between the phone (camera) and the computer (host) — version 0.1],
  revision: "0.1",
  date: datetime(year: 2026, month: 11, day: 10),
  show-toc: 1,
)

#remark(title: [Status])[Specification for the first implementation. This document is the contract between
the phone app and the desktop programs: any change to the wire format updates it in the same pull
request.]

The key words MUST, MUST NOT, SHOULD and MAY are used as in RFC 2119.

= Goals <goals>

- *Low latency first.* One byte stream, length-prefixed messages, no buffering layer.
- *Forward compatible.* Phone apps and desktop programs are released separately (F-Droid, GitHub): a
  newer peer must keep working with an older one whenever possible.
- *Transport-agnostic.* The same messages run over USB (ADB), Wi-Fi, USB tethering and, later, other
  transports. Every message is self-contained so it can be mapped onto datagrams one day.
- *Secure by design.* Encryption is a layer _below_ the messages, never mixed into them.
- *Simple to implement* on Android without any library: binary for media, JSON for everything else
  (Android ships `org.json`).

= Roles and connections <roles>

#table(
  columns: (auto, auto, 1fr),
  table.header([Role], [Who], [Sends]),
  [*camera*], [the phone], [media, events about itself, answers to commands],
  [*host*], [the computer], [commands, events about the reception],
)

The role is *independent of who opens the connection*. Either side MAY dial:

#table(
  columns: (auto, auto, 1fr, auto),
  table.header([Mode], [Who dials], [Address], [Status]),
  [USB via ADB], [camera → host], [`127.0.0.1:47100` on the phone, mapped by `adb reverse tcp:47100 tcp:47100`], [0.1],
  [Wi-Fi, camera dials], [camera → host], [host IP, port *47100*], [0.1 (see @security)],
  [Wi-Fi / station mode, host dials], [host → camera], [camera IP, port *47101*], [reserved],
)

Reserved mDNS service types: `_oculibre-host._tcp` (a host listening on 47100) and `_oculibre-cam._tcp`
(a camera listening on 47101).

A host MAY serve several cameras at the same time (one virtual camera each). A camera serves *one host
at a time* and answers a second connection with `BYE {"reason": "busy"}`.

= Layers <layers>

```
┌─────────────────────────────────────────────────────────────┐
│ Messages: 16-byte header + payload                          │
├─────────────────────────────────────────────────────────────┤
│ Preamble: "OCUL" + version + role, once per connection      │
├─────────────────────────────────────────────────────────────┤
│ Secure channel: none on loopback in 0.1,                    │
│ mandatory on any network (Wi-Fi, tethering) from 0.2        │
├─────────────────────────────────────────────────────────────┤
│ Byte stream: TCP (over ADB, Wi-Fi, USB tethering…)          │
└─────────────────────────────────────────────────────────────┘
```

All integers are *unsigned and big-endian* unless stated otherwise. All text is *UTF-8*.

= Preamble <preamble>

Right after the connection (and the secure channel, when there is one) is established, *each side*
sends 8 bytes, without waiting for the other:

#table(
  columns: (auto, auto, auto, 1fr),
  table.header([Offset], [Size], [Field], [Value]),
  [0], [4], [magic], [ASCII `OCUL` (`4F 43 55 4C`)],
  [4], [1], [major], [protocol major version (`0`)],
  [5], [1], [minor], [protocol minor version (`1`)],
  [6], [1], [role], [`1` = camera, `2` = host],
  [7], [1], [reserved], [`0`],
)

Then each side reads the peer's preamble and:
- wrong magic → close the connection (not an Oculibre peer);
- same role on both sides → close;
- *different major* → close, and tell the user which side to update (both sides know both versions);
- same major, different minor → continue; the session uses the *features* both sides announce
  (@hello).

= Message header <header>

Every message is a 16-byte header followed by `length` bytes of payload.

#table(
  columns: (auto, auto, auto, 1fr),
  table.header([Offset], [Size], [Field], [Description]),
  [0], [1], [`type`], [message type (@types)],
  [1], [1], [`flags`], [meaning depends on the type; unused bits MUST be 0 and MUST be ignored],
  [2], [1], [`stream`], [`0` = session/control, `1` = main video, `2` = audio (reserved), `3–255` reserved],
  [3], [1], [reserved], [MUST be 0, MUST be ignored],
  [4], [4], [`length`], [payload length in bytes (max: @limits)],
  [8], [8], [`timestamp`], [microseconds on the *camera clock* (@clock) for media; sender's clock for others; `0` = none],
)

= Message types <types>

#table(
  columns: (auto, auto, 1fr),
  table.header([Range], [Family], [Payload]),
  [`0x00–0x1F`], [session], [JSON or fixed binary],
  [`0x20–0x3F`], [media], [JSON (configs) or binary (frames)],
  [`0x40–0x5F`], [control], [JSON],
  [`0x60–0xEF`], [reserved for future versions], [—],
  [`0xF0–0xFF`], [experimental / debugging, never in releases], [any],
)

*Unknown types MUST be skipped* (read and discard `length` bytes). A sender MUST NOT send an optional
type unless the peer announced the matching feature (@hello).

#table(
  columns: (auto, auto, auto, auto, auto),
  table.header([Type], [Name], [Direction], [Payload], [Since]),
  [`0x01`], [`HELLO`], [both], [JSON], [0.1],
  [`0x02`], [`BYE`], [both], [JSON], [0.1],
  [`0x03`], [`PING`], [both], [binary, 16 B], [0.1],
  [`0x04`], [`PONG`], [both], [binary, 32 B], [0.1],
  [`0x20`], [`STREAM_CONFIG`], [camera → host], [JSON], [0.1],
  [`0x21`], [`VIDEO_FRAME`], [camera → host], [binary], [0.1],
  [`0x22`], [`AUDIO_FRAME`], [camera → host], [binary], [reserved],
  [`0x23`], [`STREAM_END`], [camera → host], [JSON], [0.1],
  [`0x40`], [`REQUEST`], [both], [JSON], [0.1],
  [`0x41`], [`RESPONSE`], [both], [JSON], [0.1],
  [`0x42`], [`EVENT`], [both], [JSON], [0.1],
)

JSON payloads are a single UTF-8 JSON object. *Unknown keys MUST be ignored.* Missing optional keys take
the documented default.

= Session messages <session>

== `HELLO` (0x01) <hello>

The first message each side sends after the preamble; nothing else may be sent before it. A side that
has not received `HELLO` within 5 s closes the connection.

Camera:

```json
{
  "app": "oculibre-android",
  "app_version": "0.2.0",
  "id": "5b0d7f0e-3f1c-4a8e-9f7d-0c2b2f6f1a11",
  "name": "Redmi Note 9 Pro",
  "device": { "manufacturer": "Xiaomi", "model": "Redmi Note 9 Pro",
              "os": "android", "os_version": "12", "sdk": 31 },
  "features": ["video.h264", "clock", "orientation", "stats",
               "control.keyframe", "control.bitrate"],
  "capabilities": { "...": "optional, see Capabilities" }
}
```

Host:

```json
{
  "app": "oculibre-desktop",
  "app_version": "0.2.0",
  "id": "c1f0…",
  "name": "raspeur-ubuntu-studio",
  "device": { "os": "linux", "os_version": "6.8" },
  "features": ["video.h264", "clock", "orientation", "stats"]
}
```

- `id`: a random UUID created *once per installation* and stored locally. It identifies the peer across
  sessions (which virtual camera, which pairing). It MUST NOT be derived from hardware identifiers
  (IMEI, serial number, MAC address).
- `name`: human-readable, user-editable.
- `features`: what this side understands. The session uses the *intersection*. Feature names are
  lowercase, dot-separated, and listed in @versioning.

== `BYE` (0x02) <bye>

Sent before closing on purpose. The sender closes the connection right after.

```json
{ "reason": "user_stop", "message": "optional human-readable details" }
```

#table(
  columns: (auto, 1fr),
  table.header([`reason`], [Meaning]),
  [`user_stop`], [the user stopped streaming],
  [`busy`], [the camera already serves another host],
  [`camera_lost`], [the camera was taken by another app (e.g. a phone call) or disconnected],
  [`thermal`], [the phone stopped to protect itself (overheating)],
  [`battery`], [battery too low],
  [`version`], [incompatible versions (normally detected by the preamble)],
  [`unpaired`], [the peer is not paired (@security)],
  [`protocol_error`], [the peer sent something invalid],
  [`shutdown`], [the app or the computer is closing],
  [`other`], [see `message`],
)

Unknown reasons are treated as `other`.

== `PING` (0x03) / `PONG` (0x04) <ping>

Keep-alive and clock synchronisation (@clock). Either side MAY send `PING`; the other MUST answer with
`PONG` as soon as possible. The header `timestamp` is 0; the times are in the payload.

#grid(
  columns: (1fr, 1fr),
  gutter: 12pt,
  [
    `PING`, 16 bytes:
    #table(
      columns: (auto, auto, 1fr),
      table.header([Offset], [Size], [Field]),
      [0], [4], [`seq` (chosen by the sender)],
      [4], [4], [reserved, 0],
      [8], [8], [`t1`: send time, sender clock, µs],
    )
  ],
  [
    `PONG`, 32 bytes:
    #table(
      columns: (auto, auto, 1fr),
      table.header([Offset], [Size], [Field]),
      [0], [4], [`seq` (echoed)],
      [4], [4], [reserved, 0],
      [8], [8], [`t1` (echoed)],
      [16], [8], [`t2`: `PING` received, responder clock, µs],
      [24], [8], [`t3`: `PONG` sent, responder clock, µs],
    )
  ],
)

*Keep-alive:* a side that has sent nothing for 1 s sends a `PING`. A side that has received nothing for
5 s considers the peer gone and closes.

== Capabilities <capabilities>

Optional `HELLO.capabilities`, also returned by the `get_capabilities` command.

```json
{
  "cameras": [
    {
      "id": "0",
      "facing": "back",
      "sensor_orientation": 90,
      "modes": [
        { "width": 1920, "height": 1080, "fps": 30, "path": "normal" },
        { "width": 1920, "height": 1080, "fps": 60, "path": "unlisted" },
        { "width": 1920, "height": 1080, "fps": 60, "path": "high_speed", "capture_fps": 120 }
      ],
      "controls": ["zoom", "focus", "exposure_compensation", "torch"]
    }
  ],
  "codecs": ["h264"]
}
```

`path`: `normal` (listed by the camera), `unlisted` (works but not listed, see
#link("research/60fps.typ")[`research/60fps.typ`]), `high_speed` (slow-motion session, darker).

= Media messages <media>

== `STREAM_CONFIG` (0x20) <stream-config>

Describes a media stream (`header.stream` = the stream it configures). Sent before the first frame of the
stream and *again every time any field changes* (resolution, frame rate, camera, codec data…).

```json
{
  "config_id": 1,
  "kind": "video",
  "codec": "h264",
  "codec_data": "<base64 of SPS + PPS, Annex B>",
  "width": 1920,
  "height": 1080,
  "fps": 60,
  "capture_fps": 60,
  "path": "unlisted",
  "bitrate": 12000000,
  "color": { "primaries": "bt709", "transfer": "bt709",
             "matrix": "bt709", "range": "limited" },
  "camera": { "id": "0", "facing": "back",
              "sensor_orientation": 90, "mirrored": false }
}
```

- `config_id`: increases by 1 at every new config of this stream (wraps after 65535).
- `codec_data`: base64 of the decoder initialisation data, Annex B with start codes (H.264: SPS + PPS;
  HEVC: VPS + SPS + PPS).
- `fps`: target rate of the stream; `capture_fps`: rate the camera captures at (≠ `fps` in high-speed
  mode).
- `color`: as reported by the encoder output format. Primaries / matrix: `bt601`, `bt709`, `bt2020`,
  `unknown`; transfer: `bt709`, `srgb`, `pq`, `hlg`, `unknown`; range: `limited`, `full`. Default if
  absent: `bt601`, `bt709`, `bt601`, `limited`.
- `camera.mirrored`: whether the phone already mirrored the image (normally `false`: the host mirrors).

== `VIDEO_FRAME` (0x21) <video-frame>

`header.timestamp` = capture time on the camera clock (µs). Payload:

#table(
  columns: (auto, auto, 1fr),
  table.header([Offset], [Size], [Field]),
  [0], [4], [`frame_number`: +1 per frame produced by the encoder, *including dropped ones*],
  [4], [2], [`config_id` this frame belongs to],
  [6], [2], [reserved, 0],
  [8], […], [one complete access unit, H.264/HEVC *Annex B* (start codes included)],
)

#table(
  columns: (auto, auto, 1fr),
  table.header([Flag], [Name], [Meaning]),
  [`0x01`], [`KEY`], [the frame is a keyframe (IDR)],
  [`0x02`], [`DISCONTINUITY`], [frames were dropped right before this one (gap in `frame_number`)],
)

Rules:
- The first `VIDEO_FRAME` after a `STREAM_CONFIG` MUST be a keyframe with the new `config_id`.
- A host receiving a frame whose `config_id` it has no config for drops it and sends `request_keyframe`.

== `STREAM_END` (0x23) <stream-end>

The stream stops while the session stays open (switching camera, pausing):
`{"reason": "camera_switch"}`. A new `STREAM_CONFIG` restarts it.

= Control messages <control>

== `REQUEST` (0x40) / `RESPONSE` (0x41) <request>

```json
{ "id": 7, "cmd": "set_bitrate", "args": { "bitrate": 8000000 } }

{ "id": 7, "ok": true, "result": {} }
{ "id": 7, "ok": false, "error": { "code": "unsupported", "message": "…" } }
```

- `id`: chosen by the sender, unique among its pending requests. Every request gets *exactly one*
  response. Default timeout: 5 s.
- A side receiving an unknown `cmd` answers `ok: false`, code `unknown_command`.

#table(
  columns: (auto, 1fr),
  table.header([Error `code`], [Meaning]),
  [`unknown_command`], [command not known by this version],
  [`unsupported`], [known, but not possible on this device (e.g. no torch)],
  [`invalid_args`], [arguments missing or out of range],
  [`busy`], [try again later (e.g. camera reconfiguring)],
  [`failed`], [attempted and failed; see `message`],
)

Commands handled by the *camera* (none is handled by the host in 0.1):

#table(
  columns: (auto, 1fr, auto),
  table.header([`cmd`], [`args`], [Since]),
  [`request_keyframe`], [—], [0.1],
  [`set_bitrate`], [`{"bitrate": int}`], [0.1],
  [`get_capabilities`], [— → `result` = capabilities (@capabilities)], [0.1],
  [`set_mode`], [`{"camera", "width", "height", "fps", "path"?}`], [reserved],
  [`select_camera`], [`{"camera": id}`], [reserved],
  [`set_zoom`], [`{"ratio": float}`], [reserved],
  [`focus`], [`{"mode", "x"?, "y"?, "distance"?}`; `mode` is `auto`, `continuous` or `manual`; x and y in 0..1 of the image], [reserved],
  [`set_exposure_compensation`], [`{"ev": float}`], [reserved],
  [`set_torch`], [`{"on": bool}`], [reserved],
  [`lock`], [`{"ae": bool?, "awb": bool?}`], [reserved],
  [`set_white_balance`], [`{"mode"}`: `auto`, `daylight`, `cloudy`, `incandescent`, `fluorescent`…], [reserved],
)

== `EVENT` (0x42) <event>

Unsolicited notifications, no response.

```json
{ "event": "orientation", "data": { "device_rotation": 90 } }
```

#table(
  columns: (auto, auto, 1fr, auto),
  table.header([`event`], [Direction], [`data`], [Since]),
  [`orientation`], [camera → host], [`{"device_rotation"}`: `0`, `90`, `180`, `270` or `null` (@orientation)], [0.1],
  [`stats`], [camera → host, every 1 s], [`{"camera_fps", "sent_fps", "bitrate", "dropped", "queue", "latency_ms"?}`; `latency_ms`: average delay from capture to handing the frame to the network (camera, encoder, queue)], [0.1],
  [`stats`], [host → camera, every 1 s], [`{"received_fps", "decoded_fps", "decode_errors", "rtt_ms"?, "latency_ms"?}`; `latency_ms`: average capture-to-output latency (§11)], [0.1],
  [`thermal`], [camera → host], [`{"status", "battery_temp_c"?, "battery_pct"?, "charging"?}`; `status` is one of `none`, `light`, `moderate`, `severe`, `critical`, `emergency`, `shutdown` (Android thermal status)], [reserved],
  [`mode_changed`], [camera → host], [`{"reason": "thermal"|"user"|"camera", …}`, followed by a new `STREAM_CONFIG`], [reserved],
)

= Orientation <orientation>

The camera *never rotates the image*: frames are sent as the sensor produces them. The host decides the
final rotation and mirroring (user setting `0`, `90`, `180`, `270` or `auto`, plus `mirror`).

The camera provides what `auto` needs:
- `camera.sensor_orientation` and `camera.facing` (in `STREAM_CONFIG`), from the camera characteristics;
- `device_rotation` (`orientation` event): physical rotation of the phone from its natural (portrait)
  position, *counter-clockwise*, from the accelerometer, quantised to 0/90/180/270: the same convention
  as Android's `Display.getRotation()` (`90` = turned to landscape with its top edge on the left).
  `null` when the phone lies flat or the value is unknown. Sent at stream start and on every change.
  Android's `OrientationEventListener` counts clockwise: the camera sends `(360 - angle) mod 360`.

`auto` rotation applied by the host (clockwise), as Android computes `JPEG_ORIENTATION`:

$ "back:" quad r = ("sensor_orientation" - "device_rotation" + 360) mod 360 $
$ "front:" quad r = ("sensor_orientation" + "device_rotation") mod 360 $

`auto` never mirrors. Camera frames are not mirrored (only Android's on-screen preview of a front
camera is), so the result is the true picture, as a real webcam sends it: video-call apps mirror the
self-view themselves, and text stays readable for the other participants. `mirror` is a separate user
setting, applied after the rotation.

When `device_rotation` is `null`, the host keeps the previous value (or 0). The host SHOULD let the user
fix the rotation (`0`, `90`, `180`, `270`). The rotation does not change the shape of the host's output
(apps cannot follow a size change of a webcam): a picture of the other shape is fitted with black bars.
The camera SHOULD send its first `orientation` event before its first frame, so that the first frames
are already upright.

= Clock and latency <clock>

- The camera uses *one monotonic clock* for all its timestamps (frames, audio, events): the clock of the
  camera timestamps (Android `SENSOR_TIMESTAMP`, converted to µs).

Each `PING`/`PONG` exchange gives, with $t_4$ the reception time of the `PONG`, the round trip and the
clock offset of the responder relative to the sender:

$ "rtt" = (t_4 - t_1) - (t_3 - t_2) quad quad "offset" = ((t_2 - t_1) + (t_3 - t_4)) / 2 $

- The host keeps the offset with the lowest `rtt` over the last few seconds and computes the
  *capture-to-display latency* of each frame as $"display_time" - ("frame.timestamp" - "offset")$, with
  `offset` measured by the host's own `PING`s (camera clock minus host clock).
- The camera MUST answer `PING`s with the clock of its frame timestamps. On Android this is either
  `CLOCK_MONOTONIC` or `CLOCK_BOOTTIME`, and `SENSOR_INFO_TIMESTAMP_SOURCE` is not always right: the app
  compares the first frame's `SENSOR_TIMESTAMP` with both clocks, keeps the closest, and does not answer
  `PING`s before that (the host must tolerate unanswered `PING`s).
- With the camera's `latency_ms` (capture → network) and its own processing time (received → output),
  the host splits the latency into *phone + network + host*; the network share is what remains.
  The time an application takes to display the virtual camera is not included (see the glass-to-glass
  measurement in the roadmap).

= Flow control <flow>

- *Camera:* never lets latency pile up. When its send queue is full, it drops frames until the next
  keyframe, asks its encoder for one, and sets `DISCONTINUITY` on the next frame sent.
- *Host:* keeps at most one decoded frame waiting; on a decode error or a gap it cannot recover from, it
  sends `request_keyframe`.
- *Adaptive bitrate (later):* the camera MAY adjust its bitrate from the host `stats` event and the
  `rtt`; changing the bitrate does not need a new `STREAM_CONFIG`.

= Security <security>

*Threat model:* an attacker on the same network (flat-share, school or office Wi-Fi) who tries to watch
the stream, *inject images into the user's virtual camera*, or crash the host.

Rules in 0.1:
- A host MUST accept connections *only from loopback* (`127.0.0.1`, `::1`: USB via ADB) unless the user
  explicitly allows unencrypted network connections (`--insecure-lan`), in which case it shows a warning
  for the whole session.
- Over USB tethering, the link is a real network: same rule as Wi-Fi.

From 0.2 (Wi-Fi), a *secure channel* is established below the preamble on every non-loopback connection:
- mutual authentication with long-term keys exchanged once at *pairing* (QR code shown by the host,
  scanned by the phone, containing the host key, its addresses and a one-time token);
- encryption with forward secrecy. The candidates are a Noise handshake
  (`Noise_IK/XX_25519_ChaChaPoly_BLAKE2s`) or TLS 1.3 with pinned keys; the choice is made at that step
  and does not change the other sections;
- a host MUST refuse any camera that is not paired (`BYE {"reason": "unpaired"}`), and vice versa.

The pairing URI format is reserved: `oculibre://pair?v=1&…`.

= Limits and robustness <limits>

#table(
  columns: (1fr, auto),
  table.header([Item], [Limit]),
  [`length` of any message], [16 MiB (`0x01000000`); more → `protocol_error`],
  [JSON payload], [64 KiB],
  [`HELLO` deadline], [5 s after the preamble],
  [Silence before giving up], [5 s],
  [Request timeout (default)], [5 s],
  [Pending requests per side], [32],
)

Implementations MUST validate every length and field before use, MUST NOT allocate more than the
announced and allowed `length`, and SHOULD close the connection on the first malformed message
(`BYE {"reason": "protocol_error"}`). The host parser is fuzzed (@tests).

= Versioning and compatibility <versioning>

- *Major* changes when an older peer cannot work at all with a newer one. Peers with different majors
  never talk beyond the preamble.
- *Minor* changes only add things: new message types, new JSON keys, new commands, events, error codes
  and feature names. Existing meanings never change.
- Optional behaviour is negotiated through *features* (`HELLO.features`), not through version numbers.
- While the major is *0* (pre-release), a minor MAY still break compatibility. From 1.0, the rules
  above are a promise.

#table(
  columns: (auto, 1fr),
  table.header([Feature], [Meaning]),
  [`video.h264`], [H.264 video (mandatory in 0.1)],
  [`video.hevc`], [HEVC video (reserved)],
  [`audio.opus`], [Opus audio (reserved)],
  [`clock`], [`PING`/`PONG` timestamps are meaningful (@clock)],
  [`orientation`], [`orientation` events and `camera` info in `STREAM_CONFIG`],
  [`stats`], [`stats` events],
  [`control.keyframe`], [`request_keyframe`],
  [`control.bitrate`], [`set_bitrate`],
  [`control.*`], [reserved for the other commands: `control.mode`, `control.camera`, `control.zoom`, `control.focus`, `control.exposure`, `control.torch`, `control.lock`, `control.white_balance`],
)

= Conformance tests <tests>

- `tests/protocol/vectors/`: one file per message (`.hex` with the exact bytes, `.json` with the decoded
  fields). *Both* the Kotlin and the Rust implementations parse and re-encode every vector in their unit
  tests, so they cannot drift apart.
- The Rust parser has a `cargo-fuzz` target fed with arbitrary bytes.
- A session can be *recorded* to a file (the raw message stream) and replayed into the host for
  debugging.

= Reserved for later <reserved>

#table(
  columns: (auto, 1fr),
  table.header([Topic], [What is already reserved]),
  [Audio], [`stream = 2`, `AUDIO_FRAME` (0x22), `STREAM_CONFIG.kind = "audio"`, feature `audio.opus`],
  [Several cameras from one phone], [`stream = 3–255`],
  [Several phones on one host], [`HELLO.id` → one virtual camera per camera `id`],
  [Camera controls from the host], [commands of @request, features `control.*`],
  [Station mode (host dials the phone)], [port 47101, `_oculibre-cam._tcp`],
  [Discovery], [mDNS service types (@roles)],
  [Pairing and encryption], [@security, `oculibre://pair` URIs, `BYE unpaired`],
  [Thermal management], [`thermal` and `mode_changed` events, `BYE thermal` / `battery`],
  [Other codecs], [`STREAM_CONFIG.codec`, `codec_data`, features `video.*`],
  [HDR], [`STREAM_CONFIG.color`],
  [Datagram transports (SRT, QUIC, WebRTC)], [self-contained messages; mapping to be specified],
)

= Example session <example>

USB, Redmi Note 9 Pro, 1080p60 (`→` camera to host, `←` host to camera):

```
   TCP connect 127.0.0.1:47100 (adb reverse)
→  OCUL 0.1 camera            ←  OCUL 0.1 host
→  HELLO {features: [video.h264, clock, orientation, stats, control.keyframe, control.bitrate]}
←  HELLO {features: [video.h264, clock, orientation, stats]}
→  STREAM_CONFIG stream=1 {config_id 1, 1920x1080, fps 60, path unlisted, codec_data, camera back/90}
→  EVENT orientation {device_rotation: 90}
→  VIDEO_FRAME stream=1 KEY  frame 0, config 1
→  VIDEO_FRAME stream=1      frame 1, config 1
   …
←  PING seq 1              →  PONG seq 1        (every second when idle: clock offset, rtt)
→  EVENT stats {camera_fps: 60, sent_fps: 60, …}   ←  EVENT stats {received_fps: 60, …}
←  REQUEST {id: 1, cmd: request_keyframe}        (after a decode error)
→  RESPONSE {id: 1, ok: true}
→  VIDEO_FRAME stream=1 KEY  frame 812, config 1
   …
→  BYE {reason: user_stop}
   TCP close
```

= Changelog <changelog>

#table(
  columns: (auto, 1fr),
  table.header([Version], [Changes]),
  [0.1], [First implemented version: preamble, header, `HELLO`, `BYE`, `PING`/`PONG`, `STREAM_CONFIG`, `VIDEO_FRAME`, `STREAM_END`, `REQUEST`/`RESPONSE`/`EVENT`, orientation hints, stats, loopback-only plaintext.],
  [0 (draft)], [Initial sketch (CBOR control, phone-only client, Noise IK). Superseded.],
)
