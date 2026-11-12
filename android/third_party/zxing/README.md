# ZXing core — QR decoding subset (vendored)

The in-app scanner reads the computer's pairing QR code with ZXing's decoder. The Android app has
no runtime dependencies (#16), so the needed source files are copied here instead of being pulled
by Gradle. They are compiled with the app (`sourceSets["main"].java.srcDir` in
`android/app/build.gradle.kts`); Oculibre's wrapper is `app/src/main/java/org/oculibre/QrDecoder.kt`.

| | |
|---|---|
| Project | [ZXing](https://github.com/zxing/zxing) ("Zebra Crossing") |
| Version | **3.5.3** |
| Origin | Maven Central, `com.google.zxing:core:3.5.3`, `core-3.5.3-sources.jar` |
| SHA-1 of the jar | `e842d62431675510e2b2116bbc8f75a8b1f6d1ca` |
| Licence | Apache License 2.0 (`LICENSE`), compatible with Oculibre's MIT licence |
| Changes | **None**: the files are byte-for-byte those of the jar, licence headers included |

## What is kept

Only what `com.google.zxing.qrcode.QRCodeReader` needs, found by following the imports and the
same-package references from it and from `PlanarYUVLuminanceSource` / `HybridBinarizer`
(50 files, about 8 600 lines with comments):

- `com/google/zxing/`: the reader API (`Reader`, `Result`, `BinaryBitmap`, `LuminanceSource`,
  `PlanarYUVLuminanceSource`, exceptions, hints);
- `common/`: bit matrices, binarizers, grid sampling, perspective transform;
- `common/reedsolomon/`: error correction;
- `qrcode/`, `qrcode/decoder/`, `qrcode/detector/`: QR detection and decoding.

No encoder, no other barcode format, no `android` or `javase` module.

## Updating

1. Download the new `core-<version>-sources.jar` from Maven Central and check its SHA-1 against
   the `.sha1` file next to it.
2. Replace the files under `src/` with the same paths from the jar; add any new file that the kept
   ones now reference (the build fails on a missing class).
3. Update the version and SHA-1 above, run `./gradlew testDebugUnitTest` (`QrDecoderTest` reads
   test pictures from `app/src/test/resources/qr/`).
