"""Generates patches/junkyard-amp/fonts/label-tape.{fnt,png}: an image font in the BMFont text
format (AngelCode), the common bitmap-font format of game engines. Each glyph is a piece of
embossed label-maker tape with a white letter, slightly jittered, so words read like a strip of
punched labels. Label makers only have capitals: lowercase maps to the same images.

Run: python3 -I tools/make_junkyard_bmfont.py
"""
import random
import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter, ImageFont

ROOT = Path(__file__).resolve().parent.parent
OUT = Path(sys.argv[1]) if len(sys.argv) > 1 else ROOT / "patches/junkyard-amp/fonts"
FONT = ImageFont.truetype(str(ROOT / "crates/plugin/fonts/inter-700.ttf"), 46)

H = 72            # cell height = line height (px)
BASE = 56         # baseline inside the cell
TAPE = (152, 28, 22, 255)      # red label tape
TAPE_DARK = (98, 15, 12, 255)
INK = (246, 238, 228, 255)
CHARS = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 -.:/&'!?#+"
random.seed(7)


def cell(ch):
    """One glyph: a tape piece as wide as the letter needs, embossed letter on top."""
    adv = 26 if ch == " " else int(FONT.getlength(ch) * 0.86) + 14  # condensed like embossing wheels
    img = Image.new("RGBA", (adv + 4, H), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    # Tape: full height band, slightly wavy edges, subtle vertical sheen.
    top, bottom = 8 + random.randint(-1, 1), H - 6 + random.randint(-1, 1)
    d.rectangle([0, top, adv + 3, bottom], fill=TAPE)
    for x in range(adv + 4):
        if random.random() < 0.12:
            d.line([x, top, x, bottom], fill=(170, 40, 32, 255))
    d.line([0, top, adv + 3, top], fill=(196, 72, 60, 255))
    d.line([0, bottom, adv + 3, bottom], fill=TAPE_DARK)
    if ch != " ":
        # Letter rendered wide, squeezed (condensed), raised (light edge), slightly rotated.
        tmp = Image.new("RGBA", (120, H), (0, 0, 0, 0))
        td = ImageDraw.Draw(tmp)
        td.text((10, BASE), ch, font=FONT, fill=INK, anchor="ls")
        shadow = Image.new("RGBA", tmp.size, (0, 0, 0, 0))
        ImageDraw.Draw(shadow).text((11, BASE + 2), ch, font=FONT, fill=(60, 8, 6, 200), anchor="ls")
        tmp = Image.alpha_composite(shadow.filter(ImageFilter.GaussianBlur(0.8)), tmp)
        bbox = tmp.getbbox()
        glyph = tmp.crop((bbox[0], 0, bbox[2], H))
        glyph = glyph.resize((max(1, int(glyph.width * 0.86)), H), Image.LANCZOS)
        glyph = glyph.rotate(random.uniform(-3, 3), resample=Image.BICUBIC, expand=False)
        img.alpha_composite(glyph, ((adv + 4 - glyph.width) // 2, random.randint(-2, 2)))
    return img, adv


cells = [(ch, *cell(ch)) for ch in CHARS]
# Pack into rows of ≤ 512 px.
W, x, y, placed = 512, 1, 1, []
for ch, img, adv in cells:
    if x + img.width + 1 > W:
        x, y = 1, y + H + 2
    placed.append((ch, img, adv, x, y))
    x += img.width + 2
atlas = Image.new("RGBA", (W, y + H + 2), (0, 0, 0, 0))
for ch, img, adv, x, y in placed:
    atlas.alpha_composite(img, (x, y))

OUT.mkdir(parents=True, exist_ok=True)
atlas.save(OUT / "label-tape.png")
rows = []
for ch, img, adv, x, y in placed:
    for c in {ch, ch.lower()}:
        rows.append(f"char id={ord(c)} x={x} y={y} width={img.width} height={H} xoffset=0 yoffset=0 xadvance={adv} page=0 chnl=15")
fnt = [
    'info face="Label Tape" size=72 bold=1 italic=0 charset="" unicode=1 stretchH=100 smooth=1 aa=1 padding=0,0,0,0 spacing=2,2',
    f"common lineHeight={H} base={BASE} scaleW={W} scaleH={atlas.height} pages=1 packed=0",
    'page id=0 file="label-tape.png"',
    f"chars count={len(rows)}",
    *rows,
    "kernings count=0",
    "",
]
(OUT / "label-tape.fnt").write_text("\n".join(fnt))
print(f"{OUT}/label-tape.fnt: {len(rows)} chars, atlas {W}x{atlas.height}")
