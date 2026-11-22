#!/usr/bin/env bash
# Copies canonical LOUPE icon and mascot assets from crates/app/assets into the iOS Asset Catalog.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cp "$ROOT/crates/app/assets/icon-1024.png" "$ROOT/ios/Loupe/Resources/Assets.xcassets/AppIcon.appiconset/AppIcon-1024.png"
cp "$ROOT/crates/app/assets/character.png" "$ROOT/ios/Loupe/Resources/Assets.xcassets/Mascot.imageset/mascot.png"
echo "Synced iOS icon & mascot assets."
