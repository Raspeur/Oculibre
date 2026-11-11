# Desktop (receiver)

Rust workspace.

| Crate | State |
|---|---|
| `cli` → binary `oculibre` | **Works on Linux**: protocol 0.2 session → H.264 decode (OpenH264) → v4l2loopback; `oculibre bench` |
| `protocol` | **Protocol v0.2** ([`docs/protocol.typ`](../docs/protocol.typ)): preamble, header, messages, reader with the §14 limits, clock sync, `auto` rotation. Shared test vectors in [`tests/protocol/vectors`](../tests/protocol/vectors), fuzz target in `protocol/fuzz` |
| `core`, `app-tauri` | Planned |

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
- Wi-Fi, encrypted (TLS with pinned keys, see [`docs/research/secure-channel.typ`](../docs/research/secure-channel.typ)):
  - once per phone: `cargo run --release -- --pair` shows a QR code. Scan it with the phone's camera
    app (or, phone plugged in over USB, it is sent to the phone directly), check that the key shown on
    the phone is the one printed here, tap **Pair**, then **Start**;
  - then: `cargo run --release -- --wifi`, and **Start** in the app (the address is filled in). The
    computer announces itself on the network (mDNS): if its address changes, the phone finds it again.
  - `oculibre pairings` lists the paired phones, `oculibre unpair <name>` forgets one. Without `--wifi`
    only USB (loopback) connections are accepted; plaintext is never accepted from the network.
- The phone can Stop/Start: the program keeps waiting for it.
- Rotation: by default (`--rotate auto`) the picture is upright however you hold the phone.
  The virtual camera stays landscape, like every webcam: a portrait picture gets black bars on the
  sides. `--portrait` makes it portrait instead. `--rotate 0|90|180|270` fixes the rotation;
  `--mirror` flips left ↔ right.
- An app that has the virtual camera open (OBS, ffplay, a browser tab) locks its size: close it
  before changing the quality on the phone.
- `cargo run --release -- --help` for all options.

## Measure a phone (`oculibre bench`)

```sh
cargo run --release -- bench     # then Start in the app (any quality)
```

The PC switches the phone through every camera and quality it offers (about 15 s each), measures the
real frame rate, the latency (phone + network + PC), the bitrate and the dropped frames, prints a table
and writes it into `devices/<brand>/<model>/report.md` (found through its "Model" line; run
`python3 scripts/device-report.py` first for a new phone). USB and Wi-Fi (`--wifi`) results get
their own section, replaced at each run. `--seconds`, `--warmup`, `--report <path>`, `--no-report`.
Needs app 0.2 or newer.

## Develop

```sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt
```

`cargo test` runs a simulated phone against the real session code (handshake, frames, keyframe
requests, `BYE`) and the protocol test vectors: no phone needed.

After a deliberate wire-format change, regenerate the shared vectors:
`cargo run -p oculibre-protocol --example gen_vectors`.

Fuzzing (nightly toolchain): `cargo install cargo-fuzz`, then `cd protocol && cargo +nightly fuzz run decode`.
