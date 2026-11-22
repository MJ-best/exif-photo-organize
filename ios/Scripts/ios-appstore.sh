#!/usr/bin/env bash
# Build "LOUPE" for the iOS / iPadOS App Store: Release arm64 static XCFramework + SwiftUI app,
# verified against App Store Connect ITMS checks (PrivacyInfo.xcprivacy, opaque AppIcon, dSYM,
# zero embedded static frameworks), signed as an .ipa, and optionally uploaded via altool.
#
# Needs macOS with Xcode 16+, XcodeGen, rustup target aarch64-apple-ios, and:
#   TEAM_ID             10-character Apple Developer Team ID
#   APP_IDENTITY        "Apple Distribution: <name> (<TEAM_ID>)" in keychain (for manual signing)
#
# Signing modes (supports both):
#   1. Manual (recommended for CI): set PROFILE=/path/to/LOUPE_iOS_AppStore.mobileprovision
#   2. Automatic via App Store Connect API Key (when PROFILE is unset):
#        ASC_KEY_ID, ASC_ISSUER_ID, and ~/.appstoreconnect/private_keys/AuthKey_<ASC_KEY_ID>.p8
#        (reuses the imported Apple Distribution certificate + auto-manages the iOS profile)
#
# Optional:
#   BUNDLE_ID   default io.github.mj-best.loupe (Universal Purchase with Mac App Store; override
#               with io.github.mj-best.loupe.ios if selling iOS/iPadOS separately)
#   BUILD       build number, strictly increasing per upload (default: YYMMDDHHMM)
#
# Modes:
#   ios/Scripts/ios-appstore.sh             build & export signed .ipa (upload with Transporter)
#   ios/Scripts/ios-appstore.sh --upload    build, export, validate, and upload to App Store Connect
#   ios/Scripts/ios-appstore.sh --dry-run   unsigned Release arm64 .xcarchive + ITMS preflight audit

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

MODE="${1:-}"
BUNDLE_ID="${BUNDLE_ID:-io.github.mj-best.loupe}"
BUILD="${BUILD:-$(date +%y%m%d%H%M)}"
VERSION="$(grep -m1 '^version' Cargo.toml | sed -E 's/.*"(.*)".*/\1/')"
OUT="$ROOT/dist-ios-appstore"
ARCHIVE="$OUT/LOUPE.xcarchive"
EXPORT_DIR="$OUT/export"

rm -rf "$OUT"
mkdir -p "$OUT"

echo "==> LOUPE iOS/iPadOS $VERSION ($BUILD), bundle=$BUNDLE_ID, mode=${MODE:-signed-ipa}"

# 0. Pre-flight check on source AppIcon (ITMS-90717 forbids alpha channel in iOS App Store icon).
ICON_PATH="$ROOT/ios/Loupe/Resources/Assets.xcassets/AppIcon.appiconset/AppIcon-1024.png"
if [[ ! -f "$ICON_PATH" ]]; then
  echo "ERROR: Missing iOS AppIcon at $ICON_PATH" >&2
  exit 1
fi
if command -v sips >/dev/null 2>&1; then
  HAS_ALPHA="$(sips -g hasAlpha "$ICON_PATH" | awk '/hasAlpha:/{print $2}')"
  if [[ "$HAS_ALPHA" != "no" ]]; then
    echo "ERROR: AppIcon-1024.png has an alpha channel ($HAS_ALPHA); run ios/Scripts/sync-assets.sh" >&2
    exit 1
  fi
fi

# 1. Build Release arm64 Rust static library + Swift UniFFI bindings + LoupeCore.xcframework.
"$ROOT/ios/Scripts/build-rust-ios.sh" --device-only

# 2. Generate Xcode project from ios/project.yml.
echo "==> Generating ios/Loupe.xcodeproj via XcodeGen…"
(cd "$ROOT/ios" && xcodegen generate)

# 3. Archive Release build for generic iOS device (arm64).
XCODE_VERSION="$(xcodebuild -version | awk '/^Xcode/{print $2}')"
IOS_SDK_VERSION="$(xcrun --sdk iphoneos --show-sdk-version)"
echo "==> Archiving with Xcode $XCODE_VERSION (iOS SDK $IOS_SDK_VERSION)…"

ARCHIVE_ARGS=(
  -project "$ROOT/ios/Loupe.xcodeproj"
  -scheme Loupe
  -configuration Release
  -destination "generic/platform=iOS"
  -archivePath "$ARCHIVE"
  PRODUCT_BUNDLE_IDENTIFIER="$BUNDLE_ID"
  MARKETING_VERSION="$VERSION"
  CURRENT_PROJECT_VERSION="$BUILD"
)

