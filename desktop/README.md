# Desktop (receiver)

Rust workspace.

| Crate | State |
|---|---|
| `cli` → binary `oculibre` | **Phase 1, works on Linux**: TCP → H.264 decode (OpenH264) → v4l2loopback |
| `protocol` | **Protocol v0.1** ([`docs/protocol.typ`](../docs/protocol.typ)): preamble, header, messages, reader with the §14 limits, clock sync, `auto` rotation. Shared test vectors in [`tests/protocol/vectors`](../tests/protocol/vectors), fuzz target in `protocol/fuzz` |
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
- Wi-Fi: `cargo run --release -- --no-adb`, and type the PC's IP in the app.
- The phone can Stop/Start: the program keeps waiting for it.
- `cargo run --release -- --help` for all options.

## Develop

```sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt
```

`cargo test` includes an encode → split → decode round-trip and the protocol test vectors, no phone needed.

After a deliberate wire-format change, regenerate the shared vectors:
`cargo run -p oculibre-protocol --example gen_vectors`.

Fuzzing (nightly toolchain): `cargo install cargo-fuzz`, then `cd protocol && cargo +nightly fuzz run decode`.
