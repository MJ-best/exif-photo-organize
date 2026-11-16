#!/usr/bin/env python3
"""Build every icon, and the character on its own, from one piece of artwork.

    python3 scripts/make-icons.py            # needs Pillow + NumPy

The artwork (`crates/app/assets/icon-source.webp`) is a rounded square with a soft glow on a
white background. This script

1. finds the outline of the rounded square and fits a symmetric shape to it,
2. grows the picture outwards past that outline, so that it can be cut to any shape
   (the outline of the artwork is not perfectly symmetric, and Android / Google Play cut
   their own shape out of a full square),
3. writes
   - the desktop icons into `crates/app/assets/` (see the README there),
   - the Android adaptive icon (foreground, themed monochrome) and the in-app mascot into
     `android/app/src/main/res/`,
   - the Google Play icon into `docs/play-store/graphics/`.

The store pictures that show the icon are made by `scripts/make-store-images.py`.
"""
import os
import sys

import numpy as np
from PIL import Image, ImageDraw, ImageFilter

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ASSETS = f"{ROOT}/crates/app/assets"
ANDROID = f"{ROOT}/android/app/src/main/res"
PLAY = f"{ROOT}/docs/play-store/graphics"
SRC = sys.argv[1] if len(sys.argv) > 1 else f"{ASSETS}/icon-source.webp"

ART = 1024  # the rounded square, normalised to ART x ART
WIDE = ART * 3 // 2  # the same picture grown to 1.5x (an Android adaptive icon is 108dp around 72dp)

im = Image.open(SRC).convert("RGB")
a = np.asarray(im).astype(np.float64)
h, w, _ = a.shape

# ---------------------------------------------------------------------------------------------
# 1. The outline: walk in from each side of every row and column to the first sharp colour step
#    (the glow is smooth, the edge of the tile is not), then fit a rounded rectangle whose
#    corners are superellipse arcs: centre, half width / height, corner size R and exponent n.
# ---------------------------------------------------------------------------------------------


def first_edge(line, start, stop, step):
    d = np.abs(line[2:] - line[:-2]).sum(axis=1)  # d[i] is the step across pixel i + 1
    for i in range(start, stop, step):
        if d[i] > 70:
            j = i + int(np.argmax(d[i : i + 5])) if step > 0 else i - 4 + int(np.argmax(d[i - 4 : i + 1]))
            win = d[j - 3 : j + 4]
            return (win * np.arange(j - 3, j + 4)).sum() / win.sum() + 1
    return None


