#!/usr/bin/env bash
# Builds nus.app — a double-clickable macOS bundle of the app (spikes/composite).
#
#   scripts/bundle-mac.sh            release build → dist/nus.app
#   scripts/bundle-mac.sh --debug    debug build   → dist/nus.app
#   NUS_BUNDLE_OUT=/tmp/nus.app scripts/bundle-mac.sh  isolated release artifact
#
# CEF on macOS runs only from a bundle: the Chromium Embedded Framework in
# Contents/Frameworks, and one helper app per subprocess role
# ("nus Helper", "nus Helper (GPU)", … — CEF finds them by name), each built
# from the composite_helper binary. The bundle is signed ad hoc, which is what
# Apple silicon needs to run it locally; it is not notarized, so on another
# Mac it has to be opened with right-click → Open the first time.
#
# Run once before: scripts/fetch-cef.sh (vendor/cef).
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
profile=release
[[ "${1:-}" == "--debug" ]] && profile=debug

# shellcheck source=/dev/null
. "$root/scripts/env.sh" >/dev/null
framework="$CEF_PATH/Chromium Embedded Framework.framework"
[[ -d "$framework" ]] || { echo "no CEF at $CEF_PATH — run scripts/fetch-cef.sh" >&2; exit 1; }

name=nus
id=dev.nus.app
version=$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/Cargo.toml" | head -1)
app="${NUS_BUNDLE_OUT:-$root/dist/$name.app}"
[[ "$app" == /* && "$app" == *.app ]] || { echo "NUS_BUNDLE_OUT must be an absolute .app path" >&2; exit 1; }
bin="$root/spikes/composite/target/$profile"

if [[ "${NUS_BUNDLE_SKIP_BUILD:-0}" != 1 ]]; then
  echo "· building ($profile)"
  (cd "$root/spikes/composite" && cargo build $([[ $profile == release ]] && echo --release) --locked --bins -q)
  (cd "$root" && cargo build $([[ $profile == release ]] && echo --release) --locked -p nus-cli -p nus-hold -q)
fi

echo "· laying out $app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources" "$app/Contents/Frameworks"
cp "$bin/composite" "$app/Contents/MacOS/$name"
cp "$root/target/$profile/nus-hold" "$app/Contents/MacOS/nus-hold"
mkdir -p "$app/Contents/Resources/bin"
cp "$root/target/$profile/nus" "$app/Contents/Resources/bin/nus"
mkdir -p "$app/Contents/Resources/licenses"
cp "$root/LICENSE" "$app/Contents/Resources/licenses/nus.txt"
cp "$root/assets/fonts/"{OFL-*.txt,License-*} "$app/Contents/Resources/licenses/"
cp "$root/assets/icons/LICENSE" "$app/Contents/Resources/licenses/icons.txt"
cp "$CEF_PATH/CREDITS.html" "$app/Contents/Resources/licenses/Chromium.html"
ditto "$framework" "$app/Contents/Frameworks/Chromium Embedded Framework.framework"

plist() { # plist <path> <executable> <identifier> <is-helper>
  local helper_keys=""
  [[ "$4" == 1 ]] && helper_keys="<key>LSUIElement</key><string>1</string>"
  local icon_key="<key>CFBundleIconFile</key><string>$name.icns</string>"
  cat > "$1" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>$2</string>
  <key>CFBundleDisplayName</key><string>$2</string>
  <key>CFBundleExecutable</key><string>$2</string>
  <key>CFBundleIdentifier</key><string>$3</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleDevelopmentRegion</key><string>English</string>
  $icon_key
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>LSEnvironment</key><dict><key>MallocNanoZone</key><string>0</string></dict>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSPrefersDisplaySafeAreaCompatibilityMode</key><false/>
  <key>NSSupportsAutomaticGraphicsSwitching</key><true/>
  <key>NSCameraUsageDescription</key><string>A page in nus asked to use the camera.</string>
  <key>NSMicrophoneUsageDescription</key><string>A page in nus asked to use the microphone.</string>
  <key>NSBluetoothAlwaysUsageDescription</key><string>A page in nus asked to use Bluetooth.</string>
  <key>NSWebBrowserPublicKeyCredentialUsageDescription</key><string>A page in nus asked to use a passkey.</string>
  $helper_keys
</dict></plist>
PLIST
}
plist "$app/Contents/Info.plist" "$name" "$id" 0

for role in "" " (GPU)" " (Renderer)" " (Plugin)" " (Alerts)"; do
  helper="$name Helper$role"
  h="$app/Contents/Frameworks/$helper.app"
  mkdir -p "$h/Contents/MacOS"
  cp "$bin/composite_helper" "$h/Contents/MacOS/$helper"
  suffix=$(echo "$role" | tr -d ' ()' | tr '[:upper:]' '[:lower:]')
  plist "$h/Contents/Info.plist" "$helper" "$id.helper${suffix:+.$suffix}" 1
done

echo "· icon (the white n)"
icons=$(mktemp -d)
(cd "$root" && cargo run --release --locked -q -p nus-render --example icon -- "$icons/png" >/dev/null)
set_=$icons/$name.iconset
mkdir -p "$set_"
for s in 16 32 128 256 512; do
  cp "$icons/png/nus-$s-ink.png" "$set_/icon_${s}x${s}.png"
  cp "$icons/png/nus-$((s * 2))-ink.png" "$set_/icon_${s}x${s}@2x.png"
done
iconutil -c icns "$set_" -o "$app/Contents/Resources/$name.icns"
# A content-named resource invalidates Launch Services' old icon reference on
# the next launch, without restarting the user's Dock or clearing global caches.
icon_file="$name-dock-$(shasum -a 256 "$app/Contents/Resources/$name.icns" | cut -c1-12).icns"
mv "$app/Contents/Resources/$name.icns" "$app/Contents/Resources/$icon_file"
/usr/libexec/PlistBuddy -c "Set :CFBundleIconFile $icon_file" "$app/Contents/Info.plist"
# CEF's Alerts helper can own system notification surfaces. Give every helper
# the same packaged identity rather than the generic application placeholder.
for h in "$app/Contents/Frameworks/"*Helper*.app; do
  mkdir -p "$h/Contents/Resources"
  cp "$app/Contents/Resources/$icon_file" "$h/Contents/Resources/$icon_file"
  /usr/libexec/PlistBuddy -c "Set :CFBundleIconFile $icon_file" "$h/Contents/Info.plist"
done
rm -rf "$icons"

echo "· signing (ad hoc)"
# Inside out: the framework, each helper, then the app.
codesign --force --sign - "$app/Contents/Frameworks/Chromium Embedded Framework.framework" >/dev/null 2>&1
for h in "$app/Contents/Frameworks/"*Helper*.app; do
  codesign --force --sign - "$h" >/dev/null 2>&1
done
codesign --force --sign - "$app" >/dev/null 2>&1
codesign --verify --deep --strict "$app"
# Let Finder and Dock observe a bundle update, including an in-place rebuild.
touch "$app"
/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f "$app"

echo "✓ $app ($(du -sh "$app" | cut -f1))"
echo "  open it:   open \"$app\""
echo "  or drag it to /Applications. Profile: ~/Library/Application Support/nus/profile"
