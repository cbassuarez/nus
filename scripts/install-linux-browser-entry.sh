#!/bin/sh
# Install this extracted nus package for the current user (shipped as
# install-desktop.sh). It copies the package to a stable folder, links the
# `nus` command into ~/.local/bin, registers its desktop entry, and checks Chromium's sandbox.
# Defaults remain unchanged. The extracted folder can be deleted afterwards.
#
#   ./install-desktop.sh                 install or update this channel
#   ./install-desktop.sh --uninstall     remove it (settings are kept)
#   sudo sh <installed>/install-desktop.sh --allow-sandbox
#                                        add the AppArmor rule Chromium needs
set -eu
src=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
say() { printf '%s\n' "$*"; }
fail() { printf 'nus: %s\n' "$*" >&2; exit 1; }
if [ ! -x "$src/nus" ] || [ ! -f "$src/nus-package.json" ]; then
    fail 'Run install-desktop.sh from the complete extracted nus package.'
fi
case $(cat "$src/nus-package.json") in
    *-preview.*) channel=preview ;;
    *) channel=release ;;
esac

# Chromium needs user namespaces; Ubuntu 24.04+ allows them only to programs
# whose AppArmor profile says so. This grants that to one installed copy.
if [ "${1:-}" = --allow-sandbox ]; then
    [ "$(id -u)" = 0 ] || fail 'Run --allow-sandbox with sudo.'
    [ -f /etc/apparmor.d/abi/4.0 ] || { say 'This system does not restrict user namespaces with AppArmor; nothing to do.'; exit 0; }
    case $src in *[!A-Za-z0-9._/-]*) fail "The installation path must contain only letters, digits and . _ / -: $src" ;; esac
    owner=$(stat -c %U "$src")
    name=nus-$channel-$owner
    cat > "/etc/apparmor.d/$name" <<EOF
# Allows nus's Chromium sandbox to create user namespaces for $owner's copy.
abi <abi/4.0>,
include <tunables/global>

profile $name $src/nus-desktop flags=(unconfined) {
  userns,

  include if exists <local/$name>
}
EOF
    apparmor_parser -r -T -W "/etc/apparmor.d/$name"
    say "Allowed Chromium's sandbox for $src/nus-desktop (/etc/apparmor.d/$name)."
    exit 0
fi

data=${XDG_DATA_HOME:-$HOME/.local/share}
case $data in /*) ;; *) fail 'XDG_DATA_HOME must be an absolute path.' ;; esac
dest=$data/nus/app/$channel
bin=$HOME/.local/bin
entry=$data/applications/dev.nus.app$( [ $channel = preview ] && printf .preview ).desktop

if [ "${1:-}" = --uninstall ]; then
    [ -L "$bin/nus" ] && [ "$(readlink -- "$bin/nus")" = "$dest/bin/nus" ] && rm -f -- "$bin/nus"
    [ -f "$entry" ] && grep -q "^Exec=\"$dest/nus\"" "$entry" && rm -f -- "$entry"
    rm -rf -- "$dest"
    command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$data/applications" 2>/dev/null || true
    say "Removed nus ($channel). Settings remain in $data/nus/installs/$channel."
    exit 0
fi
[ $# -eq 0 ] || fail "Unknown option: $1"
[ "$(id -u)" != 0 ] || fail 'Install nus as yourself, not as root.'

# Copy beside the destination, then swap, so a running nus keeps its files.
if [ "$src" != "$dest" ]; then
    mkdir -p -- "$data/nus/app"
    staged=$(mktemp -d "$data/nus/app/.install-XXXXXX")
    trap 'rm -rf -- "$staged"' EXIT
    cp -a -- "$src/." "$staged/"
    if [ -e "$dest" ]; then
        old=$(mktemp -d "$data/nus/app/.previous-XXXXXX")
        mv -- "$dest" "$old/package"
        mv -- "$staged" "$dest"
        rm -rf -- "$old"
    else
        mv -- "$staged" "$dest"
    fi
    trap - EXIT
fi

mkdir -p -- "$bin"
# One command for every channel: the copy installed last answers to `nus`.
ln -sfn -- "$dest/bin/nus" "$bin/nus"

# An entry left by a copy that no longer exists (a deleted download folder)
# would block registration; one that still runs is someone else's to remove.
if [ -f "$entry" ]; then
    target=$(sed -n 's/^Exec="\([^"]*\)".*/\1/p' "$entry" | head -n 1)
    if [ -n "$target" ] && [ "$target" != "$dest/nus" ] && [ ! -e "$target" ]; then
        rm -f -- "$entry"
    fi
fi
"$dest/nus" --install-browser-entry >/dev/null || fail "Could not add nus to your applications (see above). nus is installed in $dest."

say "Installed nus ($channel) to $dest."
case ":$PATH:" in *":$bin:"*) say "Open it from your applications menu, or run 'nus'." ;;
    *) say "Open it from your applications menu, or add $bin to PATH to run 'nus'." ;; esac
if ! "$dest/nus" --nus-check-userns >/dev/null 2>&1; then
    say ''
    say "This system blocks the user namespaces Chromium's sandbox needs, so web pages will not open."
    say 'To allow them for this copy of nus only, run:'
    say "  sudo sh '$dest/install-desktop.sh' --allow-sandbox"
fi