SIGNING_MODE="dry-run"
PROFILE_NAME=""
ASC_KEY_PATH=""

if [[ "$MODE" == "--dry-run" ]]; then
  ARCHIVE_ARGS+=(
    CODE_SIGNING_ALLOWED=NO
    CODE_SIGNING_REQUIRED=NO
    CODE_SIGN_IDENTITY=""
  )
else
  : "${TEAM_ID:?set TEAM_ID (10-char Apple Developer Team ID)}"
  ARCHIVE_ARGS+=(DEVELOPMENT_TEAM="$TEAM_ID")

  if [[ -n "${PROFILE:-}" ]]; then
    SIGNING_MODE="manual"
    if [[ ! -f "$PROFILE" ]]; then
      echo "ERROR: Provisioning profile not found at $PROFILE" >&2
      exit 1
    fi
    DECODED_PLIST="$OUT/profile.plist"
    security cms -D -i "$PROFILE" > "$DECODED_PLIST"
    PROFILE_UUID="$(/usr/libexec/PlistBuddy -c 'Print :UUID' "$DECODED_PLIST")"
    PROFILE_NAME="$(/usr/libexec/PlistBuddy -c 'Print :Name' "$DECODED_PLIST")"
    rm -f "$DECODED_PLIST"

    PROFILES_DIR="$HOME/Library/MobileDevice/Provisioning Profiles"
    mkdir -p "$PROFILES_DIR"
    cp "$PROFILE" "$PROFILES_DIR/${PROFILE_UUID}.mobileprovision"
    echo "==> Installed provisioning profile: $PROFILE_NAME ($PROFILE_UUID)"

    APP_IDENTITY="${APP_IDENTITY:-Apple Distribution}"
    ARCHIVE_ARGS+=(
      CODE_SIGN_STYLE=Manual
      CODE_SIGN_IDENTITY="$APP_IDENTITY"
      PROVISIONING_PROFILE_SPECIFIER="$PROFILE_NAME"
    )
  else
    SIGNING_MODE="automatic"
    : "${ASC_KEY_ID:?set ASC_KEY_ID (or provide PROFILE for manual signing)}"
    : "${ASC_ISSUER_ID:?set ASC_ISSUER_ID (or provide PROFILE for manual signing)}"
    ASC_KEY_PATH="${ASC_KEY_PATH:-$HOME/.appstoreconnect/private_keys/AuthKey_${ASC_KEY_ID}.p8}"
    if [[ ! -f "$ASC_KEY_PATH" ]]; then
      echo "ERROR: App Store Connect API key not found at $ASC_KEY_PATH" >&2
      exit 1
    fi
    echo "==> Using Xcode automatic distribution signing via App Store Connect API key ($ASC_KEY_ID)"
    ARCHIVE_ARGS+=(
      CODE_SIGN_STYLE=Automatic
      -allowProvisioningUpdates
      -authenticationKeyPath "$ASC_KEY_PATH"
      -authenticationKeyID "$ASC_KEY_ID"
      -authenticationKeyIssuerID "$ASC_ISSUER_ID"
    )
  fi
fi

xcodebuild archive "${ARCHIVE_ARGS[@]}"

# 4. ITMS pre-flight verification on the archived .app bundle.
APP_BUNDLE="$ARCHIVE/Products/Applications/Loupe.app"
if [[ ! -d "$APP_BUNDLE" ]]; then
  echo "ERROR: Archived app bundle missing at $APP_BUNDLE" >&2
  exit 1
fi

echo "==> Running App Store ITMS pre-flight checks on Loupe.app…"
if [[ ! -f "$APP_BUNDLE/PrivacyInfo.xcprivacy" ]]; then
  echo "ERROR: PrivacyInfo.xcprivacy missing from Loupe.app bundle (ITMS-91053 risk)" >&2
  exit 1
fi
plutil -lint "$APP_BUNDLE/PrivacyInfo.xcprivacy"

BUILT_BUNDLE_ID="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$APP_BUNDLE/Info.plist")"
BUILT_VERSION="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$APP_BUNDLE/Info.plist")"
BUILT_BUILD="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleVersion' "$APP_BUNDLE/Info.plist")"
BUILT_ENC="$(/usr/libexec/PlistBuddy -c 'Print :ITSAppUsesNonExemptEncryption' "$APP_BUNDLE/Info.plist")"
echo "   Bundle ID : $BUILT_BUNDLE_ID"
echo "   Version   : $BUILT_VERSION ($BUILT_BUILD)"
echo "   NonExempt : $BUILT_ENC"