pts = []
m = w // 20
for y in range(m, h - m, 2):
    for x in (first_edge(a[y], m, w // 2, 1), first_edge(a[y], w - m, w // 2, -1)):
        if x is not None:
            pts.append((x, y))
for x in range(m, w - m, 2):
    for y in (first_edge(a[:, x], m, h // 2, 1), first_edge(a[:, x], h - m, h // 2, -1)):
        if y is not None:
            pts.append((x, y))
pts = np.array(pts)


def outline(p, x, y):
    """0 on the outline, negative inside, in source pixels."""
    cx, cy, ax, ay, r, n = p
    qx = np.maximum(np.abs(x - cx) - (ax - r), 0)
    qy = np.maximum(np.abs(y - cy) - (ay - r), 0)
    return (qx**n + qy**n) ** (1 / n) - r


def fit(points):
    x0, x1, y0, y1 = points[:, 0].min(), points[:, 0].max(), points[:, 1].min(), points[:, 1].max()
    best = None
    for r0 in (0.4, 0.6, 0.75):
        for n0 in (2.0, 2.5, 3.5):
            p = np.array([(x0 + x1) / 2, (y0 + y1) / 2, (x1 - x0) / 2, (y1 - y0) / 2, r0 * (x1 - x0) / 2, n0])
            q = points
            for it in range(80):
                res = outline(p, q[:, 0], q[:, 1])
                jac = np.zeros((len(q), 6))
                for k in range(6):
                    dp = np.zeros(6)
                    dp[k] = 1e-3
                    jac[:, k] = (outline(p + dp, q[:, 0], q[:, 1]) - res) / 1e-3
                p = p + np.clip(np.linalg.lstsq(jac, -res, rcond=None)[0], -10, 10)
                p[4] = np.clip(p[4], 0.1 * p[2], 0.98 * min(p[2], p[3]))
                p[5] = np.clip(p[5], 1.5, 8)
                if it in (25, 45):  # drop the few points that landed on something else
                    q = points[np.abs(outline(p, points[:, 0], points[:, 1])) < 3]
            rms = np.sqrt(np.mean(outline(p, q[:, 0], q[:, 1]) ** 2))
            if best is None or rms < best[0]:
                best = (rms, p)
    return best


rms, P = fit(pts)
cx, cy, ax, ay, R, N = P
RN = R / ((ax + ay) / 2)  # corner size as a fraction of the half side, for the normalised shape
print(f"outline: centre {cx:.1f},{cy:.1f}  size {2 * ax:.1f} x {2 * ay:.1f}  corner {R:.1f} (n {N:.2f})  rms {rms:.2f}px")

# ---------------------------------------------------------------------------------------------
# 2. Grow the picture outwards. Pixels at least 5 px inside the fitted outline are the picture
#    (the real outline is up to 3 px inside the fit at two corners, and its edge is anti-aliased
#    against the glow); everything else is filled from its nearest known neighbours, at quarter
#    resolution because what grows out of a smooth picture is smooth.
# ---------------------------------------------------------------------------------------------

pad = int(max(0, 1.5 * ax - cx, 1.5 * ay - cy, cx + 1.5 * ax - w, cy + 1.5 * ay - h)) + 8
ap = np.pad(a, ((pad, pad), (pad, pad), (0, 0)), mode="edge")
yy, xx = np.mgrid[0 : ap.shape[0], 0 : ap.shape[1]]
dist = outline(P, xx - pad, yy - pad)
known = dist < -5


def grow(rgb, known):
    q = 4
    hh, ww = (rgb.shape[0] // q) * q, (rgb.shape[1] // q) * q
    k = known[:hh, :ww].reshape(hh // q, q, ww // q, q).sum(axis=(1, 3)).astype(np.float64)
    c = (rgb[:hh, :ww] * known[:hh, :ww, None]).reshape(hh // q, q, ww // q, q, 3).sum(axis=(1, 3))
    have = k > 0
    c[have] /= k[have][:, None]
    fixed, given = have.copy(), c.copy()
    while not have.all():
        acc = np.zeros_like(c)
        cnt = np.zeros(have.shape)
        cp = np.pad(c * have[..., None], ((1, 1), (1, 1), (0, 0)))
        hp = np.pad(have, 1).astype(np.float64)
        for dy in (0, 1, 2):
            for dx in (0, 1, 2):
                if (dy, dx) != (1, 1):
                    acc += cp[dy : dy + have.shape[0], dx : dx + have.shape[1]]
                    cnt += hp[dy : dy + have.shape[0], dx : dx + have.shape[1]]
        new = ~have & (cnt > 0)
        c[new] = acc[new] / cnt[new][:, None]
        have |= new
    # That fill leaves streaks along the diagonals: relax it (each new cell towards the mean of
    # its neighbours) until they are gone.
    for _ in range(400):
        cp = np.pad(c, ((1, 1), (1, 1), (0, 0)), mode="edge")
        c = (cp[:-2, 1:-1] + cp[2:, 1:-1] + cp[1:-1, :-2] + cp[1:-1, 2:]) / 4
        c[fixed] = given[fixed]
    up = Image.fromarray(np.clip(c, 0, 255).astype(np.uint8)).resize((ww, hh), Image.BICUBIC)
    out = rgb.copy()
    out[:hh, :ww] = np.asarray(up).astype(np.float64)
    return out


grown = grow(ap, known)
keep = np.clip((-dist - 5) / 3, 0, 1)[..., None]  # 3 px of cross-fade inside the known edge
full = ap * keep + grown * (1 - keep)

box = (cx + pad - 1.5 * ax, cy + pad - 1.5 * ay, cx + pad + 1.5 * ax, cy + pad + 1.5 * ay)
wide = Image.fromarray(np.clip(full, 0, 255).astype(np.uint8)).resize((WIDE, WIDE), Image.LANCZOS, box=box)
wide = wide.filter(ImageFilter.UnsharpMask(radius=1.6, percent=45, threshold=2))  # it was enlarged ~1.27x
art = wide.crop(((WIDE - ART) // 2,) * 2 + ((WIDE + ART) // 2,) * 2)


_shape = None


def shape(size):
    """The fitted outline, made symmetric, as an anti-aliased mask of `size` px."""
    global _shape
    if _shape is None:
        v = (np.arange(4096, dtype=np.float32) + 0.5) / 4096 * 2 - 1
        q = np.maximum(np.abs(v) - (1 - RN), 0) ** N
        _shape = Image.fromarray(((q[None, :] + q[:, None]) <= RN**N).astype(np.uint8) * 255)
    return _shape.resize((size, size), Image.LANCZOS)


def tile(size):
    """The icon as a rounded square of `size` px on a transparent background."""
    t = art.resize((size, size), Image.LANCZOS).convert("RGBA")
    t.putalpha(shape(size))
    return t


def framed(size, body, shadow):
    """`body` px tile centred on a `size` canvas, optional soft drop shadow (macOS style)."""
    canvas = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    t = tile(body)
    off = (size - body) // 2
    if shadow:
        sh = Image.new("RGBA", (size, size), (0, 0, 0, 0))
        dark = Image.new("RGBA", (body, body), (60, 20, 40, 255))
        dark.putalpha(t.getchannel("A").point(lambda v: int(v * 0.30)))
        sh.paste(dark, (off, off + max(1, size // 100)), dark)
        canvas = Image.alpha_composite(canvas, sh.filter(ImageFilter.GaussianBlur(max(1, size / 70))))
    layer = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    layer.paste(t, (off, off))
    return Image.alpha_composite(canvas, layer)


# ---------------------------------------------------------------------------------------------
# 3. The character: everything inside the tile that is not the saturated gradient (the body,
#    the eyes, the lens), and the two little dashes.
# ---------------------------------------------------------------------------------------------

mx, mn = a.max(axis=2), a.min(axis=2)
sat = (mx - mn) / np.maximum(mx, 1)
edge_dist = outline(P, *np.meshgrid(np.arange(w), np.arange(h)))
inside = edge_dist < -1.5
plain = Image.fromarray(((sat < 0.48) & inside).astype(np.uint8) * 255)
# Open it by 3 px: drops stray pixels and the slivers of glow where the real outline sits
# inside the fitted one.
plain = plain.filter(ImageFilter.MinFilter(7)).filter(ImageFilter.MaxFilter(7))


def components(mask):
    """Connected regions of a 0/255 image, largest first, as boolean arrays."""
    work = mask.copy()
    out = []
    for _ in range(64):
        ys, xs = np.nonzero(np.asarray(work) == 255)
        if len(ys) == 0:
            break
        ImageDraw.floodfill(work, (int(xs[0]), int(ys[0])), 128)
        region = np.asarray(work) == 128
        if region.sum() > 400:
            out.append(region)
        # .copy(): an image made from an array is read-only, and floodfill then silently does nothing
        work = Image.fromarray(np.where(region, 0, np.asarray(work)).astype(np.uint8)).copy()
    return sorted(out, key=lambda r: -r.sum())


def filled(region):
    """`region` with its holes (the eyes) closed."""
    img = Image.fromarray(np.pad(region, 1).astype(np.uint8) * 255).copy()
    ImageDraw.floodfill(img, (0, 0), 128)
    return (np.asarray(img) != 128)[1:-1, 1:-1]


regions = components(plain)
body = filled(regions[0])
dashes = np.zeros_like(body)
for r in regions[1:]:
    if edge_dist[r].max() < -20:  # not the pale rim along the top edge of the tile
        dashes |= r
print(f"character: body {body.sum()} px, dashes {dashes.sum()} px")


def soft(region):
    """Alpha for `region`: 1 inside, following the colour across its 2 px edge, and stopping
    4 px short of the tile's outline (where the picture is mixed with the glow outside)."""
    img = Image.fromarray(region.astype(np.uint8) * 255)
    core = np.asarray(img.filter(ImageFilter.MinFilter(5))) > 0
    band = (np.asarray(img.filter(ImageFilter.MaxFilter(5))) > 0) & ~core
    alpha = np.where(core, 1.0, np.where(band, np.clip((0.58 - sat) / 0.48, 0, 1), 0.0))
    return alpha * np.clip((-edge_dist - 4) / 1.5, 0, 1), core


def to_art(arr, mode):
    """A source-sized array, cut to the tile and scaled like `art`."""
    return Image.fromarray(arr, mode).resize((ART, ART), Image.LANCZOS, box=(cx - ax, cy - ay, cx + ax, cy + ay))


body_alpha, body_core = soft(body)
clean = grow(a, body_core)  # the body's own colours carried out across its edge (no pink fringe)
rgb = np.where(body_core[..., None], a, clean)
character = to_art(np.clip(rgb, 0, 255).astype(np.uint8), "RGB").convert("RGBA")
character = character.filter(ImageFilter.UnsharpMask(radius=1.6, percent=45, threshold=2))
character.putalpha(to_art((body_alpha * 255).astype(np.uint8), "L"))
character = character.crop(character.getbbox())

# The lens: a ring of greyish pixels inside the body; fit a circle to them (Kasa).
ring = body & (mx > 150) & (mx < 226) & (sat < 0.3)
ring &= np.asarray(Image.fromarray(body.astype(np.uint8) * 255).filter(ImageFilter.MinFilter(21))) > 0
ys, xs = np.nonzero(ring)
for _ in range(3):
    A = np.c_[2 * xs, 2 * ys, np.ones(len(xs))]
    sol = np.linalg.lstsq(A, xs**2 + ys**2, rcond=None)[0]
    lx, ly = sol[0], sol[1]
    lr = np.sqrt(sol[2] + lx**2 + ly**2)
    ok = np.abs(np.hypot(xs - lx, ys - ly) - lr) < 8
    xs, ys = xs[ok], ys[ok]
print(f"lens: centre {lx:.1f},{ly:.1f}  radius {lr:.1f}  ({len(xs)} px)")

# Themed (monochrome) glyph: body and dashes, with the eyes and the rim of the lens cut out.
gy, gx = np.mgrid[0:h, 0:w]
rim = np.clip(1 - (np.abs(np.hypot(gx - lx, gy - ly) - lr) - 5.5), 0, 1)
eyes = np.clip((150 - mx) / 70, 0, 1) * body
dash_alpha, _ = soft(dashes)
glyph = np.clip(np.maximum(body_alpha, dash_alpha) * (1 - eyes) * (1 - rim), 0, 1)
glyph = to_art((glyph * 255).astype(np.uint8), "L")

# ---------------------------------------------------------------------------------------------
# 4. Write everything.
# ---------------------------------------------------------------------------------------------

# macOS: Big Sur grid — an 824 px tile on a 1024 canvas with a soft shadow.
mac = framed(1024, 824, True)
mac.save(f"{ASSETS}/icon-macos-1024.png", optimize=True)
mac.save(f"{ASSETS}/AppIcon.icns", sizes=[(16, 16), (32, 32), (64, 64), (128, 128), (256, 256), (512, 512), (1024, 1024)])

# Windows / Linux / in-app: nearly full bleed.
big = framed(1024, 984, False)
big.save(f"{ASSETS}/icon-1024.png", optimize=True)
big.resize((512, 512), Image.LANCZOS).save(f"{ASSETS}/icon-512.png", optimize=True)
big.resize((256, 256), Image.LANCZOS).save(f"{ASSETS}/icon-256.png", optimize=True)
big.save(f"{ASSETS}/icon.ico", sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])

# The character on its own (store pictures, the website).
character.save(f"{ASSETS}/character.png", optimize=True)

# Android adaptive icon: 108dp, of which the launcher shows the middle 72dp in its own shape.
# Foreground = the grown picture with the tile exactly on those 72dp. Monochrome = the glyph,
# smaller, so that all of it stays inside the 66dp safe circle.
MONO_TILE = 52 / 108  # the tile's side as a fraction of the canvas
MONO_LIFT = 4.5 / 108  # the glyph is bottom-heavy: lift it to sit in the middle
for name, px in (("mdpi", 108), ("hdpi", 162), ("xhdpi", 216), ("xxhdpi", 324), ("xxxhdpi", 432)):
    d = f"{ANDROID}/mipmap-{name}"
    os.makedirs(d, exist_ok=True)
    wide.resize((px, px), Image.LANCZOS).save(f"{d}/ic_launcher_foreground.png", optimize=True)
    side = round(px * MONO_TILE)
    alpha = Image.new("L", (px, px), 0)
    alpha.paste(glyph.resize((side, side), Image.LANCZOS), ((px - side) // 2, (px - side) // 2 - round(px * MONO_LIFT)))
    mono = Image.new("RGBA", (px, px), (255, 255, 255, 0))
    mono.putalpha(alpha)
    mono.save(f"{d}/ic_launcher_monochrome.png", optimize=True)
os.makedirs(f"{ANDROID}/drawable-nodpi", exist_ok=True)
tile(512).save(f"{ANDROID}/drawable-nodpi/mascot.png", optimize=True)

# Google Play: a full 512 px square (Play cuts its own rounded shape).
art.resize((512, 512), Image.LANCZOS).save(f"{PLAY}/icon-512.png", optimize=True)

mid = art.getpixel((ART // 2, ART // 4))
print("done. gradient colour for android ic_launcher_background: #%02X%02X%02X" % mid)
