<!-- Thanks! English or French. Link the issue: a pull request without one may wait. -->

Closes #

## What changes for users

## How it was tested
<!-- Systems (Linux distribution, Windows 10/11), phones (model + Android), USB or Wi-Fi, apps (OBS…). -->

## Checklist
- [ ] Commits follow `#N type(scope): summary` (see CONTRIBUTING.md)
- [ ] `cargo test` and `cargo clippy --all-targets -- -D warnings` pass (desktop); `./gradlew testDebugUnitTest` (Android)
- [ ] `docs/protocol.typ`, the Rust and Kotlin code and the test vectors updated together if the wire format changed
- [ ] User-facing texts in English and French (desktop `src/i18n.ts`, Android `values` + `values-fr`)
- [ ] No new runtime dependency in the Android app; none added on the desktop without saying why
- [ ] No camera dump, key, pairing link or personal data in the diff
- [ ] If an AI tool helped: I reviewed and understand every change, and I say above what it helped with
