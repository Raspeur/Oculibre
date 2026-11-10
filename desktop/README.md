# Desktop (receiver)

Rust workspace — **not started yet**. Target: Phase 1 (CLI + Linux v4l2loopback).

Planned crates: `protocol`, `core`, `cli`, `app-tauri`, `vcam-linux`.

## Linux prerequisite

```sh
sudo apt install v4l2loopback-dkms
sudo modprobe v4l2loopback devices=1 video_nr=10 card_label="Oculibre" exclusive_caps=1
```

See [`../docs/ARCHITECTURE.md`](../docs/ARCHITECTURE.md#desktop-receiver--desktop).
