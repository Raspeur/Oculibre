# Contributing to Oculibre

Thanks for your interest! English and French are both welcome in issues and pull requests.

## Ways to help

- **Test on old phones.** Open a [device report](.github/ISSUE_TEMPLATE/device_report.md) with your model, Android version and results. This is the most valuable contribution right now.
- **Review the protocol draft** ([`docs/protocol.typ`](docs/protocol.typ)), especially the security part.
- **Code**: Android (Kotlin), desktop (Rust), virtual cameras (C++ / Swift).

## Workflow

1. Open or comment on an issue before a large change.
2. Fork, create a branch (`feat/…`, `fix/…`, `docs/…`).
3. Keep pull requests focused; describe how you tested.
4. Use [Conventional Commits](https://www.conventionalcommits.org/) (`feat:`, `fix:`, `docs:`, `refactor:`…).

## Documentation

Docs in `docs/` are written in [Typst](https://typst.app) with the shared layout `docs/oculibre.typ`
(template `isc-hei-document`, needs its fonts: Source Sans 3, Fira Code, Inria Sans). Build them with
`bash scripts/build-docs.sh`. Files that GitHub renders (`README.md`, `CONTRIBUTING.md`, `SECURITY.md`,
`devices/**`) stay in Markdown.

## Code style

- Kotlin: `ktlint`
- Rust: `cargo fmt` + `cargo clippy -- -D warnings`
- C++: `clang-format`
- Swift: `swift-format`

## Dependencies of the Android app

The Android app uses platform APIs only, so it builds for F-Droid and runs on Android 5: no library
fetched by Gradle (no AndroidX, no Compose, no Google Play Services; JUnit for tests only).
When one hard part really needs outside code, a small piece of source code may be **vendored**:

- under a licence compatible with MIT (MIT, BSD, ISC, Apache-2.0);
- pinned to a known version, in its own folder under `android/third_party/`, with its `LICENSE` and a
  `README.md` giving its origin, version and checksum, the files kept and what was changed.

First case: ZXing's QR decoder, for the in-app pairing scan ([`android/third_party/zxing`](android/third_party/zxing)).

## Rules for any change touching the network

- Never trust a length or a field coming from the network.
- Any change to the wire format updates `docs/protocol.typ` in the same pull request.
- Parser changes must keep the fuzz target passing.

By contributing, you agree that your contributions are licensed under the [MIT License](LICENSE).
