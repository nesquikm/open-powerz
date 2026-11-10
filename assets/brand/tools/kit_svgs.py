#!/usr/bin/env python3
"""Write the generated electron-surf brand SVGs into assets/brand (the folder above tools/).

meter-logo.svg is a hand-kept source in assets/brand and is not written here. Run through render.py:
    python3 -I assets/brand/tools/render.py <out-dir>

All geometry here is original, hand-drawn for open-powerz (MIT). Lettering uses the monoline stroke glyphs below
(no font). Glyph cell: x-height 0..10, baseline y=10, caps from y=-3.5, descender to 15.
"""
import os

HERE = os.path.dirname(os.path.abspath(__file__))
BRAND = os.path.dirname(HERE)  # assets/brand
NAVY = "#1a1f4a"
INK = "#111"

# id, svg body (strokes, fill none), advance
GLYPHS = {
    # lowercase
    "o": ("l-o", '<circle cx="5" cy="5" r="4.5"/>', 13),
    "p": ("l-p", '<path d="M0.5,0.5 V15"/><circle cx="5" cy="5" r="4.5"/>', 13),
    "e": ("l-e", '<path d="M1,5 H9.5 A4.5,4.5 0 1,0 8.2,8.2"/>', 13),
    "n": ("l-n", '<path d="M0.5,10 V0.5 M0.5,4.5 A4,4 0 0,1 8.5,4.5 V10"/>', 12),
    "w": ("l-w", '<path d="M0,0.5 L2.6,10 L5.25,3 L7.9,10 L10.5,0.5"/>', 14),
    "r": ("l-r", '<path d="M0.5,10 V0.5 M0.5,5 A5,5 0 0,1 7,1"/>', 11),
    "z": ("l-z", '<path d="M0.5,0.5 H9 L0.5,10 H9.5"/>', 12),
    "s": ("l-s", '<path d="M8.5,1.6 C7,0 1.6,0 1.6,2.6 C1.6,5.2 8.8,4.4 8.8,7.4 C8.8,10.4 2.5,10.6 0.8,8.4"/>', 12.5),
    "u": ("l-u", '<path d="M0.5,0.5 V5.5 A4,4 0 0,0 8.5,5.5 M8.5,0.5 V10"/>', 12),
    "c": ("l-c", '<path d="M8.6,2.3 A4.5,4.5 0 1,0 8.6,7.7"/>', 12),
    "a": ("l-a", '<circle cx="4.5" cy="5" r="4.5"/><path d="M9,0.5 V10"/>', 12.5),
    "f": ("l-f", '<path d="M7.5,-2.6 C6,-4.4 2.5,-4 2.5,-0.5 V10 M0,1 H6.5"/>', 10),
    "t": ("l-t", '<path d="M2.5,-3 V7.5 Q2.5,10 6.5,9.6 M0,1 H6.5"/>', 10),
    "h": ("l-h", '<path d="M0.5,-3.5 V10 M0.5,4.5 A4,4 0 0,1 8.5,4.5 V10"/>', 12),
    "m": ("l-m", '<path d="M0.5,10 V0.5 M0.5,4 A3.5,3.5 0 0,1 7.5,4 V10 M7.5,4 A3.5,3.5 0 0,1 14.5,4 V10"/>', 18),
    "-": ("s-dash", '<path d="M1,4 H7"/>', 10),
    "/": ("s-slash", '<path d="M8,-3.5 L1,10"/>', 11),
    # capitals
    "O": ("u-O", '<ellipse cx="6" cy="3.25" rx="5.5" ry="6.75"/>', 15),
    "P": ("u-P", '<path d="M0.5,10 V-3.5 H5 A3.5,3.5 0 0,1 5,3.5 H0.5"/>', 12),
    "W": ("u-W", '<path d="M0,-3.5 L3,10 L6.5,0 L10,10 L13,-3.5"/>', 16),
    "E": ("u-E", '<path d="M8.5,-3.5 H0.5 V10 H8.5 M0.5,3.2 H7"/>', 12),
    "R": ("u-R", '<path d="M0.5,10 V-3.5 H5 A3.5,3.5 0 0,1 5,3.5 H0.5 M4.5,3.5 L9,10"/>', 12.5),
    "Z": ("u-Z", '<path d="M0.5,-3.5 H9.5 L0.5,10 H9.5"/>', 13),
    "K": ("u-K", '<path d="M0.5,-3.5 V10 M9,-3.5 L0.5,4.5 M3.6,1.6 L9.5,10"/>', 12.5),
    "M": ("u-M", '<path d="M0.5,10 V-3.5 L6.5,5 L12.5,-3.5 V10"/>', 16),
    "C": ("u-C", '<path d="M10.2,-1 A5.5,6.75 0 1,0 10.2,7.5"/>', 14),
    "U": ("u-U", '<path d="M0.5,-3.5 V5.5 A4.5,4.5 0 0,0 9.5,5.5 V-3.5"/>', 13),
    "S": ("u-S", '<path d="M9,-1.6 C8,-4.2 1,-4.6 1,-0.6 C1,3.4 9.5,2.4 9.5,6.4 C9.5,10.6 2,10.8 0.5,7.6"/>', 13),
    "B": ("u-B", '<path d="M0.5,10 V-3.5 H5 A3.2,3.2 0 0,1 5,2.9 H0.5 M5,2.9 H5.6 A3.55,3.55 0 0,1 5.6,10 H0.5"/>', 12.5),
    # digits
    "0": ("d-0", '<ellipse cx="5" cy="3.25" rx="4.5" ry="6.75"/><path d="M7.4,-1.4 L2.6,7.9"/>', 13),
    "2": ("d-2", '<path d="M1,-1 C2,-4.6 9,-4.6 9,-0.6 C9,3 4,5 0.5,10 H9.5"/>', 13),
    "3": ("d-3", '<path d="M1,-2.2 C3,-4.6 9,-4.2 8.6,-0.6 C8.3,2 6,3 4,3 C7,3 9.5,4 9.5,6.6 C9.5,10.6 2.5,10.6 0.5,8"/>', 13),
}
SPACE = 6


