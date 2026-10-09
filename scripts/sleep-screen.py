#!/usr/bin/env python3
"""Composes Wordhord's sleep screen: Knud Baade's *Fantasibilde fra den
norske sagatid* (1850), the warden on the sea-cliff, cropped to the
screen, with WORDHORD over the sky. Dithered to the sixteen greys of
e-ink and written as an 8-bit grey PNG.

    scripts/sleep-screen.py art/baade-sagatid.jpg <out.png> [WIDTH HEIGHT [PPI]]
"""

import sys

from PIL import Image, ImageDraw, ImageFont

src, out = sys.argv[1], sys.argv[2]
W, H = (int(sys.argv[3]), int(sys.argv[4])) if len(sys.argv) > 4 else (1404, 1872)
PPI = int(sys.argv[5]) if len(sys.argv) > 5 else 227
pt = lambda p: round(p * PPI / 72)
FONTS = "fonts/junicode/"

painting = Image.open(src).convert("L")
pw, ph = painting.size
# The screen's shape, cut from the full height: the warden a third of the
# way across, the moon to his right.
cw = round(ph * W / H)
warden = 0.37 * pw
x0 = max(0, min(pw - cw, round(warden - 0.34 * cw)))
screen = painting.crop((x0, 0, x0 + cw, ph)).resize((W, H), Image.LANCZOS)

draw = ImageDraw.Draw(screen)


def tracked(text, font, y, track, fill=255):
    """Text centred at baseline `y`, its letters spaced by `track` ems."""
    space = track * font.size
    widths = [font.getlength(c) for c in text]
    total = sum(widths) + space * (len(text) - 1)
    x = (W - total) / 2
    for c, w in zip(text, widths):
        draw.text((x, y), c, font=font, fill=fill, anchor="ls")
        x += w + space


title = ImageFont.truetype(FONTS + "Junicode-Regular.otf", pt(30))
italic = ImageFont.truetype(FONTS + "Junicode-Italic.otf", pt(14))
small = ImageFont.truetype(FONTS + "Junicode-Italic.otf", pt(8))
top = round(H * 0.15)
tracked("WORDHORD", title, top, 0.2)
tracked("wordhord onleac", italic, top + pt(26), 0.02)
tracked(
    "Knud Baade, Fantasibilde fra den norske sagatid, 1850",
    small,
    H - pt(14),
    0.02,
    fill=210,
)

# Sixteen greys, as the screen has, dithered so the sky keeps its depth.
palette = Image.new("P", (1, 1))
greys = [round(k * 255 / 15) for k in range(16)]
palette.putpalette([g for v in greys for g in (v, v, v)] + [0] * (768 - 48))
screen = screen.convert("RGB").quantize(palette=palette, dither=Image.FLOYDSTEINBERG)
screen.convert("L").save(out, optimize=True)
print(f"{out}: {W}×{H}, cropped from x {x0} to {x0 + cw} of {pw}")
