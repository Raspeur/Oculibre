# Scripts

Run them from the repository root. Each one explains its options at the top of the file.

| Script | What it does |
|---|---|
| `setup-dev.sh` | Checks and installs the development environment on Ubuntu or Debian (Android SDK, JDK, Rust, FFmpeg headers); `--vcam` also sets up v4l2loopback |
| `vcam-load.sh` | Loads the Linux virtual camera (`/dev/video10`, named "Oculibre"); `--persist` loads it at every boot |
| `device-report.py` | Writes a phone's camera capabilities into `devices/`, without personal data (see [`devices/README.md`](../devices/README.md)) |
| `build-deb.sh` | Builds one `.deb` per Ubuntu/Debian release in Docker (`deb/Dockerfile`), then checks that it installs on a clean system |
| `build-docs.sh` | Builds the Typst documents of `docs/` into `docs/build/` |
| `installer-images.py` | Renders the Windows installer's pictures from the logos |
| `vcam-waiting-logo.py` | Renders the logo of the Windows virtual camera's waiting picture |
| `github-protections.sh` | Maintainers: turns on the repository's rulesets and security features (see [`.github/rulesets`](../.github/rulesets)) |
