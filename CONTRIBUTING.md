# Contributing to Oculibre

Thanks for your interest! English and French are both welcome in issues and pull requests.

## Ways to help

- **Test on old phones.** Open a [device report](.github/ISSUE_TEMPLATE/device_report.md) with your model, Android version and results. This is the most valuable contribution right now.
- **Review the protocol draft** ([`docs/protocol.md`](docs/protocol.md)), especially the security part.
- **Code**: Android (Kotlin), desktop (Rust), virtual cameras (C++ / Swift).

## Workflow

1. Open or comment on an issue before a large change.
2. Fork, create a branch (`feat/…`, `fix/…`, `docs/…`).
3. Keep pull requests focused; describe how you tested.
4. Use [Conventional Commits](https://www.conventionalcommits.org/) (`feat:`, `fix:`, `docs:`, `refactor:`…).

## Code style

- Kotlin: `ktlint`
- Rust: `cargo fmt` + `cargo clippy -- -D warnings`
- C++: `clang-format`
- Swift: `swift-format`

## Rules for any change touching the network

- Never trust a length or a field coming from the network.
- Any change to the wire format updates `docs/protocol.md` in the same pull request.
- Parser changes must keep the fuzz target passing.

By contributing, you agree that your contributions are licensed under the [MIT License](LICENSE).