def glyph_defs():
    return "\n".join(
        f'    <symbol id="{gid}" overflow="visible"><g fill="none">{body}</g></symbol>' for gid, body, _ in GLYPHS.values()
    )


def measure(s):
    w = sum(SPACE if ch == " " else GLYPHS[ch][2] for ch in s)
    return w - 3


def text(s, x, y, scale, fill, outline=INK, ow=5.4, fw=3.0, colors=None):
    """Stroke lettering: an outline layer, then a fill layer (one colour, or one per non-space character)."""
    uses, cx = [], 0.0
    for ch in s:
        if ch == " ":
            cx += SPACE
            continue
        gid, _, adv = GLYPHS[ch]
        uses.append((gid, cx))
        cx += adv
    under = "".join(f'<use href="#{g}" x="{px:g}"/>' for g, px in uses)
    if colors:
        over = "".join(f'<use href="#{g}" x="{px:g}" stroke="{colors[i % len(colors)]}"/>' for i, (g, px) in enumerate(uses))
    else:
        over = "".join(f'<use href="#{g}" x="{px:g}"/>' for g, px in uses)
    return (
        f'<g transform="translate({x:g},{y:g}) scale({scale:g})" fill="none" stroke-linecap="round" stroke-linejoin="round">'
        f'<g stroke="{outline}" stroke-width="{ow:g}">{under}</g>'
        f'<g stroke="{fill}" stroke-width="{fw:g}">{over}</g></g>'
    )


