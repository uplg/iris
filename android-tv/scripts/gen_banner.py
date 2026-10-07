#!/usr/bin/env python3
"""Render the Android TV banner: the Iris mark beside "Iris" in Borel, on the night ground.

The banner is a layer-list (`res/drawable/banner.xml`): the flat night ground
(`banner_bg.xml`), the vector mark (`iris_mark.xml`) and the wordmark bitmap. A
VectorDrawable can't embed a font, so "Iris" is rasterised here from the real
Borel TTF onto a transparent canvas. This script lays out the whole lockup and
writes `banner.xml` too, so the mark's offset and the word's position can't
drift apart.

Usage:
    python3 android-tv/scripts/gen_banner.py            # xhdpi
    python3 android-tv/scripts/gen_banner.py --all       # every density

Requires Pillow:  python3 -m pip install --break-system-packages Pillow
"""

from __future__ import annotations

import argparse
import os
import sys

try:
    from PIL import Image, ImageDraw, ImageFont
except ImportError:
    sys.exit("Pillow is required:  python3 -m pip install --break-system-packages Pillow")

# web/src/styles/tokens.css: --raw-cloud on --raw-night
CLOUD = (242, 240, 234)

HERE = os.path.dirname(os.path.abspath(__file__))
RES = os.path.join(HERE, "..", "app", "src", "main", "res")
FONT_PATH = os.path.join(RES, "font", "borel_display_regular.ttf")

# The banner is 160×90 dp (320×180 px at xhdpi, the Android TV spec).
DENSITIES = {"mdpi": 1.0, "hdpi": 1.5, "tvdpi": 1.33125, "xhdpi": 2.0, "xxhdpi": 3.0}
BASE_W, BASE_H = 160, 90
MARK = 40  # dp
GAP = 9  # dp between the mark and the word
WORD_SIZE = 34  # dp, Borel's em


def layout():
    """dp positions of the lockup, centred: (mark_left, word_left, word_width)."""
    probe = ImageFont.truetype(FONT_PATH, 1000)
    word_w = probe.getlength("Iris") * WORD_SIZE / 1000
    total = MARK + GAP + word_w
    mark_left = (BASE_W - total) / 2
    return mark_left, mark_left + MARK + GAP, word_w


def render(scale, ss):
    w, h = round(BASE_W * scale) * ss, round(BASE_H * scale) * ss
    img = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    _, word_left, _ = layout()
    k = scale * ss
    font = ImageFont.truetype(FONT_PATH, round(WORD_SIZE * k))
    left, top, _, bottom = font.getbbox("Iris")
    # Borel's tall ascenders: centre the ink, not the em box
    y = (h - (bottom - top)) / 2 - top
    ImageDraw.Draw(img).text((word_left * k - left, y), "Iris", font=font, fill=CLOUD + (255,))
    return img.resize((round(BASE_W * scale), round(BASE_H * scale)), Image.LANCZOS)


BANNER_XML = """<?xml version="1.0" encoding="utf-8"?>
<!-- Android TV launcher banner, written by scripts/gen_banner.py (edit the script, not this
     file): the night ground, the Iris mark, and "Iris" in Borel as a bitmap (a vector can't
     embed a font). -->
<layer-list xmlns:android="http://schemas.android.com/apk/res/android">
    <item android:drawable="@drawable/banner_bg" />
    <item
        android:width="{mark}dp"
        android:height="{mark}dp"
        android:left="{left}dp"
        android:gravity="left|center_vertical"
        android:drawable="@drawable/iris_mark" />
    <item android:drawable="@drawable/banner_wordmark" />
</layer-list>
"""


def main():
    ap = argparse.ArgumentParser(description="Render the Iris TV banner.")
    ap.add_argument("--all", action="store_true", help="every density (default: xhdpi only)")
    ap.add_argument("--ss", type=int, default=4, help="supersample factor (default 4)")
    args = ap.parse_args()
    if not os.path.exists(FONT_PATH):
        sys.exit(f"Borel not found at {FONT_PATH}")

    buckets = DENSITIES if args.all else {"xhdpi": DENSITIES["xhdpi"]}
    for name, scale in buckets.items():
        d = os.path.join(RES, f"drawable-{name}")
        os.makedirs(d, exist_ok=True)
        path = os.path.join(d, "banner_wordmark.png")
        render(scale, args.ss).save(path, "PNG")
        print(f"  wrote {os.path.relpath(path, os.path.join(HERE, '..'))}")

    mark_left, _, _ = layout()
    xml = BANNER_XML.format(mark=MARK, left=round(mark_left, 1))
    with open(os.path.join(RES, "drawable", "banner.xml"), "w", encoding="utf-8") as f:
        f.write(xml)
    print("  wrote app/src/main/res/drawable/banner.xml")


if __name__ == "__main__":
    main()
