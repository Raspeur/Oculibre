#!/usr/bin/env bash
# Phase-1 viewer: receives the phone's raw H.264 stream and displays it with ffplay.
#
#   bash scripts/view.sh          # USB (adb reverse) — type 127.0.0.1 in the app
#   bash scripts/view.sh wifi     # Wi-Fi — type the PC's IP shown below in the app
#
# Several phones plugged in? Pick one: ANDROID_SERIAL=<serial from 'adb devices'> bash scripts/view.sh
set -euo pipefail

PORT="${PORT:-47100}"
SDK="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-$HOME/Android/Sdk}}"
ADB="$(command -v adb || echo "$SDK/platform-tools/adb")"

if [ "${1:-usb}" = "wifi" ]; then
    echo "Wi-Fi mode — type one of these addresses in the app:"
    hostname -I | tr ' ' '\n' | grep -E '^[0-9]+\.' | sed 's/^/   /'
    echo "(if nothing arrives, check your firewall: sudo ufw allow $PORT/tcp)"
else
    "$ADB" reverse "tcp:$PORT" "tcp:$PORT"
    echo "USB mode — the phone's 127.0.0.1:$PORT now reaches this PC."
fi

echo "Waiting for the phone… press Start in the app. Close the window or Ctrl+C to quit."
exec ffplay -hide_banner -loglevel warning \
    -fflags nobuffer -flags low_delay -framedrop \
    -window_title "Oculibre" \
    -f h264 "tcp://0.0.0.0:$PORT?listen=1"
