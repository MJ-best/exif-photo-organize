#!/usr/bin/env bash
# Copies canonical LOUPE icon and mascot assets from crates/app/assets into the iOS Asset Catalog,
# flattening the iOS App Store icon to opaque 24-bit RGB (no alpha channel — ITMS-90717).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
SRC_ICON="$ROOT/crates/app/assets/icon-1024.png"
DST_ICON="$ROOT/ios/Loupe/Resources/Assets.xcassets/AppIcon.appiconset/AppIcon-1024.png"

python3 - "$SRC_ICON" "$DST_ICON" <<'PY'
import math, struct, sys
from PIL import Image

src, dst = sys.argv[1], sys.argv[2]
img = Image.open(src).convert("RGBA")
w, h = img.size
px = img.load()
cx, cy = (w - 1) / 2.0, (h - 1) / 2.0
out = Image.new("RGB", (w, h))
opx = out.load()

for y in range(h):
    for x in range(w):
        r, g, b, a = px[x, y]
        if a == 255:
            opx[x, y] = (r, g, b)
            continue
        dx, dy = cx - x, cy - y
        dist = math.hypot(dx, dy)
        bg = (253, 246, 236)
        if dist > 0:
            ux, uy = dx / dist, dy / dist
            for s in range(1, int(dist) + 1):
                sx = int(round(x + ux * s))
                sy = int(round(y + uy * s))
                if 0 <= sx < w and 0 <= sy < h:
                    sr, sg, sb, sa = px[sx, sy]
                    if sa >= 250:
                        bg = (sr, sg, sb)
                        break
        alpha = a / 255.0
        opx[x, y] = (
            int(round(r * alpha + bg[0] * (1.0 - alpha))),
            int(round(g * alpha + bg[1] * (1.0 - alpha))),
            int(round(b * alpha + bg[2] * (1.0 - alpha))),
        )

out.save(dst, format="PNG", optimize=True)
with open(dst, "rb") as f:
    raw = f.read(32)
_, _, _, ct = struct.unpack(">IIBB", raw[16:26])
assert ct == 2, f"Expected opaque RGB PNG (color_type=2), got {ct}"
PY

cp "$ROOT/crates/app/assets/character.png" "$ROOT/ios/Loupe/Resources/Assets.xcassets/Mascot.imageset/mascot.png"
echo "Synced iOS opaque AppIcon & mascot assets."
