# Android app (camera)

Kotlin, **no dependencies** (platform APIs only), minSdk 21 (Android 5.0).

`Camera2 → MediaCodec (H.264, input Surface, zero-copy) → Oculibre protocol 0.2 over TCP`
([`docs/protocol.typ`](../docs/protocol.typ)).

- `MainActivity.kt`: computer address, quality picker (only what this phone can really do), Start/Stop,
  live stats, battery-saver shortcuts.
- `StreamService.kt`: foreground service, so streaming survives the screen turning off.
- `StreamSession.kt`: camera, encoder, drop policy (drop until the next keyframe when the network lags),
  protocol session (handshake, `STREAM_CONFIG`, `VIDEO_FRAME`, `PING`/`PONG`, commands, events).
- `Protocol.kt`, `Json.kt`: the protocol itself, pure Kotlin (no Android class), tested on the JVM
  against the shared vectors of [`tests/protocol/vectors`](../tests/protocol/vectors).
- `Capabilities.kt`: what the camera and the encoder can do (including hidden 60 fps).
- `LabActivity.kt`: 60 fps experiments ([`docs/research/60fps.typ`](../docs/research/60fps.typ)).

The phone never rotates the picture: it sends its sensor orientation and its rotation, the computer
decides ([#3](https://github.com/Raspeur/Oculibre/issues/3)).

## Build and run (no Android Studio needed)

```sh
bash ../scripts/setup-dev.sh --vcam   # once: checks SDK, JDK, Rust, v4l2loopback…
./gradlew installDebug                # builds and installs on the plugged-in phone
cd ../desktop && cargo run --release  # on the PC, then press Start in the app
```

## Tests

```sh
./gradlew testDebugUnitTest           # protocol, JSON, base64, shared vectors
```
