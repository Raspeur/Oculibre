# Contributing to Oculibre

Thanks for your interest! English and French are both welcome in issues and pull requests.

## Ways to help

- **Test on old phones.** Open a *Device report* issue with your model, Android version and results.
  This is the most valuable contribution right now.
- **Report bugs** with the *Bug report* form: it asks for what we need (phone, computer, versions,
  problem code). Never paste a pairing QR code or `oculibre://pair` link: it lets someone pair.
- **Ask or share in [Discussions](https://github.com/Raspeur/Oculibre/discussions)**: questions, setups,
  ideas not ready for an issue. Issues are for bugs and planned work.
- **Review the protocol** ([`docs/protocol.typ`](docs/protocol.typ)), especially the security part.
- **Code**: Android (Kotlin), desktop and virtual cameras (Rust), desktop window (TypeScript + React).

## Workflow

1. **An issue first.** Open one (or comment on an existing one) before writing code: we agree on
   the what and the how before you spend time. Small fixes (typos, an obvious bug) may skip this.
2. Fork, create a branch from `main` (`feat/…`, `fix/…`, `docs/…`).
3. Keep pull requests focused (one subject), fill the template, say how you tested (systems,
   phones, USB or Wi-Fi, apps).
4. `main` is protected: changes land through pull requests only, after review and green checks.

### Commit messages

The issue number first, then a [Conventional Commits](https://www.conventionalcommits.org/) type
and scope, in English:

```
#17 feat(desktop): Media Foundation virtual camera for Windows 11

What changes for users, in a few sentences.

- technical details, as a short list

Refs #17
```

Types: `feat`, `fix`, `docs`, `refactor`, `test`, `build`, `ci`, `chore`. Scopes: `desktop`,
`android`, `protocol`, `docs`… `Closes #N` in the last commit when the issue is done.

### Before opening the pull request

- Desktop (`desktop/`): `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings`;
  the window: `cd app && npm run build`
- Android: `./gradlew testDebugUnitTest`
- Texts shown to users exist in English **and** French
- No camera dump (`scripts/device-report.py` makes a shareable report), no key, no personal data

## Using AI tools

AI assistants are welcome here: the maintainer uses them too, on this very project. What matters is
the result, and that you stand behind it.

- **You understand every line you submit** and can explain why it is there. "The AI wrote it" is
  not an answer in a review.
- **Be critical of what it produces.** Build it, run the tests, try it on a real phone or computer
  when it touches the cameras, the network or the installers. These tools invent APIs, miss edge
  cases and sound sure of themselves when they are wrong.
- **The project's conventions win.** Commit format, code style, comments that explain why, texts in
  English and French, no new dependency without a reason: adapt the output to the project, not the
  other way round.
- **Keep it focused.** No large generated rewrite that nobody asked for; one subject per pull
  request, as for any change.
- **Say it.** Tell in the pull request what an AI tool helped with (one line is enough). Commits may
  carry a `Co-Authored-By:` trailer naming the tool, as the maintainer's do.
- **Issues and reviews are yours.** Do not paste an AI-written bug report or review you have not
  checked yourself.

## Documentation

Docs in `docs/` are written in [Typst](https://typst.app) with the shared layout `docs/oculibre.typ`
(template `isc-hei-document`, needs its fonts: Source Sans 3, Fira Code, Inria Sans). Build them with
`bash scripts/build-docs.sh`. Files that GitHub renders (`README.md`, `CONTRIBUTING.md`, `SECURITY.md`,
`devices/**`) stay in Markdown.

## Code style

- Rust: `cargo fmt` (on the files you change) + `cargo clippy -- -D warnings`
- Kotlin: `ktlint`
- TypeScript: `tsc` with the project's settings
- Comments explain *why*; user-facing words avoid jargon (no "v4l2", "DirectShow" or "TLS" in the
  apps' texts)

## Dependencies of the Android app

The Android app uses platform APIs only, so it builds for F-Droid and runs on Android 5: no library
fetched by Gradle (no AndroidX, no Compose, no Google Play Services; JUnit for tests only).
When one hard part really needs outside code, a small piece of source code may be **vendored**:

- under a licence compatible with MIT (MIT, BSD, ISC, Apache-2.0);
- pinned to a known version, in its own folder under `android/third_party/`, with its `LICENSE` and a
  `README.md` giving its origin, version and checksum, the files kept and what was changed.

First case: ZXing's QR decoder, for the in-app pairing scan ([`android/third_party/zxing`](android/third_party/zxing)).
Its licence is shown in the app (About › Open source licences): a new vendored piece adds its own.

## Dependencies of the desktop programs

Crates and npm packages are welcome when they earn their place. Their licence must be one of
[`desktop/deny.toml`](desktop/deny.toml): the CI check *Licences* fails otherwise. The licences file
the apps ship is generated from the lock files at build time (see
[`desktop/README.md`](desktop/README.md#licences-of-the-components)); a new npm package bundled into the
page is added to the list at the top of `desktop/app/scripts/licences.mjs`.

## Dependency updates

Dependabot opens at most one pull request per ecosystem (Rust, npm, Gradle, Actions) on the first of
each month, plus one at any time for a security advisory. Its commit messages (`chore(deps): …`)
have no issue number: the one exception to the commit format.

- **Monthly updates** only move versions the manifests already allow (Cargo and npm lock files,
  minor and patch versions of the Gradle plugins and the actions). CI green: approve and merge.
  CI red: close it with a comment, then open an issue if the update matters.
- **Bigger updates** (a new major version, a 0.x crate jumping to its next minor) are done by hand,
  in their own branch and pull request, with the code changes they need. Some must move together:
  `windows` and `windows-core`; `ffmpeg-sys-next` follows the FFmpeg version of the distributions.
- **Security updates** may be bigger: read the advisory first. If the vulnerable code is not used by
  Oculibre and the fix cannot be taken yet, dismiss the alert with the reason.

## Rules for any change touching the network

- Never trust a length or a field coming from the network.
- Any change to the wire format updates `docs/protocol.typ` in the same pull request.
- Parser changes must keep the fuzz target passing.

By contributing, you agree that your contributions are licensed under the [MIT License](LICENSE).
