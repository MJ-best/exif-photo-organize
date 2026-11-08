#!/usr/bin/env bash
# Try the Mac App Store flavour on this Mac before having an Apple Developer account: it runs in
# the App Sandbox exactly as the store build will (ad-hoc signed with the sandbox entitlements).
#
# What to check (docs/app-store/README.md, "샌드박스 미리 확인"):
#   - choose a library folder, quit, start again: it opens without asking
#   - plug in an SD card: the "사용 허락" dialog, choose the card once; next time it opens directly
#   - 폴더에서 가져오기, 내보내기, 작품 폴더 (Finder에서 열기), 카드 추출
#
# Its settings live in ~/Library/Containers/io.github.mj-best.photo-organize (separate from the
# website build). Usage: scripts/macos-sandbox-test.sh
set -euo pipefail
cd "$(dirname "$0")/.."
OUT=dist-sandbox-test
rm -rf "$OUT"
cargo build --release -p photoorg-app --no-default-features
scripts/macos-bundle.sh target/release/photo-organize "$OUT"
APP="$OUT/Photo Organize.app"
codesign --force --sign - --entitlements packaging/macos/sandbox-test.entitlements "$APP"
codesign --display --entitlements - "$APP"
echo "Opening the sandboxed build (log: Console.app, process photo-organize)…"
open "$APP"
