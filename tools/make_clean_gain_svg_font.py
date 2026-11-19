"""Generates patches/clean-gain/fonts/clean-line.svg: a monoline geometric SVG font (SVG 1.1
font format), the kind of file an LLM can write by hand.

Glyphs are drawn as round-capped strokes on a cap-height box (700 units, y down, easier to
think in) and converted to the SVG-font convention (y up, baseline at 0). Lowercase letters are
small caps (the capitals scaled to 72 %, same stroke width). Run: python3 -I tools/make_clean_gain_svg_font.py
"""
import re
import sys
from pathlib import Path

CAP = 700
STROKE = 92
SIDE = 70  # side bearing

# char: (width, path in a y-down box: x 0..width, y 0 (cap) .. 700 (baseline))
G = {
    "A": (520, "M0 700 L260 0 L520 700 M95 450 L425 450"),
    "B": (450, "M0 350 L270 350 A175 175 0 0 1 270 700 L0 700 L0 0 L240 0 A175 175 0 0 1 240 350"),
    "C": (500, "M490 140 A300 350 0 1 0 490 560"),
    "D": (500, "M0 0 L0 700 L180 700 A320 350 0 0 0 180 0 Z"),
    "E": (430, "M430 0 L0 0 L0 700 L430 700 M0 350 L350 350"),
    "F": (430, "M430 0 L0 0 L0 700 M0 350 L350 350"),
    "G": (520, "M490 130 A300 350 0 1 0 520 520 L520 400 L320 400"),
    "H": (480, "M0 0 L0 700 M480 0 L480 700 M0 350 L480 350"),
    "I": (0, "M0 0 L0 700"),
    "J": (300, "M300 0 L300 520 A170 180 0 0 1 0 560"),
    "K": (460, "M0 0 L0 700 M440 0 L0 420 M150 290 L460 700"),
    "L": (400, "M0 0 L0 700 L400 700"),
    "M": (600, "M0 700 L0 0 L300 450 L600 0 L600 700"),
    "N": (480, "M0 700 L0 0 L480 700 L480 0"),
    "O": (600, "M300 0 A300 350 0 1 0 300 700 A300 350 0 1 0 300 0"),
    "P": (450, "M0 700 L0 0 L270 0 A180 180 0 0 1 270 360 L0 360"),
    "Q": (600, "M300 0 A300 350 0 1 0 300 700 A300 350 0 1 0 300 0 M380 520 L600 730"),
    "R": (460, "M0 700 L0 0 L270 0 A180 180 0 0 1 270 360 L0 360 M250 360 L460 700"),
    "S": (470, "M450 120 C400 20 40 10 40 180 C40 340 460 330 460 520 C460 690 80 710 20 590"),
    "T": (480, "M0 0 L480 0 M240 0 L240 700"),
    "U": (460, "M0 0 L0 470 A230 230 0 0 0 460 470 L460 0"),
    "V": (500, "M0 0 L250 700 L500 0"),
    "W": (720, "M0 0 L170 700 L360 200 L550 700 L720 0"),
    "X": (480, "M0 0 L480 700 M480 0 L0 700"),
    "Y": (500, "M0 0 L250 360 L500 0 M250 360 L250 700"),
    "Z": (460, "M0 0 L460 0 L0 700 L460 700"),
    "0": (460, "M230 0 A230 350 0 1 0 230 700 A230 350 0 1 0 230 0"),
    "1": (200, "M0 130 L170 0 L170 700"),
    "2": (440, "M30 160 C60 20 420 0 420 200 C420 380 0 500 0 700 L440 700"),
    "3": (440, "M20 90 C120 -20 420 0 420 180 C420 330 260 350 180 350 C300 350 440 380 440 530 C440 720 100 730 10 620"),
    "4": (470, "M340 700 L340 0 L0 480 L470 480"),
    "5": (430, "M420 0 L60 0 L30 330 C120 280 430 260 430 500 C430 720 90 730 10 620"),
    "6": (440, "M400 40 C200 -40 0 120 0 450 A220 225 0 1 0 220 250 C110 250 30 320 0 420"),
    "7": (460, "M0 0 L460 0 L170 700"),
    "8": (440, "M220 340 A180 170 0 1 1 220 0 A180 170 0 1 1 220 340 M220 340 A210 180 0 1 0 220 700 A210 180 0 1 0 220 340"),
    "9": (440, "M40 660 C240 740 440 580 440 250 A220 225 0 1 0 220 450 C330 450 410 380 440 280"),
    "-": (240, "M0 420 L240 420"),
    ".": (0, "M0 700 L0 700"),
    ",": (40, "M40 690 L0 790"),
    ":": (0, "M0 250 L0 250 M0 700 L0 700"),
    "'": (0, "M0 0 L0 200"),
    "/": (380, "M380 0 L0 700"),
    "&": (520, "M520 700 L120 230 A150 140 0 1 1 330 220 L60 480 A160 160 0 0 0 300 690 L480 470"),
    "(": (160, "M160 -20 C-50 180 -50 520 160 720"),
    ")": (160, "M0 -20 C210 180 210 520 0 720"),
    "%": (520, "M520 0 L0 700 M100 0 A100 120 0 1 0 100 240 A100 120 0 1 0 100 0 M420 460 A100 120 0 1 0 420 700 A100 120 0 1 0 420 460"),
}

