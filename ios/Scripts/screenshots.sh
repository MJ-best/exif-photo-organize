#!/usr/bin/env bash
# Store screenshots of the iOS app on the iOS Simulator (iPhone 6.9" · iPad 13"), with the sample
# card in docs/screenshots/sample-card (real photos and their XMP sidecars).
#
#   ./ios/Scripts/build-rust-ios.sh --sim-only --debug && (cd ios && xcodegen generate)
#   ./ios/Scripts/screenshots.sh [output folder]     # default: docs/ios-app-store/screenshots
#
# 1. Imports the card into a library with the same engine (photoorg CLI: import + analyze).
# 2. Builds the Debug app, installs it on an iPhone and an iPad simulator and copies the library
#    into the app's Documents/LOUPE (the on-device library).
# 3. Launches the app once per screen with `-LoupeScene <scene>` (Loupe/App/ScreenshotScene.swift)
#    and captures it with `simctl io screenshot` (status bar 9:41, full battery).
# The import screen starts from an empty library with the card copied to Documents/<CARD_NAME>.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="${1:-$ROOT/docs/ios-app-store/screenshots}"
CARD="$ROOT/docs/screenshots/sample-card"
CARD_NAME="NIKON Z f"
BUNDLE="io.github.mj-best.loupe"
WAIT="${LOUPE_SCREENSHOT_WAIT:-10}"
WORK="$(mktemp -d)"
LIB="$WORK/LOUPE"
mkdir -p "$OUT"

# file | scene | photo (file name start) | appearance
IPHONE_SHOTS=(
  "iphone-1-library|library||light"
  "iphone-2-viewer|viewer|DSC_1930|light"
  "iphone-3-info|info|DSC_0606|light"
  "iphone-4-composition|composition|DSC_1926|light"
  "iphone-6-cocoa|library||dark"
)
IPAD_SHOTS=(
  "ipad-1-library|library||light"
  "ipad-2-viewer|viewer|DSC_0606|light"
  "ipad-3-composition|composition|DSC_1929|light"
  "ipad-5-cocoa|viewer|DSC_1930|dark"
)
# Import screens (empty library, the card in Documents).
IPHONE_IMPORT="iphone-5-import"
IPAD_IMPORT="ipad-4-import"

echo "==> Library from the sample card (photoorg CLI)"
# Debug is enough for five photos (dependencies are optimised in dev, ~10 s) and reuses the
# host build of build-rust-ios.sh.
cargo build -q -p photoorg-cli --manifest-path "$ROOT/Cargo.toml"
PHOTOORG="$ROOT/target/debug/photoorg"
"$PHOTOORG" import "$CARD" "$LIB"
"$PHOTOORG" analyze "$LIB"
"$PHOTOORG" cull "$LIB" --threshold 0 || true

# Newest available simulator whose name matches $1 (newest iOS runtime first).
pick_device() {
  xcrun simctl list devices available -j | python3 -c '
import json, re, sys
pat = re.compile(sys.argv[1])
nums = lambda s: [int(n) for n in re.findall(r"\d+", s)]
devices = json.load(sys.stdin)["devices"]
for rt in sorted((r for r in devices if ".iOS-" in r), key=nums, reverse=True):
    found = [d for d in devices[rt] if d.get("isAvailable") and pat.search(d["name"])]
    if found:
        best = max(found, key=lambda d: nums(d["name"]))
        print(best["udid"], best["name"], sep="\t")
        sys.exit(0)
sys.exit(1)
' "$1"
}

IFS=$'\t' read -r IPHONE IPHONE_NAME < <(pick_device '^iPhone \d+ Pro Max$')
IFS=$'\t' read -r IPAD IPAD_NAME < <(pick_device '^iPad Pro 13-inch')
echo "==> iPhone: $IPHONE_NAME ($IPHONE) · iPad: $IPAD_NAME ($IPAD)"

echo "==> Building the Debug app"
xcodebuild build -quiet \
  -project "$ROOT/ios/Loupe.xcodeproj" -scheme Loupe -configuration Debug \
  -destination "id=$IPHONE" -derivedDataPath "$WORK/dd" \
  CODE_SIGNING_ALLOWED=NO
APP="$WORK/dd/Build/Products/Debug-iphonesimulator/Loupe.app"

boot() {
  xcrun simctl boot "$1" 2>/dev/null || true
  xcrun simctl bootstatus "$1" -b >/dev/null
  xcrun simctl status_bar "$1" override --time "9:41" \
    --dataNetwork wifi --wifiMode active --wifiBars 3 \
    --cellularMode active --cellularBars 4 \
    --batteryState charged --batteryLevel 100
}

# A fresh install with the library (`library`) or with an empty library and the card (`card`).
fresh() {
  xcrun simctl terminate "$1" "$BUNDLE" 2>/dev/null || true
  xcrun simctl uninstall "$1" "$BUNDLE" 2>/dev/null || true
  xcrun simctl install "$1" "$APP"
  local data
  data="$(xcrun simctl get_app_container "$1" "$BUNDLE" data)"
  mkdir -p "$data/Documents"
  if [ "$2" = library ]; then
    cp -R "$LIB" "$data/Documents/LOUPE"
  else
    cp -R "$CARD" "$data/Documents/$CARD_NAME"
  fi
}

# shoot <udid> <file> <scene> <photo> <appearance>
shoot() {
  local args=(-LoupeScene "$3")
  if [ -n "$4" ]; then args+=(-LoupePhoto "$4"); fi
  if [ "$3" = import ]; then args+=(-LoupeCard "$CARD_NAME"); fi
  xcrun simctl ui "$1" appearance "$5"
  xcrun simctl launch "$1" "$BUNDLE" "${args[@]}" >/dev/null
  sleep "$WAIT"
  xcrun simctl io "$1" screenshot --type=png "$WORK/$2.png" >/dev/null
  xcrun simctl terminate "$1" "$BUNDLE" 2>/dev/null || true
  # JPEG keeps the repository small; App Store Connect takes .jpg at the same size.
  sips -s format jpeg -s formatOptions 92 "$WORK/$2.png" --out "$OUT/$2.jpg" >/dev/null
  echo "    $2.jpg ($(sips -g pixelWidth -g pixelHeight "$OUT/$2.jpg" | awk '/pixel/ {printf "%s ", $2}'))"
}

# run_device <udid> <import file> <shots...>
run_device() {
  local udid="$1" import_file="$2"
  shift 2
  boot "$udid"
  fresh "$udid" library
  local s file scene photo look
  for s in "$@"; do
    IFS='|' read -r file scene photo look <<<"$s"
    shoot "$udid" "$file" "$scene" "$photo" "$look"
  done
  fresh "$udid" card
  shoot "$udid" "$import_file" import "" light
  xcrun simctl shutdown "$udid" || true
}

echo "==> $IPHONE_NAME"
run_device "$IPHONE" "$IPHONE_IMPORT" "${IPHONE_SHOTS[@]}"
echo "==> $IPAD_NAME"
run_device "$IPAD" "$IPAD_IMPORT" "${IPAD_SHOTS[@]}"

rm -rf "$WORK"
echo "==> Screenshots in $OUT"