# ---- the mascot (coordinates of meter-logo.svg, board sits on the crest at x=100) ----
SURFER = """
  <g stroke="#111" stroke-linejoin="round" stroke-linecap="round">
    <ellipse cx="100" cy="102" rx="40" ry="8" transform="rotate(-8,100,102)" fill="#ff5ca8" stroke-width="5"/>
    <path d="M66,106 L134,96" stroke="#fff" stroke-width="2.5"/>
    <path d="M92,80 L88,101 M108,80 L113,98" stroke-width="6"/>
    <path d="M80,62 L58,52 M120,62 L144,56" stroke-width="6"/>
    <circle cx="56" cy="51" r="6" fill="#ffd23f" stroke-width="4"/>
    <circle cx="146" cy="55" r="6" fill="#ffd23f" stroke-width="4"/>
    <circle cx="100" cy="62" r="25" fill="#ffd23f" stroke-width="6"/>
    <path d="M80,55 H120" stroke-width="4"/>
    <rect x="81" y="53" width="16" height="11" rx="5" fill="#111" stroke-width="2"/>
    <rect x="103" y="53" width="16" height="11" rx="5" fill="#111" stroke-width="2"/>
    <path d="M85,56 L89,56 M107,56 L111,56" stroke="#7fd4ff" stroke-width="2"/>
    <path d="M88,72 Q100,84 112,72" fill="none" stroke-width="4"/>
    <g transform="translate(86,14) scale(1.6)" fill="none">
      <use href="#l-e" stroke="#111" stroke-width="5"/><use href="#l-e" stroke="#3ff0ff" stroke-width="3"/>
      <path d="M13,1 H19" stroke="#111" stroke-width="5"/><path d="M13,1 H19" stroke="#3ff0ff" stroke-width="3"/>
    </g>
  </g>"""

SPARK = '<path id="spark" d="M0,-8 L2,-2 L8,0 L2,2 L0,8 L-2,2 L-8,0 L-2,-2 Z"/>'


