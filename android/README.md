# Android app (sender)

Kotlin, **no dependencies** (platform APIs only), minSdk 21 (Android 5.0).

## Current state: phase-1 spike

`Camera2 → MediaCodec (H.264, input Surface, zero-copy) → TCP`, raw Annex B stream (no Oculibre framing yet).

- `MainActivity.kt` — one address field, Start/Stop, live stats (fps, Mbit/s, dropped frames)
- `StreamSession.kt` — camera, encoder, drop policy (drop until next keyframe when the network lags), sender

Defaults: back camera, 1280×720 (or the closest size ≤ 720p), 30 fps, 4 Mbit/s, keyframe every second.
The image is in the sensor's orientation: **hold the phone in landscape**.

## Build and run (no Android Studio needed)

```sh
bash ../scripts/setup-dev.sh        # once: checks SDK, JDK, ffmpeg…
./gradlew installDebug              # builds and installs on the plugged-in phone
bash ../scripts/view.sh             # on the PC, then press Start in the app
```

`ffplay` accepts one connection: relaunch `view.sh` after each Stop.
