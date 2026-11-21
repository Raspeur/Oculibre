# Releasing Oculibre

For the maintainer. A release is made by pushing a version tag: the workflow
[`release.yml`](workflows/release.yml) builds every package and makes a **draft** release, which is
published by hand once checked.

| Package | Built by | Signed |
|---|---|---|
| `Oculibre_<version>_<release>_amd64.deb` (Ubuntu 26.04, 24.04, Debian 13) | `scripts/build-deb.sh`, in Docker | no (apt checks nothing for a downloaded `.deb`) |
| `Oculibre_<version>_x64-setup.exe` | `tauri build` on Windows | not yet: SmartScreen warns about an unknown publisher (SignPath Foundation once public) |
| `Oculibre_<version>_android.apk` | `assembleRelease` | yes, with the release key (below) |
| `SHA256SUMS.txt` | the workflow | |

## Once: the Android signing key

Every version of the app must be signed with the **same key, forever**: Android refuses to update an
app signed with another one, so losing the key means every user uninstalls and reinstalls. Keep two
copies: one in the password manager, one offline.

1. Create it on your own computer (the JDK's `keytool`; it asks for a password, use a long random
   one from the password manager):

   ```sh
   mkdir -p ~/oculibre-release && cd ~/oculibre-release
   keytool -genkeypair -keystore oculibre-release.jks -storetype PKCS12 \
     -alias oculibre -keyalg RSA -keysize 4096 -validity 36500 -dname "CN=Oculibre"
   keytool -list -v -keystore oculibre-release.jks | grep -E "Alias|SHA256"
   base64 -w0 oculibre-release.jks > oculibre-release.jks.base64
   ```

2. Save in the password manager (one entry, "Oculibre Android release key"): the password, the
   alias `oculibre`, the SHA-256 fingerprint printed above, and the content of
   `oculibre-release.jks.base64` (a secure note field holds it; or the `.jks` as an attachment).

3. Give it to the release workflow, as secrets of the `release` environment (only `v*` tags can use
   it; `scripts/github-protections.sh` creates it). `gh` reads each value from the keyboard, so it
   lands neither in the shell history nor in a file:

   ```sh
   gh secret set OCULIBRE_KEYSTORE_BASE64 --env release < oculibre-release.jks.base64
   gh secret set OCULIBRE_KEYSTORE_PASSWORD --env release
   gh secret set OCULIBRE_KEY_PASSWORD --env release     # the same password (PKCS12)
   gh secret set OCULIBRE_KEY_ALIAS --env release --body oculibre
   ```

4. Then delete `~/oculibre-release` from the computer, or keep it on an encrypted disk only.

A local signed build, if ever needed: `OCULIBRE_KEYSTORE_FILE=… OCULIBRE_KEYSTORE_PASSWORD=…
OCULIBRE_KEY_ALIAS=oculibre OCULIBRE_KEY_PASSWORD=… ./gradlew assembleRelease` (in `android/`).

## Each release

1. On a branch, in a pull request:
   - the version everywhere: `desktop/Cargo.toml` (`[workspace.package]`), `desktop/app/package.json`
     (and `npm install` for the lock file), `desktop/app/src-tauri/tauri.conf.json`,
     `android/app/build.gradle.kts` (`versionName`, and `versionCode` + 1);
   - the release notes: `.github/release-notes/v<version>.md` (what is new for users, what is
     known not to work).
2. Once merged, a rehearsal (optional, recommended after a change of the build): *Actions ›
   Release › Run workflow* on `main` builds every package (APK unsigned) without publishing;
   they are in the run's artifacts.
3. Then tag `main`:

   ```sh
   git switch main && git pull
   git tag -a v0.4.0 -m "Oculibre 0.4.0"
   git push origin v0.4.0
   ```

4. Watch *Actions › Release*. The first job checks that the tag and the four version numbers agree.
   About 30 minutes (no cache, on purpose).
5. Open the draft in *Releases*: install the packages on test machines or VMs, check
   the notes, then **Publish release**. Not as a pre-release: the README links to
   `releases/latest`, which skips pre-releases (the notes say "beta").

A tag is never moved: a broken release is fixed by the next version.
