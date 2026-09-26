#!/usr/bin/env bash
# Builds CEF for nus with MP4/H.264/AAC *support* but none of FFmpeg's
# patent-pool decoders: H.264 is decoded by VideoToolbox and AAC by
# AudioToolbox (patches/chromium-152), which macOS licenses. Streaming sites
# (Netflix, Prime Video) need both; VP9, AV1 and Opus were already there.
#
#   scripts/build-cef-royalty-free.sh            macOS arm64 → vendor/cef
#   CEF_BUILD_DIR=/Volumes/big/cef scripts/build-cef-royalty-free.sh
#
# Needs Xcode, ~100 GB free and several hours. Pinned to the CEF that
# vendor/cef-rs expects (152.0.6+g708dc14, Chromium 152.0.7977.83).
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
[[ "$(uname -s)" == Darwin ]] || { echo "macOS only for now (the AAC patch is AudioToolbox)" >&2; exit 1; }
work="${CEF_BUILD_DIR:-$HOME/cef_build}"
branch=7977
commit=708dc14
arch_flag=--arm64-build
[[ "$(uname -m)" == x86_64 ]] && arch_flag=--x64-build
mkdir -p "$work"
cd "$work"
[[ -f automate-git.py ]] || curl -fsSLO https://bitbucket.org/chromiumembedded/cef/raw/master/tools/automate/automate-git.py

# proprietary_codecs: MP4 demuxing and H.264/AAC parsing, and the types are
# reported as playable. ffmpeg_branding=Chromium: FFmpeg without its H.264
# and AAC decoders, so the platform decoders are the only ones.
export GN_DEFINES="is_official_build=true proprietary_codecs=true ffmpeg_branding=Chromium chrome_pgo_phase=0"
export CEF_ARCHIVE_FORMAT=tar.bz2
common=(--download-dir="$work/code" --branch="$branch" --checkout="$commit" "$arch_flag" --minimal-distrib --no-debug-build)

echo "· sync (first run downloads Chromium, ~30 GB)"
python3 automate-git.py "${common[@]}" --no-build --no-distrib

echo "· patch Chromium: AAC through AudioToolbox"
src="$work/code/chromium/src"
patch="$root/patches/chromium-152/mac-aac-audiotoolbox.patch"
if git -C "$src" apply --reverse --check "$patch" 2>/dev/null; then
  echo "  already applied"
else
  git -C "$src" apply "$patch"
fi

echo "· build (hours)"
python3 automate-git.py "${common[@]}" --no-update --force-build --force-distrib

archive=$(ls -t "$src/cef/binary_distrib/"cef_binary_*_minimal.tar.bz2 | head -1)
echo "· export $archive → vendor/cef"
(cd "$root/vendor/cef-rs" && cargo run --release -p export-cef-dir -- --force --archive "$archive" "$root/vendor/cef")
echo "Done. Rebuild the app: scripts/bundle-mac.sh"