def wave_line(xmin, xmax, base, L=75, k=35):
    """Sine-like wave in scene units; one crest sits between x=62.5 and 137.5 (peak at x=100)."""
    x = -12.5 - 2 * L * max(0, int((-12.5 - xmin) // (2 * L)) + 1)
    i = 0 if ((x + 12.5) / L) % 2 == 0 else 1
    d = f"M{x:g},{base:g}"
    while x < xmax:
        dy = k if i % 2 == 0 else -k
        d += f" C{x + L / 3:g},{base + dy:g} {x + 2 * L / 3:g},{base + dy:g} {x + L:g},{base:g}"
        x += L
        i += 1
    return d, x


def scene(xmin, xmax, bottom=205, uid="s", rounded_bottom=False):
    """Wave + foam + surfer, in the 200-unit scene space. Returns (defs, body)."""
    top, xend = wave_line(xmin, xmax, 130)
    x0 = float(top.split(",")[0][1:])
    if rounded_bottom:
        r = 14
        sea = f"{top} L{xend:g},{bottom - r:g} Q{xend:g},{bottom:g} {xend - r:g},{bottom:g} L{x0 + r:g},{bottom:g} Q{x0:g},{bottom:g} {x0:g},{bottom - r:g} Z"
    else:
        sea = f"{top} L{xend:g},{bottom:g} L{x0:g},{bottom:g} Z"
    l1, _ = wave_line(xmin, xmax, 150)
    l2, _ = wave_line(xmin, xmax, 174)
    defs = f'<clipPath id="sea-{uid}"><path d="{sea}"/></clipPath>'
    body = f"""
  <g stroke="#111" stroke-linejoin="round" stroke-linecap="round">
    <path d="{sea}" fill="#2f6bff" stroke-width="6"/>
    <g clip-path="url(#sea-{uid})" fill="none">
      <path d="{l1}" stroke="#7fd4ff" stroke-width="6"/>
      <path d="{l2}" stroke="#1d47c9" stroke-width="10"/>
    </g>
    <path d="{sea}" fill="none" stroke-width="6"/>
    <path d="M70,121 C88,99 112,99 130,121" fill="none" stroke="#fff" stroke-width="5"/>
    <circle cx="146" cy="110" r="4" fill="#fff" stroke-width="2.5"/>
    <circle cx="156" cy="100" r="3" fill="#fff" stroke-width="2.5"/>
    <circle cx="150" cy="92" r="2.2" fill="#fff" stroke-width="2"/>
  </g>{SURFER}"""
    return defs, body


def spark(x, y, s, color, sw=2.5):
    return f'<use href="#spark" transform="translate({x:g},{y:g}) scale({s:g})" fill="{color}" stroke="#111" stroke-width="{sw:g}" stroke-linejoin="round"/>'


def svg(w, h, comment, defs, body):
    return f"""<?xml version="1.0" encoding="UTF-8"?>
<!-- {comment} Original art for open-powerz, MIT. Lettering: hand-drawn stroke glyphs, no font. -->
<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}">
  <defs>
{glyph_defs()}
    {SPARK}
    {defs}
  </defs>
{body}
</svg>
"""


def meter_logo_wordmark():
    d, b = scene(-12.5, 227.5, bottom=192, uid="m", rounded_bottom=True)
    ww = measure("open-powerz") * 1.25
    body = (
        spark(22, 26, 1, "#fff") + spark(174, 18, 0.7, "#ffd23f", 3) + spark(182, 84, 0.6, "#3ff0ff", 3)
        + f'<g transform="translate(20,0) scale(0.8)">{b}</g>'
        + text("open-powerz", (200 - ww) / 2, 172, 1.25, "#3ff0ff", ow=5.6, fw=3.1)
    )
    return svg(200, 200, "Meter startup logo with wordmark (alpha 200x200).", d, body)


def banner():
    W, H, s, tx, ty = 1280, 400, 2.0, 800, 52
    d, b = scene((0 - tx) / s, (W - tx) / s, uid="b")
    sub = "for KM003C / KM002C"
    body = (
        f'<clipPath id="card"><rect width="{W}" height="{H}" rx="32"/></clipPath>'
        f'<g clip-path="url(#card)"><rect width="{W}" height="{H}" fill="{NAVY}"/>'
        + spark(1210, 70, 1.6, "#fff") + spark(640, 52, 1.1, "#ffd23f", 3) + spark(1220, 200, 1.2, "#3ff0ff", 3)
        + spark(560, 236, 0.9, "#ff5ca8", 3)
        + f'<g transform="translate({tx},{ty}) scale({s})">{b}</g>'
        + text("open-powerz", 72, 74, 5.6, "#ffd23f", ow=5.2, fw=3.0)
        + text(sub, 78, 192, 2.4, "#ffffff", ow=4.6, fw=2.4)
        + "</g>"
    )
    return svg(W, H, f"README banner {W}x{H}; own navy card with rounded corners, works on light and dark.", d, body)


def social():
    W, H, s, tx, ty = 1280, 640, 2.6, 780, 202
    d, b = scene((0 - tx) / s, (W - tx) / s, bottom=205, uid="c")
    l1, l2, l3 = "Open-source app for the", "POWER-Z KM003C/KM002C", "USB-C meter"
    body = (
        f'<rect width="{W}" height="{H}" fill="{NAVY}"/>'
        + spark(1190, 80, 1.8, "#fff") + spark(700, 70, 1.2, "#ffd23f", 3) + spark(1200, 300, 1.3, "#3ff0ff", 3)
        + spark(120, 520, 1.1, "#ff5ca8", 3)
        + f'<g transform="translate({tx},{ty}) scale({s})">{b}</g>'
        + text("open-powerz", 80, 120, 6.0, "#ffd23f", ow=5.2, fw=3.0)
        + text(l1, 86, 250, 2.5, "#ffffff", ow=4.4, fw=2.4)
        + text(l2, 86, 312, 2.5, "#ffffff", ow=4.4, fw=2.4)
        + text(l3, 86, 374, 2.5, "#ffffff", ow=4.4, fw=2.4)
    )
    return svg(W, H, f"GitHub social preview {W}x{H}.", d, body)


def icon():
    # 64-unit design; the tile fills the 824/1024 safe area of a desktop app icon
    return f"""<?xml version="1.0" encoding="UTF-8"?>
<!-- open-powerz app icon: the electron-surf mascot's head. Original art, MIT. -->
<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 64 64">
  <defs>
    <clipPath id="tile"><rect x="6.25" y="6.25" width="51.5" height="51.5" rx="11.5"/></clipPath>
  </defs>
  <rect x="6.25" y="6.25" width="51.5" height="51.5" rx="11.5" fill="{NAVY}"/>
  <g clip-path="url(#tile)" stroke="#111" stroke-linejoin="round" stroke-linecap="round">
    <path d="M-2,50 C3,45 8,45 13,50 C18,55 23,55 28,50 C33,45 38,45 43,50 C48,55 53,55 58,50 C63,45 68,45 73,50 V70 H-2 Z" fill="#2f6bff" stroke-width="2.2"/>
    <path d="M-2,55.5 C3,50.5 8,50.5 13,55.5 C18,60.5 23,60.5 28,55.5 C33,50.5 38,50.5 43,55.5 C48,60.5 53,60.5 58,55.5 C63,50.5 68,50.5 73,55.5" fill="none" stroke="#7fd4ff" stroke-width="2"/>
  </g>
  <g stroke="#111" stroke-linejoin="round" stroke-linecap="round">
    <circle cx="32" cy="31" r="16" fill="#ffd23f" stroke-width="3"/>
    <path d="M16.5,27 H47.5" stroke-width="2.6"/>
    <rect x="18" y="24.5" width="12" height="9" rx="3.8" fill="#111" stroke-width="1"/>
    <rect x="34" y="24.5" width="12" height="9" rx="3.8" fill="#111" stroke-width="1"/>
    <path d="M21,27.2 H24 M37,27.2 H40" stroke="#7fd4ff" stroke-width="1.6"/>
    <path d="M25.5,38.5 Q32,44 38.5,38.5" fill="none" stroke-width="2.6"/>
  </g>
</svg>
"""


def favicon():
    # Simplified icon: full-bleed tile, bigger head, no wave or highlights; built to read at 16 px
    return f"""<?xml version="1.0" encoding="UTF-8"?>
<!-- open-powerz favicon: the mascot's head, simplified. Original art, MIT. -->
<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32" viewBox="0 0 32 32">
  <clipPath id="t"><rect width="32" height="32" rx="7"/></clipPath>
  <rect width="32" height="32" rx="7" fill="{NAVY}"/>
  <path clip-path="url(#t)" d="M-2,26 C1,23 4,23 7,26 C10,29 13,29 16,26 C19,23 22,23 25,26 C28,29 31,29 34,26 V34 H-2 Z" fill="#2f6bff" stroke="#111" stroke-width="1.6"/>
  <g stroke="#111" stroke-linejoin="round" stroke-linecap="round">
    <circle cx="16" cy="14.5" r="11" fill="#ffd23f" stroke-width="2"/>
    <path d="M5.5,12 H26.5" stroke-width="2"/>
    <rect x="7.5" y="10" width="7.5" height="6" rx="2.4" fill="#111" stroke-width="0.8"/>
    <rect x="17" y="10" width="7.5" height="6" rx="2.4" fill="#111" stroke-width="0.8"/>
    <path d="M11,19.5 Q16,23.5 21,19.5" fill="none" stroke-width="2"/>
  </g>
</svg>
"""


def write_all():
    out = {
        "meter-logo-wordmark.svg": meter_logo_wordmark(),
        "app-icon.svg": icon(),
        "favicon.svg": favicon(),
        "readme-banner.svg": banner(),
        "social-preview.svg": social(),
    }
    for name, data in out.items():
        with open(os.path.join(BRAND, name), "w") as f:
            f.write(data)
