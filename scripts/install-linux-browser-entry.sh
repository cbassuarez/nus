#!/bin/sh
# Run this copy from an extracted, stable nus package. Defaults remain unchanged.
set -eu
base=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
if [ ! -x "$base/nus" ] || [ ! -f "$base/nus-package.json" ]; then
    printf '%s\n' 'Run install-desktop.sh from the complete extracted nus package.' >&2
    exit 1
fi
exec "$base/nus" --install-browser-entry