if [[ "$BUILT_BUNDLE_ID" != "$BUNDLE_ID" ]]; then
  echo "ERROR: CFBundleIdentifier mismatch ($BUILT_BUNDLE_ID != $BUNDLE_ID)" >&2
  exit 1
fi
if [[ -d "$APP_BUNDLE/Frameworks/LoupeCore.xcframework" ]]; then
  echo "ERROR: Static LoupeCore.xcframework must NOT be embedded in Frameworks/" >&2
  exit 1
fi
if [[ ! -d "$ARCHIVE/dSYMs/Loupe.app.dSYM" ]]; then
  echo "ERROR: Missing dSYM bundle in archive ($ARCHIVE/dSYMs/Loupe.app.dSYM)" >&2
  exit 1
fi

echo "==> Linked dynamic libraries (verifying zero unexpected private frameworks):"
otool -L "$APP_BUNDLE/Loupe"

if [[ "$MODE" == "--dry-run" ]]; then
  ZIP_PATH="$OUT/LOUPE-iOS-${VERSION}-${BUILD}-dry-run.xcarchive.zip"
  ditto -c -k --sequesterRsrc --keepParent "$ARCHIVE" "$ZIP_PATH"
  echo "==> Dry run succeeded! Unsigned archive verified: $ZIP_PATH"
  exit 0
fi

# 5. Export signed App Store .ipa.
EXPORT_OPTIONS="$OUT/ExportOptions.plist"
if [[ "$SIGNING_MODE" == "manual" ]]; then
  cat > "$EXPORT_OPTIONS" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>method</key>
    <string>app-store-connect</string>
    <key>destination</key>
    <string>export</string>
    <key>signingStyle</key>
    <string>manual</string>
    <key>signingCertificate</key>
    <string>Apple Distribution</string>
    <key>teamID</key>
    <string>${TEAM_ID}</string>
    <key>provisioningProfiles</key>
    <dict>
        <key>${BUNDLE_ID}</key>
        <string>${PROFILE_NAME}</string>
    </dict>
    <key>stripSwiftSymbols</key>
    <true/>
    <key>uploadSymbols</key>
    <true/>
</dict>
</plist>
EOF
else
  cat > "$EXPORT_OPTIONS" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>method</key>
    <string>app-store-connect</string>
    <key>destination</key>
    <string>export</string>
    <key>signingStyle</key>
    <string>automatic</string>
    <key>teamID</key>
    <string>${TEAM_ID}</string>
    <key>stripSwiftSymbols</key>
    <true/>
    <key>uploadSymbols</key>
    <true/>
</dict>
</plist>
EOF
fi

EXPORT_ARGS=(
  -exportArchive
  -archivePath "$ARCHIVE"
  -exportPath "$EXPORT_DIR"
  -exportOptionsPlist "$EXPORT_OPTIONS"
)
if [[ "$SIGNING_MODE" == "automatic" ]]; then
  EXPORT_ARGS+=(
    -allowProvisioningUpdates
    -authenticationKeyPath "$ASC_KEY_PATH"
    -authenticationKeyID "$ASC_KEY_ID"
    -authenticationKeyIssuerID "$ASC_ISSUER_ID"
  )
fi

echo "==> Exporting signed App Store .ipa…"
xcodebuild "${EXPORT_ARGS[@]}"

RAW_IPA="$(find "$EXPORT_DIR" -maxdepth 1 -name '*.ipa' | head -n 1)"
if [[ -z "$RAW_IPA" || ! -f "$RAW_IPA" ]]; then
  echo "ERROR: xcodebuild -exportArchive did not produce an .ipa in $EXPORT_DIR" >&2
  exit 1
fi

IPA="$OUT/LOUPE-iOS-${VERSION}-${BUILD}.ipa"
mv "$RAW_IPA" "$IPA"
echo "==> Signed App Store IPA ready: $IPA ($(du -h "$IPA" | awk '{print $1}'))"

# 6. Validate and upload to App Store Connect when --upload is requested.
if [[ "$MODE" == "--upload" ]]; then
  : "${ASC_KEY_ID:?set ASC_KEY_ID}" "${ASC_ISSUER_ID:?set ASC_ISSUER_ID}"
  echo "==> Validating IPA with App Store Connect…"
  xcrun altool --validate-app --type ios --file "$IPA" --apiKey "$ASC_KEY_ID" --apiIssuer "$ASC_ISSUER_ID"
  echo "==> Uploading IPA to App Store Connect…"
  xcrun altool --upload-app --type ios --file "$IPA" --apiKey "$ASC_KEY_ID" --apiIssuer "$ASC_ISSUER_ID"
  echo "==> Uploaded! Select build $BUILD for version $VERSION in App Store Connect (TestFlight / App Store)."
fi
