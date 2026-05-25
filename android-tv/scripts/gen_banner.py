#!/usr/bin/env python3
"""Render the Cal Sans wordmark layer for the Android TV banner.

The banner itself is XML (`res/drawable/banner.xml`, a layer-list): a smooth
vector gradient ground (`banner_bg.xml`) with this wordmark bitmap on top. A
VectorDrawable can't embed a font, so the "Iris /" lockup is rasterised here
from the real Cal Sans TTF onto a TRANSPARENT canvas, then composited over the
vector background by the launcher — keeping the gradient banding-free.

Best practices baked in:
  * Output is 320×180 px @ xhdpi (the Android TV banner spec) → written to
    `drawable-xhdpi/banner_wordmark.png`; `--all` adds the other buckets.
  * Supersampled then downscaled (LANCZOS) for clean antialiased edges.
  * Transparent background — the gradient lives in the vector layer, so no
    8-bit banding on the glow.

Usage:
    python3 android-tv/scripts/gen_banner.py            # xhdpi
    python3 android-tv/scripts/gen_banner.py --all       # every density
    python3 android-tv/scripts/gen_banner.py --ss 6       # heavier supersample

Requires Pillow:  python3 -m pip install --break-system-packages Pillow
(or a venv:  python3 -m venv .venv && .venv/bin/pip install Pillow)
"""

from __future__ import annotations

import argparse
import os
import sys

try:
    from PIL import Image, ImageDraw, ImageFont
except ImportError:
    sys.exit("Pillow is required:  python3 -m pip install --break-system-packages Pillow")

# ── Brand tokens (mirror ui/theme/Color.kt) ─────────────────────────────────
BRAND3 = (240, 143, 232)     # #F08FE8  warm end
BRAND = (165, 141, 255)      # #A58DFF  violet
BRAND2 = (105, 193, 252)     # #69C1FC  cool end
DIM = (114, 116, 123)        # #72747B  fg-dim (the slash)

HERE = os.path.dirname(os.path.abspath(__file__))
RES = os.path.join(HERE, "..", "app", "src", "main", "res")
FONT_PATH = os.path.join(RES, "font", "cal_sans_regular.ttf")

# Banner is 320×180 dp; px per density bucket (xhdpi = the 320×180 spec asset).
DENSITIES = {"mdpi": 1.0, "hdpi": 1.5, "tvdpi": 1.33125, "xhdpi": 2.0, "xxhdpi": 3.0}
BASE_W, BASE_H = 160, 90  # dp; ×2 = 320×180


def lerp(a, b, t):
    return tuple(round(a[i] + (b[i] - a[i]) * t) for i in range(3))


def grad3(t):
    """3-stop brand ramp: brand3 → brand → brand2."""
    return lerp(BRAND3, BRAND, t * 2) if t < 0.5 else lerp(BRAND, BRAND2, (t - 0.5) * 2)


def render(width, height):
    """Transparent canvas with the 'Iris /' lockup centred."""
    img = Image.new("RGBA", (width, height), (0, 0, 0, 0))

    # Auto-fit Cal Sans so the lockup is ~58% of the banner width.
    target_w = width * 0.58
    probe = ImageFont.truetype(FONT_PATH, 100)
    iris_w0, slash_w0 = probe.getlength("Iris"), probe.getlength("/")
    fontsize = int(100 * target_w / (iris_w0 + 100 * 0.10 + slash_w0))
    font = ImageFont.truetype(FONT_PATH, fontsize)
    gap = fontsize * 0.10

    iris_w, slash_w = font.getlength("Iris"), font.getlength("/")
    total_w = iris_w + gap + slash_w
    bbox = font.getbbox("Iris/")
    x0 = (width - total_w) / 2
    y0 = (height - (bbox[3] - bbox[1])) / 2 - bbox[1]

    # "Iris": white into a mask, then pour the brand gradient through it.
    mask = Image.new("L", (width, height), 0)
    ImageDraw.Draw(mask).text((x0, y0), "Iris", font=font, fill=255)
    grad = Image.new("RGBA", (width, height), (0, 0, 0, 0))
    gpix = grad.load()
    gx0, gx1 = x0, x0 + iris_w
    for x in range(width):
        col = grad3(min(1.0, max(0.0, (x - gx0) / (gx1 - gx0)))) + (255,)
        for y in range(height):
            gpix[x, y] = col
    img.paste(grad, (0, 0), mask)

    # "/": dim, same face.
    ImageDraw.Draw(img).text((x0 + iris_w + gap, y0), "/", font=font, fill=DIM + (255,))
    return img


def main():
    ap = argparse.ArgumentParser(description="Render the Iris banner wordmark layer.")
    ap.add_argument("--all", action="store_true", help="every density (default: xhdpi only)")
    ap.add_argument("--ss", type=int, default=4, help="supersample factor (default 4)")
    args = ap.parse_args()

    if not os.path.exists(FONT_PATH):
        sys.exit(f"Cal Sans not found at {FONT_PATH}")

    buckets = DENSITIES if args.all else {"xhdpi": DENSITIES["xhdpi"]}
    for name, scale in buckets.items():
        w, h = round(BASE_W * scale), round(BASE_H * scale)
        out = render(w * args.ss, h * args.ss).resize((w, h), Image.LANCZOS)
        d = os.path.join(RES, f"drawable-{name}")
        os.makedirs(d, exist_ok=True)
        path = os.path.join(d, "banner_wordmark.png")
        out.save(path, "PNG")
        print(f"  wrote {os.path.relpath(path, os.path.join(HERE, '..'))}  ({w}×{h})")
    print("Done. Banner = banner.xml (layer-list: banner_bg vector + this wordmark).")


if __name__ == "__main__":
    main()
