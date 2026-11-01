"""Cut the rounded-square artwork out of its black background and build the app icons.

usage: python3 scripts/make-icons.py crates/app/assets/icon-source.webp crates/app/assets
(needs Pillow + NumPy)
"""
import sys
from collections import deque

import numpy as np
from PIL import Image, ImageFilter

src, out = sys.argv[1], sys.argv[2]
im = Image.open(src).convert("RGB")
a = np.asarray(im).astype(np.int32)
h, w, _ = a.shape

# 1. Exterior = near-black pixels connected to the border (flood fill).
dark = a.max(axis=2) < 60
ext = np.zeros((h, w), bool)
q = deque()
for x in range(w):
    for y in (0, h - 1):
        if dark[y, x] and not ext[y, x]:
            ext[y, x] = True
            q.append((y, x))
for y in range(h):
    for x in (0, w - 1):
        if dark[y, x] and not ext[y, x]:
            ext[y, x] = True
            q.append((y, x))
while q:
    y, x = q.popleft()
    for dy, dx in ((1, 0), (-1, 0), (0, 1), (0, -1)):
        ny, nx = y + dy, x + dx
        if 0 <= ny < h and 0 <= nx < w and dark[ny, nx] and not ext[ny, nx]:
            ext[ny, nx] = True
            q.append((ny, nx))

# 2. Grow the exterior a little to drop the dark anti-aliased fringe, then soften for AA.
mask = Image.fromarray(np.where(ext, 0, 255).astype(np.uint8))
mask = mask.filter(ImageFilter.MinFilter(9))  # erode the shape by 4 px
mask = mask.filter(ImageFilter.GaussianBlur(1.2))
rgba = im.convert("RGBA")
rgba.putalpha(mask)
bbox = mask.point(lambda v: 255 if v > 8 else 0).getbbox()
art = rgba.crop(bbox)
side = max(art.size)
sq = Image.new("RGBA", (side, side), (0, 0, 0, 0))
sq.paste(art, ((side - art.width) // 2, (side - art.height) // 2))
print("artwork bbox", bbox, "->", sq.size)


def framed(size, body, shadow):
    """`body` px artwork centred on a `size` canvas, optional soft drop shadow (macOS style)."""
    canvas = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    art_s = sq.resize((body, body), Image.LANCZOS)
    off = (size - body) // 2
    if shadow:
        sh = Image.new("RGBA", (size, size), (0, 0, 0, 0))
        alpha = art_s.getchannel("A").point(lambda v: int(v * 0.30))
        black = Image.new("RGBA", (body, body), (40, 25, 15, 255))
        black.putalpha(alpha)
        sh.paste(black, (off, off + max(1, size // 100)), black)
        sh = sh.filter(ImageFilter.GaussianBlur(max(1, size / 70)))
        canvas = Image.alpha_composite(canvas, sh)
    layer = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    layer.paste(art_s, (off, off))
    return Image.alpha_composite(canvas, layer)


# macOS: Big Sur grid — 824 px body on a 1024 canvas with a soft shadow.
mac = framed(1024, 824, True)
mac.save(f"{out}/icon-macos-1024.png")
mac.save(f"{out}/AppIcon.icns", sizes=[(16, 16), (32, 32), (64, 64), (128, 128), (256, 256), (512, 512), (1024, 1024)])

# Windows / Linux / in-app: nearly full bleed.
full = framed(1024, 984, False)
full.save(f"{out}/icon-1024.png")
full.resize((256, 256), Image.LANCZOS).save(f"{out}/icon-256.png", optimize=True)
full.resize((512, 512), Image.LANCZOS).save(f"{out}/icon-512.png", optimize=True)
full.save(f"{out}/icon.ico", sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
print("done")
