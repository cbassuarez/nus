#!/usr/bin/env bash
# Builds CEF for nus with MP4/H.264/AAC *support* but none of FFmpeg's
# software H.264/AAC decoders: decoding uses VideoToolbox and AudioToolbox
# (patches/chromium-152). This is a system-codec build, not a determination
# of patent or distribution obligations. It does not grant DRM/provider
# acceptance. macOS streaming pages use system WebKit without this build.
#
#   scripts/build-cef-royalty-free.sh            macOS arm64 → vendor/cef
#   CEF_BUILD_DIR=/Volumes/big/cef scripts/build-cef-royalty-free.sh
#
# Needs Xcode, ~70 GB free and several hours. Pinned to the CEF that
# vendor/cef-rs expects (152.0.6+g708dc14, Chromium 152.0.7977.83).
# Chromium comes without its git history (the full history alone is ~70 GB)
# and is built without debug symbols (they roughly double the output).
# Safe to run again after a failure: it resumes where it stopped.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
[[ "$(uname -s)" == Darwin ]] || { echo "macOS only for now (the AAC patch is AudioToolbox)" >&2; exit 1; }
work="${CEF_BUILD_DIR:-$HOME/cef_build}"

# LiteRT keeps prebuilt binaries in Git LFS; Chromium doesn't use them and
# expects the pointer files. A global LFS filter (`git lfs install`) with no
# git-lfs on PATH fails that checkout, and the whole sync with it. A blank
# process turns the filter off, smudge included, for this build's git alone.
export GIT_CONFIG_COUNT=2 \
  GIT_CONFIG_KEY_0=filter.lfs.process GIT_CONFIG_VALUE_0= \
  GIT_CONFIG_KEY_1=filter.lfs.required GIT_CONFIG_VALUE_1=false
branch=7977
commit=708dc14
arch_flag=--arm64-build
[[ "$(uname -m)" == x86_64 ]] && arch_flag=--x64-build
mkdir -p "$work"
cd "$work"

# Space, before hours of work rather than after: a first run needs ~70 GB
# (Chromium ~30, toolchains ~5, the build ~35); a resumed one, less.
need=70
[[ -d "$work/code/chromium/src/out" ]] && need=30
avail=$(df -Pk "$work" | awk 'NR==2 {print int($4/1024/1024)}')
if (( avail < need )) && [[ "${CEF_SKIP_SPACE_CHECK:-0}" != 1 ]]; then
  echo "Only ${avail} GB free at $work; this needs about ${need} GB." >&2
  echo "Free some space, or build elsewhere: CEF_BUILD_DIR=/Volumes/…/cef $0" >&2
  exit 1
fi
[[ -f automate-git.py ]] || curl -fsSLO https://bitbucket.org/chromiumembedded/cef/raw/master/tools/automate/automate-git.py

# proprietary_codecs: MP4 demuxing and H.264/AAC parsing, and the types are
# reported as playable. ffmpeg_branding=Chromium: FFmpeg without its H.264
# and AAC decoders, so the platform decoders are the only ones. Chromium
# asserts against the pair; a patch lifts it.
# symbol_level=0: no debug symbols (they'd double the build; nus ships none).
export GN_DEFINES="is_official_build=true proprietary_codecs=true ffmpeg_branding=Chromium chrome_pgo_phase=0 symbol_level=0 blink_symbol_level=0 v8_symbol_level=0"
export CEF_ARCHIVE_FORMAT=tar.bz2
common=(--download-dir="$work/code" --branch="$branch" --checkout="$commit" "$arch_flag" --minimal-distrib --no-debug-build --no-chromium-history)

echo "· sync (first run downloads Chromium without history, ~30 GB)"
python3 automate-git.py "${common[@]}" --no-build --no-distrib

# Without history, automate-git.py syncs Chromium's dependencies only in the
# run that creates src, so a sync that died partway is never finished: src sits
# at the right tag without V8 and the rest, and CEF's patches fail on them.
# gclient writes .gclient_entries only once every dependency is in. A
# dependency cut off mid-clone is subtler: it keeps a .git with nothing checked
# out, and gclient, which trusts src's gitlinks over the disk, leaves it so.
# Those are cleared for gclient to clone again.
chromium="$work/code/chromium"
src="$chromium/src"
broken=$(git -C "$src" ls-files -s | awk '$1 == 160000 {print $4}' | while read -r dep; do
  [[ -e "$src/$dep/.git" ]] || continue
  git -C "$src/$dep" rev-parse -q --verify HEAD >/dev/null || echo "$dep"
done)
if [[ ! -f "$chromium/.gclient_entries" || -n "$broken" ]]; then
  echo "· finish the Chromium sync an earlier run left incomplete"
  for dep in $broken; do
    echo "  clone again: $dep"
    rm -rf "${src:?}/$dep"
  done
  (cd "$chromium" && export PATH="$work/code/depot_tools:$PATH" DEPOT_TOOLS_UPDATE=0 &&
    gclient sync --nohooks --no-history && gclient runhooks)
fi

echo "· patch Chromium: AAC through AudioToolbox, and let GN build it"
for patch in "$root"/patches/chromium-152/*.patch; do
  if git -C "$src" apply --reverse --check "$patch" 2>/dev/null; then
    echo "  $(basename "$patch"): already applied"
  else
    git -C "$src" apply "$patch"
    echo "  $(basename "$patch"): applied"
  fi
done

echo "· build (hours)"
python3 automate-git.py "${common[@]}" --no-update --force-build --force-distrib

archive=$(ls -t "$src/cef/binary_distrib/"cef_binary_*_minimal.tar.bz2 | head -1)
echo "· export $archive → vendor/cef"
# Unpacked beside the current CEF and swapped in only when complete, so a
# failed export never leaves the app without one.
rm -rf "$root/vendor/cef.new"
(cd "$root/vendor/cef-rs" && cargo run --release -p export-cef-dir -- --force --archive "$archive" "$root/vendor/cef.new")
[[ -d "$root/vendor/cef.new/Chromium Embedded Framework.framework" ]] || { echo "export incomplete: $root/vendor/cef.new" >&2; exit 1; }
rm -rf "$root/vendor/cef.old"
[[ -d "$root/vendor/cef" ]] && mv "$root/vendor/cef" "$root/vendor/cef.old"
mv "$root/vendor/cef.new" "$root/vendor/cef"
rm -rf "$root/vendor/cef.old"
echo "Done. Rebuild the app: scripts/bundle-mac.sh"
