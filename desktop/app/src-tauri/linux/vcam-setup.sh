#!/bin/sh
# Sets up the "Oculibre" virtual camera (v4l2loopback), as root. Installed as
# /usr/lib/oculibre/vcam-setup; run by the package after installation, and by the app through
# pkexec (polkit action org.oculibre.vcam) when the camera is missing.
#
# - writes the module's options and the load at boot, if they are not there yet;
# - loads the module now (no reboot needed);
# - never unloads a v4l2loopback another app set up (OBS, a script…).
#
# Exit codes, read by the app (desktop/core/src/vcam_setup.rs):
#   0 the camera is ready
#   2 no v4l2loopback module for the running kernel (package missing)
#   3 Secure Boot refused the module (built on this computer, key not enrolled)
#   4 v4l2loopback is already loaded with other settings: ready after the next reboot
#   1 anything else (the reason is on stderr)

PATH=/usr/sbin:/usr/bin:/sbin:/bin

OPTIONS='options v4l2loopback devices=1 video_nr=10 card_label=Oculibre exclusive_caps=1'

ready() {
    grep -qs Oculibre /sys/class/video4linux/*/name
}

# The settings used at every boot (the package ships the same files).
if [ ! -e /etc/modprobe.d/oculibre.conf ]; then
    echo "$OPTIONS" > /etc/modprobe.d/oculibre.conf || exit 1
fi
if [ ! -e /etc/modules-load.d/oculibre.conf ]; then
    echo v4l2loopback > /etc/modules-load.d/oculibre.conf || exit 1
fi

ready && exit 0

if lsmod | grep -q '^v4l2loopback '; then
    echo "v4l2loopback is already loaded with other settings; the Oculibre camera will be there after the next reboot" >&2
    exit 4
fi

if ! out=$(modprobe v4l2loopback 2>&1); then
    echo "$out" >&2
    case "$out" in
        *"Key was rejected"*|*"Required key not available"*) exit 3 ;;
        *"not found"*) exit 2 ;;
        *) exit 1 ;;
    esac
fi

# The device appears through udev: wait for it, a little.
udevadm settle --timeout=5 2>/dev/null
for _ in 1 2 3 4 5; do
    ready && exit 0
    sleep 0.2
done
echo "v4l2loopback is loaded but no \"Oculibre\" device appeared" >&2
exit 1
