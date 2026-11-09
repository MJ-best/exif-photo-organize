#!/usr/bin/env bash
# Sign "LOUPE.app" — and notarize it when the Apple secrets are available.
#
# Usage: scripts/macos-sign.sh "dist/LOUPE.app"
#
# Without a Developer ID certificate the app gets an ad-hoc signature (it runs, but macOS shows
# "Apple은 … 악성 코드가 없음을 확인할 수 없습니다" until the user allows it once).
#
# With these environment variables (GitHub repository secrets, see README "macOS 공증"):
#   MACOS_CERTIFICATE_P12        base64 of the exported "Developer ID Application" certificate (.p12)
#   MACOS_CERTIFICATE_PASSWORD   password of that .p12
# the app is signed with the hardened runtime, and additionally with
#   APPLE_ID, APPLE_TEAM_ID, APPLE_APP_PASSWORD   (app-specific password from appleid.apple.com)
# it is notarized by Apple and the ticket is stapled, so it opens without any warning.
set -euo pipefail
APP="$1"
BIN="$APP/Contents/MacOS/photo-organize"

if [[ -z "${MACOS_CERTIFICATE_P12:-}" ]]; then
  echo "No Developer ID certificate configured: ad-hoc signature (not notarized)."
  codesign --force --deep --sign - "$APP"
  codesign --verify --verbose "$APP"
  exit 0
fi

TMP="${RUNNER_TEMP:-$(mktemp -d)}"
KEYCHAIN="$TMP/signing.keychain-db"
KEYCHAIN_PASSWORD="$(openssl rand -hex 16)"
echo "$MACOS_CERTIFICATE_P12" | base64 --decode > "$TMP/certificate.p12"
security create-keychain -p "$KEYCHAIN_PASSWORD" "$KEYCHAIN"
security set-keychain-settings -lut 21600 "$KEYCHAIN"
security unlock-keychain -p "$KEYCHAIN_PASSWORD" "$KEYCHAIN"
security import "$TMP/certificate.p12" -k "$KEYCHAIN" -P "${MACOS_CERTIFICATE_PASSWORD:-}" -T /usr/bin/codesign
rm -f "$TMP/certificate.p12"
security set-key-partition-list -S apple-tool:,apple: -s -k "$KEYCHAIN_PASSWORD" "$KEYCHAIN" >/dev/null
# Put the temporary keychain first in the search list so codesign finds the identity.
security list-keychains -d user -s "$KEYCHAIN" $(security list-keychains -d user | tr -d '"')

IDENTITY="$(security find-identity -v -p codesigning "$KEYCHAIN" | grep -m1 'Developer ID Application' | sed -E 's/.*"(.*)"/\1/')"
if [[ -z "$IDENTITY" ]]; then
  echo "error: no 'Developer ID Application' identity in MACOS_CERTIFICATE_P12" >&2
  exit 1
fi
echo "Signing with: $IDENTITY"
# Inside out: the executable, then the bundle. Hardened runtime + secure timestamp are required
# for notarization.
codesign --force --options runtime --timestamp --sign "$IDENTITY" "$BIN"
codesign --force --options runtime --timestamp --sign "$IDENTITY" "$APP"
codesign --verify --strict --verbose=2 "$APP"

if [[ -n "${APPLE_ID:-}" && -n "${APPLE_TEAM_ID:-}" && -n "${APPLE_APP_PASSWORD:-}" ]]; then
  ditto -c -k --keepParent "$APP" "$TMP/notarize.zip"
  xcrun notarytool submit "$TMP/notarize.zip" \
    --apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" --password "$APPLE_APP_PASSWORD" --wait
  xcrun stapler staple "$APP"
  xcrun stapler validate "$APP"
  spctl --assess --type execute --verbose=2 "$APP"
  echo "Notarized and stapled."
else
  echo "Signed with Developer ID but not notarized (APPLE_ID / APPLE_TEAM_ID / APPLE_APP_PASSWORD not set)."
fi
