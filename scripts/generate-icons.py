#!/usr/bin/env python3
"""Generate the pimp-my-dsh desktop icon set (DSH monogram).

Deterministic: same inputs produce byte-identical PNG/ICO output. The icon is
an original geometric monogram, not a Tauri default. It renders a rounded
square with a diagonal indigo->violet gradient, a soft top-left sheen, an
inset hairline, the white "DSH" monogram in Segoe UI Bold, and a short amber
terminal-prompt underscore beneath the wordmark.

Output: apps/desktop/src-tauri/icons/{icon.png, icon.ico, 32x32.png,
128x128.png, 128x128@2x.png}.
"""

from __future__ import annotations

import os

from PIL import Image, ImageDraw, ImageFilter, ImageFont

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(ROOT, "apps", "desktop", "src-tauri", "icons")

MASTER = 1024
RADIUS = 232

TOP_LEFT = (79, 70, 229)       # indigo-600
BOTTOM_RIGHT = (15, 23, 42)    # slate-900
INK = (255, 255, 255, 255)
AMBER = (251, 191, 36, 255)

# Transparent "safe zone" inset so the mark reads well at 16px.
INSET = 56


def _lerp(a: tuple[int, int, int], b: tuple[int, int, int], t: float) -> tuple[int, int, int]:
    return tuple(round(a[i] + (b[i] - a[i]) * t) for i in range(3))


def _gradient(size: int) -> Image.Image:
    img = Image.new("RGB", (size, size))
    px = img.load()
    top_left = TOP_LEFT
    bottom_right = BOTTOM_RIGHT
    for y in range(size):
        for x in range(size):
            t = (x + y) / (2.0 * (size - 1))
            px[x, y] = _lerp(top_left, bottom_right, t)
    return img


def _round_mask(size: int, radius: int) -> Image.Image:
    mask = Image.new("L", (size, size), 0)
    d = ImageDraw.Draw(mask)
    d.rounded_rectangle([0, 0, size - 1, size - 1], radius=radius, fill=255)
    return mask


def _font(size: int) -> ImageFont.FreeTypeFont:
    for path in (
        "C:/Windows/Fonts/segoeuib.ttf",   # Segoe UI Bold
        "C:/Windows/Fonts/segoeui.ttf",    # Segoe UI Regular
        "C:/Windows/Fonts/arialbd.ttf",
        "C:/Windows/Fonts/arial.ttf",
    ):
        if os.path.exists(path):
            return ImageFont.truetype(path, size)
    return ImageFont.load_default()


def _draw_wordmark(draw: ImageDraw.ImageDraw, size: int) -> None:
    """Center the DSH monogram and amber prompt underscore."""
    text = "DSH"
    tracking = int(size * 0.014)
    safe_width = size - 2 * INSET

    font_size = int(size * 0.52)
    font = _font(font_size)
    glyphs = [draw.textbbox((0, 0), ch, font=font, stroke_width=0) for ch in text]
    widths = [b[2] - b[0] for b in glyphs]
    total = sum(widths) + tracking * (len(text) - 1)

    # Shrink to fit the safe zone (keeps the monogram fully inside the rounded square).
    if total > safe_width:
        scale = safe_width / total
        font_size = int(font_size * scale)
        font = _font(font_size)
        glyphs = [draw.textbbox((0, 0), ch, font=font, stroke_width=0) for ch in text]
        widths = [b[2] - b[0] for b in glyphs]
        total = sum(widths) + tracking * (len(text) - 1)

    ascent = max(b[3] - b[1] for b in glyphs)
    x = (size - total) / 2
    baseline = size / 2 + ascent * 0.34
    for ch, b, w in zip(text, glyphs, widths):
        draw.text((x - b[0], baseline - b[3]), ch, font=font, fill=INK)
        x += w + tracking

    # Amber prompt underscore, centered under the wordmark.
    bar_w = total * 0.62
    bar_h = max(int(size * 0.016), 6)
    y = baseline + ascent * 0.16 + bar_h
    x0 = (size - bar_w) / 2
    draw.rounded_rectangle(
        [x0, y - bar_h, x0 + bar_w, y],
        radius=bar_h // 2,
        fill=AMBER,
    )


def build_master() -> Image.Image:
    size = MASTER
    base = _gradient(size)
    # Soft top-left sheen for depth.
    sheen = Image.new("L", (size, size), 0)
    sd = ImageDraw.Draw(sheen)
    sd.ellipse(
        [-size * 0.45, -size * 0.55, size * 0.72, size * 0.52],
        fill=110,
    )
    sheen = sheen.filter(ImageFilter.GaussianBlur(size * 0.12))
    lightened = Image.blend(base, Image.new("RGB", (size, size), (255, 255, 255)), 0.18)
    base = Image.composite(lightened, base, sheen)

    canvas = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    canvas.paste(base, (0, 0), _round_mask(size, RADIUS))

    draw = ImageDraw.Draw(canvas)
    # Inset hairline.
    draw.rounded_rectangle(
        [INSET * 0.55, INSET * 0.55, size - 1 - INSET * 0.55, size - 1 - INSET * 0.55],
        radius=RADIUS - int(INSET * 0.55),
        outline=(255, 255, 255, 46),
        width=max(int(size * 0.004), 2),
    )

    _draw_wordmark(draw, size)
    return canvas


def main() -> None:
    os.makedirs(OUT, exist_ok=True)
    master = build_master()

    icon_512 = master.resize((512, 512), Image.LANCZOS)
    icon_512.save(os.path.join(OUT, "icon.png"))

    for name, px in (
        ("32x32.png", 32),
        ("128x128.png", 128),
        ("128x128@2x.png", 256),
    ):
        master.resize((px, px), Image.LANCZOS).save(os.path.join(OUT, name))

    # Multi-resolution Windows ICO (PNG-compressed entries).
    ico = master.convert("RGBA")
    ico.save(
        os.path.join(OUT, "icon.ico"),
        format="ICO",
        sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)],
    )
    print(f"wrote icons to {OUT}")


if __name__ == "__main__":
    main()
