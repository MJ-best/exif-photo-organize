#!/usr/bin/env python3
"""Compose the store pictures that carry the icon's colours and the character.

    cd site && npm ci && cd ..               # once: the fonts (Pretendard, SIL OFL)
    python3 scripts/make-store-images.py     # needs Pillow + NumPy

Writes

- `docs/app-store/screenshots/<name>.jpg` (2880 x 1800, Mac App Store): each app window from
  `docs/app-store/screenshots/raw/` on the icon's gradient, under a headline, with the character
  looking over the top of the window. The headlines are in SHOTS below.
- `docs/play-store/graphics/feature-graphic.png` (1024 x 500, Google Play).

The raw pictures are plain captures of the app (no personal folder names: cut the bottom of the
sidebar off). To add one, drop it into `raw/` and add a line to SHOTS.
The icons themselves come from `scripts/make-icons.py`.
"""
import os
import sys

import numpy as np
from PIL import Image, ImageDraw, ImageFilter, ImageFont

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ASSETS = f"{ROOT}/crates/app/assets"
SHOTS_DIR = f"{ROOT}/docs/app-store/screenshots"
PLAY = f"{ROOT}/docs/play-store/graphics"
FONTS = f"{ROOT}/site/node_modules/pretendard/dist/public/static"

# name (raw/<name>.jpg), headline, line under it
SHOTS = [
    ("real-1-acut", "내 눈을 닮아 가는 A컷 추천", "픽 · 별점을 학습해 취향에 맞는 좋은 컷부터 먼저 추천합니다"),
    ("real-3-focus", "왜 B컷인지, 근거를 한눈에", "초점 · 흔들림 · 수직선 · 연사 순위까지 기계적으로 짚어 줍니다"),
    ("real-4-portrait", "인물 · 구도 · 시선까지 살핍니다", "얼굴 초점과 헤드룸 · 시선 앞 공간 · 수직선까지 놓치지 않습니다"),
    ("real-5-rated", "별점 매기고 사진집으로 바로", "별점을 매긴 사진을 작품과 테마별로 골라 사진집에 모읍니다"),
    ("real-8-gather", "한 폴더에 모아 라이트룸으로", "사진집의 파일을 한 폴더로 옮기고 별점 · 테마 XMP 도 함께 넘깁니다"),
    ("real-6-crop", "자동 수평 · 구도 크롭까지", "수평 · 수직 구도 모델이 기울기를 맞추고 3분할로 알맞게 잘라 줍니다"),
    ("real-7-scene", "장면마다 다른 기준으로, 구도까지", "동물 · 인물 · 정물 등 장면을 알아보고 기울기와 3분할 구도를 살핍니다"),
    ("real-2-viewer", "노출 과다 · 노이즈도 바로 걸러 냅니다", "흔들리고 날아간 B컷을 먼저 모아 두고, 결정은 언제나 사진가가 합니다"),
]

if not os.path.exists(f"{FONTS}/Pretendard-Bold.otf"):
    sys.exit("fonts not found: run `npm ci` in site/ first (Pretendard comes with the site's packages)")


def font(weight, size):
    return ImageFont.truetype(f"{FONTS}/Pretendard-{weight}.otf", size)


# ---------------------------------------------------------------------------------------------
# The backdrop: the icon's own gradient with the character and the dashes taken out. Keep the
# saturated pixels of the tile, fill the rest in from them on a small grid (it is smooth).
# ---------------------------------------------------------------------------------------------


def gradient():
    g = 96
    icon = np.asarray(Image.open(f"{ASSETS}/icon-1024.png").convert("RGBA")).astype(np.float64)
    rgb, alpha = icon[..., :3], icon[..., 3]
    mx, mn = rgb.max(axis=2), rgb.min(axis=2)
    keep = (alpha > 254) & ((mx - mn) / np.maximum(mx, 1) > 0.5)
    keep = np.asarray(Image.fromarray(keep.astype(np.uint8) * 255).filter(ImageFilter.MinFilter(17))) > 0
    q = icon.shape[0] // g
    n = keep[: g * q, : g * q].reshape(g, q, g, q).sum(axis=(1, 3)).astype(np.float64)
    c = (rgb[: g * q, : g * q] * keep[: g * q, : g * q, None]).reshape(g, q, g, q, 3).sum(axis=(1, 3))
    have = n > 0
    c[have] /= n[have][:, None]
    fixed, given = have.copy(), c.copy()
    while not have.all():
        cp = np.pad(c * have[..., None], ((1, 1), (1, 1), (0, 0)))
        hp = np.pad(have, 1).astype(np.float64)
        acc, cnt = np.zeros_like(c), np.zeros(have.shape)
        for dy in (0, 1, 2):
            for dx in (0, 1, 2):
                acc += cp[dy : dy + g, dx : dx + g]
                cnt += hp[dy : dy + g, dx : dx + g]
        new = ~have & (cnt > 0)
        c[new] = acc[new] / cnt[new][:, None]
        have |= new
    for _ in range(600):
        cp = np.pad(c, ((1, 1), (1, 1), (0, 0)), mode="edge")
        c = (cp[:-2, 1:-1] + cp[2:, 1:-1] + cp[1:-1, :-2] + cp[1:-1, 2:]) / 4
        c[fixed] = given[fixed]
    return Image.fromarray(np.clip(c, 0, 255).astype(np.uint8))


GRADIENT = gradient()
CHARACTER = Image.open(f"{ASSETS}/character.png").convert("RGBA")


def backdrop(w, h):
    """The gradient over w x h: its middle band (less of the yellow corner, which white text
    does not show on), smoothed."""
    g = GRADIENT.size[0]
    return GRADIENT.resize((w, h), Image.BICUBIC, box=(g * 0.10, g * 0.14, g * 0.97, g * 0.86)).filter(
        ImageFilter.GaussianBlur(max(w, h) / 90)
    )


