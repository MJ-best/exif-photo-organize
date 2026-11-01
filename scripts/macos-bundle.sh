#!/usr/bin/env bash
# Wrap the release binary into "Photo Organize.app".
# Usage: scripts/macos-bundle.sh target/release/photo-organize <out-dir>
set -euo pipefail
BIN="${1:-target/release/photo-organize}"
OUT="${2:-dist}"
VERSION="$(grep -m1 '^version' Cargo.toml | sed -E 's/.*"(.*)".*/\1/')"
APP="$OUT/Photo Organize.app"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/photo-organize"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Photo Organize</string>
  <key>CFBundleDisplayName</key><string>Photo Organize</string>
  <key>CFBundleIdentifier</key><string>io.github.mj-best.photo-organize</string>
  <key>CFBundleExecutable</key><string>photo-organize</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>${VERSION}</string>
  <key>CFBundleVersion</key><string>${VERSION}</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSRemovableVolumesUsageDescription</key><string>SD 카드의 사진을 가져오기 위해 이동식 볼륨에 접근합니다.</string>
  <key>NSDocumentsFolderUsageDescription</key><string>사진 라이브러리 폴더를 읽고 씁니다.</string>
</dict>
</plist>
PLIST
echo "created $APP"
