#!/usr/bin/env bash
# Build "LOUPE" for the Mac App Store: the sandboxed App Store flavour (no license keys,
# no network), universal (Apple Silicon + Intel), signed for the App Store, as an installer package —
# and upload it to App Store Connect with --upload.
#
# Needs a Mac with Xcode and, from the Apple Developer account (docs/app-store/README.md):
#   TEAM_ID             the 10-character team id
#   APP_IDENTITY        "Apple Distribution: <name> (<TEAM_ID>)"                 in the keychain
#   INSTALLER_IDENTITY  "3rd Party Mac Developer Installer: <name> (<TEAM_ID>)"  in the keychain
#   PROFILE             the Mac App Store provisioning profile (.provisionprofile)
# Optional:
#   BUNDLE_ID   default io.github.mj-best.loupe (must match the App ID and the profile; the website
#               build keeps io.github.mj-best.photo-organize so its users' settings and permissions stay)
#   BUILD       build number, larger than every build uploaded before (default: date and time)
#   COPYRIGHT   default "© <year> 은총"
# For --upload: an App Store Connect API key (Users and Access → Integrations):
#   ASC_KEY_ID, ASC_ISSUER_ID, and the key file ~/.appstoreconnect/private_keys/AuthKey_<ASC_KEY_ID>.p8
# Without --upload, send the .pkg with Apple's Transporter app.
# --dry-run needs none of the above: everything but the Apple signatures (ad-hoc signed, unsigned
# package — not for upload), to check the packaging on any Mac or in CI.
#
# Usage: scripts/macos-appstore.sh [--upload | --dry-run]
set -euo pipefail
cd "$(dirname "$0")/.."

MODE="${1:-}"
if [[ "$MODE" == "--dry-run" ]]; then
  TEAM_ID="${TEAM_ID:-TEAMID0000}"
  APP_IDENTITY="-"
  INSTALLER_IDENTITY=""
else
  : "${TEAM_ID:?set TEAM_ID}" "${APP_IDENTITY:?set APP_IDENTITY}" "${INSTALLER_IDENTITY:?set INSTALLER_IDENTITY}"
  : "${PROFILE:?set PROFILE to the .provisionprofile}"
fi
BUNDLE_ID="${BUNDLE_ID:-io.github.mj-best.loupe}"
BUILD="${BUILD:-$(date +%y%m%d%H%M)}"
COPYRIGHT="${COPYRIGHT:-© $(date +%Y) 은총}"
VERSION="$(grep -m1 '^version' Cargo.toml | sed -E 's/.*"(.*)".*/\1/')"
OUT=dist-appstore
rm -rf "$OUT"
mkdir -p "$OUT"

echo "== LOUPE $VERSION ($BUILD) for the Mac App Store, $BUNDLE_ID"

# 1. The App Store flavour for both architectures, merged.
for target in aarch64-apple-darwin x86_64-apple-darwin; do
  cargo build --release -p photoorg-app --no-default-features --target "$target"
done
lipo -create \
  target/aarch64-apple-darwin/release/photo-organize \
  target/x86_64-apple-darwin/release/photo-organize \
  -output "$OUT/photo-organize"

# 2. The bundle, with what App Store Connect expects in Info.plist.
scripts/macos-bundle.sh "$OUT/photo-organize" "$OUT"
APP="$OUT/LOUPE.app"
PLIST="$APP/Contents/Info.plist"
pb() { /usr/libexec/PlistBuddy -c "$1" "$PLIST"; }
pb "Set :CFBundleIdentifier $BUNDLE_ID"
pb "Set :CFBundleVersion $BUILD"
pb "Add :LSApplicationCategoryType string public.app-category.photography"
pb "Add :ITSAppUsesNonExemptEncryption bool false"
pb "Add :NSHumanReadableCopyright string $COPYRIGHT"
# What Xcode writes for every build: App Store Connect reads the SDK and Xcode an app was built
# with from these (the binary itself is linked against the same SDK, through cargo's clang).
SDK_VERSION="$(xcrun --sdk macosx --show-sdk-version)"
SDK_BUILD="$(xcrun --sdk macosx --show-sdk-build-version)"
XCODE_VERSION="$(xcodebuild -version | awk '/^Xcode/{print $2}')"
XCODE_BUILD="$(xcodebuild -version | awk '/^Build version/{print $3}')"
IFS=. read -r XC_MAJOR XC_MINOR XC_PATCH _ <<< "$XCODE_VERSION.0.0"
pb "Add :DTPlatformName string macosx"
pb "Add :DTPlatformVersion string $SDK_VERSION"
pb "Add :DTPlatformBuild string $SDK_BUILD"
pb "Add :DTSDKName string macosx$SDK_VERSION"
pb "Add :DTSDKBuild string $SDK_BUILD"
pb "Add :DTXcode string $(printf '%d%d%d' "$XC_MAJOR" "${XC_MINOR:-0}" "${XC_PATCH:-0}")"
pb "Add :DTXcodeBuild string $XCODE_BUILD"
pb "Add :DTCompiler string com.apple.compilers.llvm.clang.1_0"
pb "Add :BuildMachineOSBuild string $(sw_vers -buildVersion)"
echo "== built with Xcode $XCODE_VERSION ($XCODE_BUILD), macOS SDK $SDK_VERSION ($SDK_BUILD)"
if [[ -n "${PROFILE:-}" ]]; then cp "$PROFILE" "$APP/Contents/embedded.provisionprofile"; fi

# 3. Sign for the App Store, sandboxed.
sed -e "s/__TEAM_ID__/$TEAM_ID/g" -e "s/__BUNDLE_ID__/$BUNDLE_ID/g" \
  packaging/macos/appstore.entitlements > "$OUT/appstore.entitlements"
codesign --force --sign "$APP_IDENTITY" --entitlements "$OUT/appstore.entitlements" "$APP"
codesign --verify --strict --verbose=2 "$APP"
codesign --display --entitlements - "$APP"

# 4. The installer package App Store Connect takes.
PKG="$OUT/LOUPE-$VERSION-$BUILD.pkg"
if [[ -n "$INSTALLER_IDENTITY" ]]; then
  productbuild --component "$APP" /Applications --sign "$INSTALLER_IDENTITY" "$PKG"
else
  productbuild --component "$APP" /Applications "$PKG"
fi
pkgutil --check-signature "$PKG" || true
echo "== $PKG"
/usr/libexec/PlistBuddy -c Print "$PLIST"

# 5. Check it with App Store Connect, and upload.
if [[ "$MODE" == "--upload" ]]; then
  : "${ASC_KEY_ID:?set ASC_KEY_ID}" "${ASC_ISSUER_ID:?set ASC_ISSUER_ID}"
  xcrun altool --validate-app --type macos --file "$PKG" --apiKey "$ASC_KEY_ID" --apiIssuer "$ASC_ISSUER_ID"
  xcrun altool --upload-app --type macos --file "$PKG" --apiKey "$ASC_KEY_ID" --apiIssuer "$ASC_ISSUER_ID"
  echo "== uploaded: choose build $BUILD for version $VERSION in App Store Connect and submit it for review"
fi
