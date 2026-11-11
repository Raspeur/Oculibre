# Device database

What each phone's cameras really expose to apps, and how Oculibre behaves on it.
Manufacturers often hide features from third-party apps (see [docs/research/60fps.typ](../docs/research/60fps.typ)),
so real data from real phones is the only way to know what Oculibre can do.

```
devices/<brand>/<model>/
    camera-android<version>-<build>.txt   sanitised camera dump (one per firmware)
    report.md                             summary table + test results
```

## Add your phone (5 minutes, no coding)

1. Enable **Developer options → USB debugging** on the phone, plug it in, accept the prompt.
2. From the repository root:
   ```sh
   python3 scripts/device-report.py
   # or, if the folder name is not the marketing name:
   python3 scripts/device-report.py --name "Galaxy S23 Ultra"
   ```
3. Measure Oculibre on it, automatically: load the virtual camera (`bash scripts/vcam-load.sh`), run
   `cargo run --release -- bench` in `desktop/`, then press **Start** in the app. The PC goes through
   every camera and quality (a few minutes; keep the phone still, facing something lit) and writes the
   results into `report.md`. Nothing to copy by hand.
4. Open a pull request (or attach both files to a
   [device report issue](../.github/ISSUE_TEMPLATE/device_report.md)).

Several phones plugged in? `ANDROID_SERIAL=<serial from 'adb devices'> python3 scripts/device-report.py`.

## Privacy

`adb shell dumpsys media.camera` contains more than camera specs. The script keeps **only the static
camera characteristics** and removes:

- the camera usage log (which app opened the camera, when, process IDs);
- dynamic / session info and HAL state dumps;
- per-unit factory calibration (colour matrices, lens geometry, dual-camera calibration blobs,
  EEPROM/OTP data), which could identify a specific phone.

The phone's serial number is never read. **Never commit a raw dump**: `camera-dump*.txt` is git-ignored.

## Summary

| Device | Android | Listed max fps | Real 60 fps (unlisted `[60,60]`) | Slow-motion mode | Latency 1080p60, USB / Wi-Fi (glass-to-glass) |
|---|---|---|---|---|---|
| [Xiaomi Redmi Note 9 Pro](redmi/redmi-note-9-pro/report.md) | 12 | 30 | ✅ 1080p 60.5 fps (exposure ≤ 8.3 ms) | 1080p@120: too dark indoors | not measured yet |
| [Samsung Galaxy S23 Ultra](samsung/galaxy-s23-ultra/report.md) | 17 | 30 | ✅ 1080p 59.8 fps (exposure 16.7 ms) | 1080p@120/240: good picture | 112 ms / 103 ms |
