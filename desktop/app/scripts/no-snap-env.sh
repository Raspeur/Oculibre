#!/usr/bin/env bash
# Runs a command without the variables a snap application (VS Code installed as a snap) gives to
# its terminal. They point into /snap (GTK modules, locales, GIO modules…): WebKitGTK then loads
# the snap's old libraries and crashes with
#   WebKitNetworkProcess: symbol lookup error: /snap/core20/…/libpthread.so.0: undefined symbol
# Outside a snap terminal this changes nothing.
set -euo pipefail
if [ -n "${SNAP:-}" ]; then
    unset GTK_PATH GTK_EXE_PREFIX GTK_IM_MODULE_FILE GIO_MODULE_DIR GDK_PIXBUF_MODULE_FILE \
          GDK_PIXBUF_MODULEDIR LOCPATH GSETTINGS_SCHEMA_DIR
    # VS Code keeps the values it replaced: put them back.
    if [ -n "${XDG_DATA_DIRS_VSCODE_SNAP_ORIG:-}" ]; then
        export XDG_DATA_DIRS="$XDG_DATA_DIRS_VSCODE_SNAP_ORIG"
    fi
    if [ -n "${XDG_CONFIG_DIRS_VSCODE_SNAP_ORIG:-}" ]; then
        export XDG_CONFIG_DIRS="$XDG_CONFIG_DIRS_VSCODE_SNAP_ORIG"
    fi
    for v in $(compgen -e | grep '^SNAP'); do unset "$v"; done
fi
exec "$@"
