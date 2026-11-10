#!/usr/bin/env bash
# Loads v4l2loopback with the settings Oculibre expects: /dev/video10, named "Oculibre".
#
#   bash scripts/vcam-load.sh            # now (until next reboot)
#   bash scripts/vcam-load.sh --persist  # now and at every boot
set -euo pipefail

OPTS="devices=1 video_nr=10 card_label=Oculibre exclusive_caps=1"

is_ours() { grep -qs Oculibre /sys/class/video4linux/*/name; }

if is_ours; then
    echo "Virtual camera already loaded."
else
    if lsmod | grep -q '^v4l2loopback'; then
        echo "v4l2loopback is loaded with other settings — reloading it (close apps using a webcam)…"
        sudo modprobe -r v4l2loopback
    fi
    if ! sudo modprobe v4l2loopback $OPTS; then
        echo
        echo "modprobe failed. If the message says 'Key was rejected by service', Secure Boot"
        echo "blocks the module: run 'sudo update-secureboot-policy --enroll-key', reboot,"
        echo "and choose 'Enroll MOK' on the blue screen."
        exit 1
    fi
fi

if [ "${1:-}" = "--persist" ]; then
    echo v4l2loopback | sudo tee /etc/modules-load.d/oculibre.conf > /dev/null
    echo "options v4l2loopback $OPTS" | sudo tee /etc/modprobe.d/oculibre.conf > /dev/null
    echo "Will also be loaded at every boot."
fi

for d in /sys/class/video4linux/*; do
    if grep -qs Oculibre "$d/name"; then echo "Ready: /dev/$(basename "$d")"; fi
done
