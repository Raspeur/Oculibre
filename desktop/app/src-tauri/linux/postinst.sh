#!/bin/sh
# After installing Oculibre: set up the virtual camera now, so that no reboot is needed. From now
# on it is loaded at every boot (/etc/modules-load.d/oculibre.conf) with the options of
# /etc/modprobe.d/oculibre.conf: /dev/video10, named "Oculibre". The work is done by
# /usr/lib/oculibre/vcam-setup, which the app also runs (through pkexec) to repair the camera.
# Never fails the installation: when the camera cannot be loaded now, it says what to do.

# USB without developer mode: the phone in accessory mode is the user's from its next plug
# (/usr/lib/udev/rules.d/70-oculibre.rules).
udevadm control --reload-rules 2>/dev/null || true

# Executable whatever the packaging kept (polkit runs it by this path).
chmod 755 /usr/lib/oculibre/vcam-setup
/usr/lib/oculibre/vcam-setup
case $? in
    0) ;;
    2)
        if [ -d /var/lib/dkms/v4l2loopback ]; then
            echo "Oculibre: the virtual camera module could not be built for this kernel (DKMS)." >&2
            echo "Oculibre: install the kernel headers: sudo apt install linux-headers-$(uname -r), then open Oculibre." >&2
        else
            echo "Oculibre: no v4l2loopback module for this kernel yet: install linux-modules-v4l2loopback-generic" >&2
            echo "Oculibre: (or v4l2loopback-dkms), then open Oculibre." >&2
        fi
        ;;
    3)
        echo "Oculibre: Secure Boot refused the virtual camera module built on this computer (DKMS)." >&2
        echo "Oculibre: enrol its key (Ubuntu: sudo update-secureboot-policy --enroll-key; Debian:" >&2
        echo "Oculibre: sudo mokutil --import /var/lib/dkms/mok.pub), then reboot and choose Enroll MOK." >&2
        ;;
    4)
        echo "Oculibre: v4l2loopback is already loaded with other settings (another app?)." >&2
        echo "Oculibre: the \"Oculibre\" virtual camera will be ready after the next reboot." >&2
        ;;
    *)
        echo "Oculibre: the virtual camera could not be set up now; open Oculibre to try again." >&2
        ;;
esac
exit 0
