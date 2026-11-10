# Desktop (receiver)

Rust workspace.

| Crate | State |
|---|---|
| `cli` → binary `oculibre` | **Phase 1, works on Linux**: TCP → H.264 decode (OpenH264) → v4l2loopback |
| `protocol`, `core`, `app-tauri` | Planned |

No system library needed: OpenH264 (BSD-2-Clause) is compiled from source by Cargo
(needs a C/C++ compiler: `sudo apt install build-essential`).

## Use it

```sh
bash ../scripts/vcam-load.sh     # once per boot (or --persist)
cargo run --release              # in this folder
```

Then press **Start** in the phone app. The phone appears as a webcam named **Oculibre**
in Firefox, Chrome, Discord, Zoom, OBS… (it shows up once the first frame has arrived).

- USB: `adb reverse` is done automatically; use `127.0.0.1` in the app.
- Wi-Fi: `cargo run --release -- --no-adb`, and type the PC's IP in the app.
- The phone can Stop/Start: the program keeps waiting for it.
- `cargo run --release -- --help` for all options.

## Develop

```sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt
```

`cargo test` includes an encode → split → decode round-trip, no phone needed.