def rounded(size, radius):
    s = 4
    m = Image.new("L", (size[0] * s, size[1] * s), 0)
    ImageDraw.Draw(m).rounded_rectangle((0, 0, size[0] * s - 1, size[1] * s - 1), radius * s, fill=255)
    return m.resize(size, Image.LANCZOS)


def shadow(canvas, box, radius, blur, offset, strength):
    """A soft shadow of a rounded rectangle `box` on `canvas` (RGB), in place."""
    x0, y0, x1, y1 = box
    pad = blur * 3
    layer = Image.new("L", (x1 - x0 + pad * 2, y1 - y0 + pad * 2), 0)
    ImageDraw.Draw(layer).rounded_rectangle((pad, pad, pad + x1 - x0, pad + y1 - y0), radius, fill=int(255 * strength))
    layer = layer.filter(ImageFilter.GaussianBlur(blur))
    dark = Image.new("RGB", layer.size, (70, 10, 50))
    canvas.paste(dark, (x0 - pad, y0 - pad + offset), layer)


def text(canvas, xy, s, f, fill=(255, 255, 255), lift=0.16):
    """Text with a faint soft shadow, so that white stays readable on the light end of the gradient."""
    x, y = xy
    box = ImageDraw.Draw(canvas).textbbox((x, y), s, font=f)
    pad = f.size
    layer = Image.new("L", (box[2] - box[0] + pad * 2, box[3] - box[1] + pad * 2), 0)
    ImageDraw.Draw(layer).text((pad - (box[0] - x), pad - (box[1] - y)), s, font=f, fill=int(255 * lift))
    layer = layer.filter(ImageFilter.GaussianBlur(f.size / 9))
    canvas.paste(Image.new("RGB", layer.size, (120, 20, 60)), (box[0] - pad, box[1] - pad + f.size // 22), layer)
    ImageDraw.Draw(canvas).text((x, y), s, font=f, fill=fill)
    return box


def dashes(canvas, x, y, k):
    """The icon's two little strokes, with (x, y) at their lower right and `k` px of thickness."""
    d = ImageDraw.Draw(canvas)
    for (ax, ay), (bx, by) in (((-3.9, -1.9), (-2.5, -0.5)), ((-1.45, -3.0), (-0.9, -1.25))):
        d.line((x + ax * k, y + ay * k, x + bx * k, y + by * k), fill=(255, 255, 255), width=round(k))
        for px, py in ((ax, ay), (bx, by)):
            d.ellipse((x + px * k - k / 2, y + py * k - k / 2, x + px * k + k / 2, y + py * k + k / 2), fill=(255, 255, 255))


def character(width):
    h = round(CHARACTER.height * width / CHARACTER.width)
    return CHARACTER.resize((width, h), Image.LANCZOS)


# ---------------------------------------------------------------------------------------------
# Mac App Store screenshots
# ---------------------------------------------------------------------------------------------

W, H = 2880, 1800
MARGIN = 150
TOP = 336  # top of the window
AREA_H = H - TOP - 70  # the most the window may take

for name, head, sub in SHOTS:
    raw = Image.open(f"{SHOTS_DIR}/raw/{name}.jpg").convert("RGB")
    scale = min((W - 2 * MARGIN) / raw.width, AREA_H / raw.height)
    win = raw.resize((round(raw.width * scale), round(raw.height * scale)), Image.LANCZOS)
    x0 = (W - win.width) // 2
    box = (x0, TOP, x0 + win.width, TOP + win.height)
    radius = 30

    canvas = backdrop(W, H)
    hb = text(canvas, (x0 + 6, 74), head, font("ExtraBold", 108))
    text(canvas, (x0 + 8, hb[3] + 34), sub, font("Medium", 50), fill=(255, 250, 246), lift=0.2)

    # The character looks over the top of the window, on the right.
    ch = character(540)
    cx = box[2] - ch.width - 150
    cy = TOP - round(ch.height * 0.70)
    dashes(canvas, cx + 30, cy + 44, 26)
    canvas.paste(ch, (cx, cy), ch)

    shadow(canvas, box, radius, 46, 30, 0.42)
    canvas.paste(win, box[:2], rounded(win.size, radius))
    canvas.save(f"{SHOTS_DIR}/{name}.jpg", quality=92, subsampling=0)
    print(f"{name}.jpg  window {win.width} x {win.height}")

# ---------------------------------------------------------------------------------------------
# Google Play feature graphic
# ---------------------------------------------------------------------------------------------

FW, FH = 1024, 500
S = 2  # draw at 2x
canvas = backdrop(FW * S, FH * S)
tb = text(canvas, (72 * S, 118 * S), "LOUPE", font("ExtraBold", 92 * S))
text(canvas, (74 * S, tb[3] + 26 * S), "사진가의 서랍,", font("SemiBold", 40 * S))
tb2 = text(canvas, (74 * S, tb[3] + 78 * S), "사진을 고르는 시간", font("SemiBold", 40 * S))
text(canvas, (74 * S, tb2[3] + 30 * S), "카드에서 바로 · 기기 안에서만", font("Medium", 24 * S), fill=(255, 248, 244), lift=0.22)
ch = character(470 * S)
cx, cy = (FW - 500) * S, (FH - 258) * S
dashes(canvas, cx + 40 * S, cy + 30 * S, 22 * S)
canvas.paste(ch, (cx, cy), ch)
canvas.resize((FW, FH), Image.LANCZOS).save(f"{PLAY}/feature-graphic.png", optimize=True)
print("feature-graphic.png")
