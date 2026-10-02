#!/bin/sh
# Install nus on Linux:
#
#   curl -fsSL https://raw.githubusercontent.com/cbassuarez/nus/main/scripts/install.sh | sh
#
# Options (after `sh -s --`):
#   --preview   the preview channel (also chosen while there is no stable release)
#   --user      install for this account only, without sudo
#
# On Debian and Ubuntu it installs the release's .deb with apt, which sets up
# Chromium's sandbox and keeps nus updated with your system. Elsewhere, or with
# --user, it installs the release archive to ~/.local/share/nus, which nus
# updates itself. Every download is checked against the release's SHA256SUMS.
set -eu

say() { printf '%s\n' "$*"; }
fail() { printf 'nus installer: %s\n' "$*" >&2; exit 1; }
need() { command -v "$1" >/dev/null 2>&1 || fail "$1 is required."; }

usage() {
    say 'Install nus on Linux.  Options: --preview (the preview channel), --user (this account only, no sudo).'
}

# Every release asset URL, newest release first. NUS_RELEASES_API names a mirror.
assets() {
    curl -fsSL -H 'Accept: application/vnd.github+json' \
        "${NUS_RELEASES_API:-https://api.github.com/repos/cbassuarez/nus/releases?per_page=30}" |
        sed -n 's/.*"browser_download_url": *"\([a-z]*:[^"]*\)".*/\1/p'
}

# The newest Linux archive of a channel.
archive_for() {
    for url in $urls; do
        name=${url##*/}
        case $1:$name in
            preview:nus-*-preview.*-linux-x86_64.tar.gz) printf '%s\n' "$url"; return ;;
            release:nus-*-preview.*) ;;
            release:nus-*-linux-x86_64.tar.gz) printf '%s\n' "$url"; return ;;
        esac
    done
}

# Download one asset of the chosen release into $tmp and check its hash.
fetch() {
    say "Downloading $1…"
    curl -fL --progress-bar "$base/$1" -o "$tmp/$1"
    (cd "$tmp" && grep "  $1\$" SHA256SUMS.txt | sha256sum -c --status) || fail "$1 does not match the release's SHA256SUMS."
}

main() {
    channel=release mode=auto
    for arg; do
        case $arg in
            --preview) channel=preview ;;
            --user) mode=user ;;
            -h|--help) usage; exit 0 ;;
            *) fail "Unknown option: $arg" ;;
        esac
    done
    [ "$(uname -s)" = Linux ] || fail 'This installer is for Linux. Other systems: https://cbassuarez.com/nus.dev/download/'
    [ "$(uname -m)" = x86_64 ] || fail "nus is not available for $(uname -m) yet."
    need curl; need tar; need sha256sum

    urls=$(assets) || fail 'Could not reach GitHub to find the latest release.'
    archive=$(archive_for $channel)
    if [ -z "$archive" ] && [ $channel = release ]; then
        say 'There is no stable release of nus yet; installing the preview channel.'
        channel=preview
        archive=$(archive_for preview)
    fi
    [ -n "$archive" ] || fail 'Could not find a Linux release of nus.'
    base=${archive%/*}
    deb=
    for url in $urls; do
        case $url in "$base"/nus*_amd64.deb) deb=${url##*/}; break ;; esac
    done

    tmp=$(mktemp -d)
    trap 'rm -rf "$tmp"' EXIT
    trap 'exit 1' INT TERM
    curl -fsSL "$base/SHA256SUMS.txt" -o "$tmp/SHA256SUMS.txt"

    if [ $mode = auto ] && [ -n "$deb" ] && command -v apt-get >/dev/null 2>&1 && command -v dpkg >/dev/null 2>&1; then
        fetch "$deb"
        # apt reads local packages as its own unprivileged user.
        chmod 755 "$tmp"; chmod 644 "$tmp/$deb"
        if [ "$(id -u)" = 0 ]; then sudo=; elif command -v sudo >/dev/null 2>&1; then sudo=sudo
        else fail 'Installing the .deb needs root. Run this as root, or add --user to install for this account only.'; fi
        say "Installing $deb with apt…"
        $sudo apt-get install -y "$tmp/$deb"
        say ''
        say "Installed nus. Open it from your applications, or run 'nus'."
        return
    fi

    name=${archive##*/}
    fetch "$name"
    tar -xzf "$tmp/$name" -C "$tmp"
    folder=$tmp/${name%.tar.gz}
    grep -q -- --uninstall "$folder/install-desktop.sh" 2>/dev/null ||
        fail "${name%.tar.gz} predates this installer. Download it from https://cbassuarez.com/nus.dev/download/"
    sh "$folder/install-desktop.sh"
}

# Nothing runs until the whole script has arrived.
main "$@"
