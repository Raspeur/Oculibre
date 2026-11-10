#!/usr/bin/env bash
# Oculibre — check and install the development environment (Ubuntu / Debian).
#
#   bash scripts/setup-dev.sh          # Android build + phase-1 viewer
#   bash scripts/setup-dev.sh --vcam   # also v4l2loopback (Linux virtual webcam, needed soon)
#
# Safe to run several times. Uses sudo only for apt packages.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SDK="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-$HOME/Android/Sdk}}"
WITH_VCAM=0
[ "${1:-}" = "--vcam" ] && WITH_VCAM=1

ok()   { printf '  \033[32m✔\033[0m %s\n' "$*"; }
warn() { printf '  \033[33m!\033[0m %s\n' "$*"; }
step() { printf '\n\033[1m%s\033[0m\n' "$*"; }
APT=()

# ---------------------------------------------------------------- Android SDK
step "1. Android SDK"
if [ ! -d "$SDK" ]; then
    echo "  Android SDK not found at $SDK."
    echo "  If it lives elsewhere: ANDROID_HOME=/path/to/sdk bash scripts/setup-dev.sh"
    exit 1
fi
ok "SDK: $SDK"
printf 'sdk.dir=%s\n' "$SDK" > "$ROOT/android/local.properties"
ok "android/local.properties written"

SDKMANAGER=""
for c in "$SDK"/cmdline-tools/latest/bin/sdkmanager "$SDK"/cmdline-tools/*/bin/sdkmanager; do
    if [ -x "$c" ]; then SDKMANAGER="$c"; break; fi
done
if [ -n "$SDKMANAGER" ]; then
    yes | "$SDKMANAGER" --licenses > /dev/null 2>&1 || true
    "$SDKMANAGER" "platform-tools" "platforms;android-35" "build-tools;35.0.0" > /dev/null
    ok "SDK packages: platform-tools, android-35, build-tools 35.0.0"
else
    warn "sdkmanager not found — Gradle will download the missing SDK packages on the first build"
fi

ADB="$SDK/platform-tools/adb"
if [ -x "$ADB" ]; then ok "adb: $("$ADB" version | head -1)"; else warn "adb not found in $SDK/platform-tools"; fi
if ! command -v adb > /dev/null; then
    warn "adb is not in your PATH. Add this line to ~/.bashrc:"
    echo "      export PATH=\"\$PATH:$SDK/platform-tools\""
fi
# udev rules so Xiaomi / Samsung phones are detected without root
if ! ls /etc/udev/rules.d/51-android.rules /lib/udev/rules.d/51-android.rules > /dev/null 2>&1; then
    APT+=(android-sdk-platform-tools-common)
fi

# ---------------------------------------------------------------- Java
step "2. Java (JDK 17 or newer)"
JV=0
if command -v java > /dev/null; then
    JV="$(java -version 2>&1 | awk -F'"' '/version/ { split($2, v, "."); print (v[1] == "1") ? v[2] : v[1]; exit }')"
fi
if [ "${JV:-0}" -ge 17 ] 2> /dev/null; then
    ok "java $JV"
else
    APT+=(openjdk-17-jdk-headless)
    warn "JDK 17 will be installed"
fi

# ---------------------------------------------------------------- ffmpeg
step "3. ffmpeg / ffplay (phase-1 viewer)"
if command -v ffplay > /dev/null; then ok "$(ffplay -version | head -1)"; else APT+=(ffmpeg); warn "ffmpeg will be installed"; fi

# ---------------------------------------------------------------- Rust
step "4. Rust (desktop client)"
if command -v cargo > /dev/null || [ -x "$HOME/.cargo/bin/cargo" ]; then
    CARGO="$(command -v cargo || echo "$HOME/.cargo/bin/cargo")"
    ok "$("$CARGO" --version)"
else
    warn "Rust not found — install it with: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
fi

# ---------------------------------------------------------------- v4l2loopback
step "5. v4l2loopback (Linux virtual webcam)"
if [ "$WITH_VCAM" = 1 ]; then
    APT+=(v4l2loopback-dkms v4l-utils "linux-headers-$(uname -r)")
    warn "v4l2loopback will be installed (if Secure Boot is on, you may be asked to enroll a MOK key)"
else
    ok "skipped (run with --vcam when we get to the virtual webcam)"
fi

# ---------------------------------------------------------------- apt
if [ "${#APT[@]}" -gt 0 ]; then
    step "Installing: ${APT[*]}"
    sudo apt-get update
    sudo apt-get install -y "${APT[@]}"
    if [[ " ${APT[*]} " == *" android-sdk-platform-tools-common "* ]]; then
        sudo usermod -aG plugdev "$USER" || true
        sudo udevadm control --reload-rules || true
        warn "Unplug / replug the phone (and log out/in once) for the USB rules to apply"
    fi
fi

step "Done. Next:"
echo "  1. On the phone: enable Developer options + USB debugging (Xiaomi: also 'Install via USB')"
echo "  2. Plug it in, accept the RSA prompt, check:  adb devices"
echo "  3. Build and install:  cd android && ./gradlew installDebug"
echo "  4. On the PC:          bash scripts/view.sh"
echo "  5. In the app:         press Start"
