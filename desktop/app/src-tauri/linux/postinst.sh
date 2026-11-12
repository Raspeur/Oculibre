#!/bin/sh
# After installing Oculibre: load the virtual camera now, so that no reboot is needed. From now
# on it is loaded at every boot (/etc/modules-load.d/oculibre.conf) with the options of
# /etc/modprobe.d/oculibre.conf: /dev/video10, named "Oculibre".
# Never fails the installation: when the camera cannot be loaded now, it says what to do.
set -e

if grep -qs Oculibre /sys/class/video4linux/*/name; then
    exit 0 # already there
fi
if lsmod | grep -q '^v4l2loopback'; then
    echo "Oculibre: v4l2loopback is already loaded with other settings (another app?)." >&2
    echo "Oculibre: the \"Oculibre\" virtual camera will be ready after the next reboot." >&2
    exit 0
fi
if ! modprobe v4l2loopback 2>/dev/null; then
    echo "Oculibre: the virtual camera could not be loaded now." >&2
    echo "Oculibre: with Secure Boot and a module built on this computer (DKMS), enrol its key:" >&2
    echo "Oculibre:   sudo update-secureboot-policy --enroll-key, then reboot and choose Enroll MOK." >&2
fi
exit 0
