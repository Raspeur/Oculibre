#!/bin/sh
# After installing Oculibre: set up the virtual camera now, so that no reboot is needed. From now
# on it is loaded at every boot (/etc/modules-load.d/oculibre.conf) with the options of
# /etc/modprobe.d/oculibre.conf: /dev/video10, named "Oculibre". The work is done by
# /usr/lib/oculibre/vcam-setup, which the app also runs (through pkexec) to repair the camera.
# Never fails the installation: when the camera cannot be loaded now, it says what to do.

# Executable whatever the packaging kept (polkit runs it by this path).
chmod 755 /usr/lib/oculibre/vcam-setup
/usr/lib/oculibre/vcam-setup
case $? in
    0) ;;
    2)
        echo "Oculibre: no v4l2loopback module for this kernel yet: install linux-modules-v4l2loopback-generic" >&2
        echo "Oculibre: (or v4l2loopback-dkms), then open Oculibre." >&2
        ;;
    3)
        echo "Oculibre: Secure Boot refused the virtual camera module built on this computer (DKMS)." >&2
        echo "Oculibre: enrol its key: sudo update-secureboot-policy --enroll-key, then reboot and choose Enroll MOK." >&2
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
