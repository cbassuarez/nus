#!/usr/bin/env bash
set -euo pipefail
app="$1"
channel="$2"
if [[ -z "${MACOS_CERTIFICATE:-}" ]]; then
  [[ "$channel" != stable ]] || { echo 'Stable releases require Developer ID signing and notarization.' >&2; exit 1; }
  echo 'Preview retains its ad-hoc signature.'
  exit 0
fi
: "${MACOS_CERTIFICATE_PASSWORD:?Missing certificate password}"
: "${MACOS_SIGN_IDENTITY:?Missing Developer ID identity}"
: "${APPLE_API_KEY:?Missing notarization API key}"
: "${APPLE_API_KEY_ID:?Missing notarization API key ID}"
: "${APPLE_API_ISSUER:?Missing notarization API issuer}"
scratch=$(mktemp -d)
keychain="$scratch/release.keychain-db"
keychain_password=$(openssl rand -hex 32)
cleanup() { security delete-keychain "$keychain" >/dev/null 2>&1 || true; rm -rf "$scratch"; }
trap cleanup EXIT
printf '%s' "$MACOS_CERTIFICATE" | base64 --decode > "$scratch/certificate.p12"
printf '%s' "$APPLE_API_KEY" > "$scratch/AuthKey.p8"
chmod 600 "$scratch/certificate.p12" "$scratch/AuthKey.p8"
security create-keychain -p "$keychain_password" "$keychain"
security set-keychain-settings -lut 21600 "$keychain"
security unlock-keychain -p "$keychain_password" "$keychain"
security import "$scratch/certificate.p12" -k "$keychain" -P "$MACOS_CERTIFICATE_PASSWORD" -T /usr/bin/codesign >/dev/null
security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$keychain_password" "$keychain" >/dev/null
cat > "$scratch/entitlements.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>com.apple.security.cs.allow-jit</key><true/>
<key>com.apple.security.cs.allow-unsigned-executable-memory</key><true/>
<key>com.apple.security.cs.disable-library-validation</key><true/>
</dict></plist>
PLIST
sign() { codesign --force --timestamp --options runtime --keychain "$keychain" --sign "$MACOS_SIGN_IDENTITY" --entitlements "$scratch/entitlements.plist" "$1"; }
# Sign nested Mach-O code first, then its enclosing bundles. --deep is for
# verification, never a substitute for inside-out signing.
while IFS= read -r -d '' path; do
  if file -b "$path" | grep -q 'Mach-O'; then sign "$path"; fi
done < <(find "$app/Contents" -type f -perm -111 -print0)
sign "$app/Contents/Frameworks/Chromium Embedded Framework.framework"
for helper in "$app/Contents/Frameworks/"*Helper*.app; do sign "$helper"; done
sign "$app"
codesign --verify --deep --strict "$app"
ditto -c -k --sequesterRsrc --keepParent "$app" "$scratch/notarize.zip"
xcrun notarytool submit "$scratch/notarize.zip" --key "$scratch/AuthKey.p8" --key-id "$APPLE_API_KEY_ID" --issuer "$APPLE_API_ISSUER" --wait --timeout 30m
xcrun stapler staple "$app"
xcrun stapler validate "$app"
spctl --assess --type execute --verbose "$app"
