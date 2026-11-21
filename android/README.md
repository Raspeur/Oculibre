# Android app (camera)

Kotlin, **no runtime dependencies** (platform APIs only: no AndroidX, no Compose), minSdk 21
(Android 5.0). The one exception is vendored source: ZXing's QR decoder in
[`third_party/zxing`](third_party/zxing) (Apache-2.0, pinned, unchanged).

`Camera2 → MediaCodec (H.264, input Surface, zero-copy) → Oculibre protocol 0.3 over TCP`, in TLS
over Wi-Fi ([`docs/protocol.typ`](../docs/protocol.typ)).

The phone never rotates or mirrors the picture: it sends its sensor orientation and its rotation, the
computer decides ([#3](https://github.com/Raspeur/Oculibre/issues/3)). The camera and the quality
are chosen from the computer (`set_mode`); the phone only offers the modes it really delivers.

## Code (`app/src/main/java/org/oculibre/`)

**Screens (the look of [`design/mockups`](../design/mockups), `Ui.kt` and `res/values/colors.xml`):**

- `WelcomeActivity.kt`: first launch, one step per screen (camera, notifications, staying active,
  connecting to the computer).
- `MainActivity.kt`: the everyday screen: the computer, a big Start/Stop button, what happens in
  plain words, problems (`Problem.kt`, `ProblemBanner.kt`, codes shared with the computer).
- `SettingsActivity.kt`: what Oculibre needs with a "Fix" for each, the paired computer, language.
- `ScannerActivity.kt`, `QrDecoder.kt`, `PairingPrompt.kt`: the in-app QR scan and the one
  "Pair with this computer?" confirmation (also for `oculibre://pair` links from a camera app or USB).
- `AdvancedActivity.kt`, `LabActivity.kt`: for developers: address, start quality, 60 fps
  experiments ([`docs/research/60fps.typ`](../docs/research/60fps.typ)), latency switches (`Tuning.kt`).

**Streaming:**

- `StreamService.kt`: foreground service, so streaming survives the screen turning off.
- `StreamSession.kt`: camera + encoder (a pipeline replaced as a whole on `set_mode`), drop policy
  (drop until the next keyframe when the network lags), the protocol session.
- `Capabilities.kt`: what each camera and the encoder can really do, hidden 60 fps included.

**Connection and security:**

- `SecureChannel.kt`: Wi-Fi TLS (1.3, or 1.2 on Android 5 to 9) with a Keystore key and the
  computer's pinned certificate. USB (loopback through `adb reverse`) stays plaintext.
- `Pairing.kt`: reads `oculibre://pair` offers, remembers the paired computer.
- `Discovery.kt`, `Networks.kt`: finds the computer (USB first, then the same subnet, mDNS, VPN) and
  remembers what worked.

Pure Kotlin (no Android class, tested on the JVM): `Protocol.kt`, `Json.kt` (byte-identical to the
desktop's serde_json), `PairingOffer`, `Problem`.

Texts: `res/values/strings.xml` (English) and `res/values-fr/strings.xml` (French).

## Build and run (no Android Studio needed)

```sh
bash ../scripts/setup-dev.sh          # once: checks SDK, JDK, Rust, v4l2loopback…
./gradlew installDebug                # builds and installs on the plugged-in phone
```

Then start Oculibre on the computer (the desktop app, or `cargo run --release` in `desktop/`) and
press **Start** in the app. Several phones plugged in: `ANDROID_SERIAL=<serial> ./gradlew installDebug`.

## Tests

```sh
./gradlew testDebugUnitTest           # protocol and shared vectors, JSON, pairing, problems, QR
```