TOKEN = re.compile(r"[MLCAZ]|-?\d+(?:\.\d+)?")


def transform(d, scale, dx, flip=True):
    """Absolute M/L/C/A/Z only. Scales, shifts by dx, flips y (y' = CAP - y) for the font."""
    toks = TOKEN.findall(d)
    out, i, cmd = [], 0, None
    def pt(x, y):
        x, y = float(x) * scale + dx, float(y) * scale
        if flip:
            y = CAP - y
        return f"{x:.0f} {y:.0f}"
    while i < len(toks):
        t = toks[i]
        if t in "MLCAZ":
            cmd = t
            out.append(t)
            i += 1
            if t == "Z":
                continue
        if cmd in "ML":
            out.append(pt(toks[i], toks[i + 1])); i += 2
        elif cmd == "C":
            out.append(" ".join(pt(toks[i + 2 * k], toks[i + 2 * k + 1]) for k in range(3))); i += 6
        elif cmd == "A":
            rx, ry, rot, large, sweep = toks[i:i + 5]
            sweep = str(1 - int(sweep)) if flip else sweep  # mirroring flips the direction
            out.append(f"{float(rx) * scale:.0f} {float(ry) * scale:.0f} {rot} {large} {sweep} {pt(toks[i + 5], toks[i + 6])}"); i += 7
    return " ".join(out)


def glyph(ch, width, d, scale=1.0):
    adv = round(width * scale + 2 * SIDE)
    path = transform(d, scale, SIDE)
    # Small caps sit on the baseline: shift up by what the scaling removed from the cap box.
    if scale != 1.0:
        path = transform(d, scale, SIDE, flip=False)
        path = transform(path_shift(path, CAP * (1 - scale)), 1.0, 0)
    esc = {"&": "&amp;", "'": "&apos;", '"': "&quot;", "<": "&lt;"}.get(ch, ch)
    return (f'  <glyph unicode="{esc}" horiz-adv-x="{adv}"><path d="{path}" fill="none" stroke="currentColor" '
            f'stroke-width="{STROKE}" stroke-linecap="round" stroke-linejoin="round"/></glyph>')


def path_shift(d, dy):
    """Shift all y coordinates of an absolute y-down path by dy (keeps arcs' flags)."""
    toks = TOKEN.findall(d)
    out, i, cmd = [], 0, None
    while i < len(toks):
        t = toks[i]
        if t in "MLCAZ":
            cmd = t; out.append(t); i += 1
            if t == "Z":
                continue
        if cmd in "ML":
            out.append(f"{toks[i]} {float(toks[i + 1]) + dy:.0f}"); i += 2
        elif cmd == "C":
            out.append(" ".join(f"{toks[i + 2 * k]} {float(toks[i + 2 * k + 1]) + dy:.0f}" for k in range(3))); i += 6
        elif cmd == "A":
            out.append(" ".join(toks[i:i + 5]) + f" {toks[i + 5]} {float(toks[i + 6]) + dy:.0f}"); i += 7
    return " ".join(out)


lines = [
    '<svg xmlns="http://www.w3.org/2000/svg">',
    '<!-- Clean Line: monoline geometric display font for the Clean Gain patch (SVG 1.1 font). -->',
    '<defs>',
    '<font id="clean-line" horiz-adv-x="600">',
    '  <font-face font-family="Clean Line" units-per-em="1000" ascent="800" descent="200" cap-height="700" x-height="504"/>',
    '  <missing-glyph horiz-adv-x="500"><path d="M70 0 L70 700 L430 700 L430 0 Z" fill="none" stroke="currentColor" stroke-width="40"/></missing-glyph>',
    '  <glyph unicode=" " horiz-adv-x="300"/>',
]
for ch, (w, d) in G.items():
    lines.append(glyph(ch, w, d))
    if ch.isalpha():
        lines.append(glyph(ch.lower(), w, d, 0.72))
# A few kerning pairs (positive k = tighter).
for a, b, k in [("A", "V", 60), ("V", "A", 60), ("L", "T", 60), ("T", "a", 50), ("A", "Y", 50), ("Y", "a", 50), ("r", "a", 20)]:
    lines.append(f'  <hkern u1="{a}" u2="{b}" k="{k}"/>')
lines += ["</font>", "</defs>", "</svg>", ""]

out = Path(sys.argv[1] if len(sys.argv) > 1 else "patches/clean-gain/fonts/clean-line.svg")
out.parent.mkdir(parents=True, exist_ok=True)
out.write_text("\n".join(lines))
print(f"{out}: {len(G)} glyphs + small caps, {out.stat().st_size} bytes")
