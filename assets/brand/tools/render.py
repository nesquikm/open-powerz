#!/usr/bin/env python3
"""Regenerate the open-powerz brand kit in assets/brand.

Usage:
    python3 -I assets/brand/tools/render.py <out-dir>

Needs only Python's standard library, Pillow and rsvg-convert (plus a built `powerz` CLI at
<repo>/target/debug/powerz for the logo preview check; `cargo build -p powerz-cli`).

Rewrites the generated SVGs (via kit_svgs.py) and every PNG kept in assets/brand: meter-logo.png,
meter-logo-wordmark.png, app-icon-1024.png, readme-banner.png, social-preview.png, favicon-16.png and
favicon-32.png. On-meter mocks, powerz preview checks, the app-icon strip, strip-only renders and the
kit sheet go to <out-dir> only, never into the repository.
"""
import argparse
import os
import subprocess
import sys

from PIL import Image, ImageDraw

HERE = os.path.dirname(os.path.abspath(__file__))
sys.dont_write_bytecode = True  # keep __pycache__ out of assets/brand
sys.path.insert(0, HERE)
import kit_svgs  # noqa: E402

BRAND = kit_svgs.BRAND
REPO = os.path.dirname(os.path.dirname(BRAND))
POWERZ = os.path.join(REPO, "target", "debug", "powerz")
NAVY = (0x1A, 0x1F, 0x4A)
SCREEN = 240


def rgb565(c):
    r, g, b = c[:3]
    r5, g6, b5 = r >> 3, g >> 2, b >> 3
    return ((r5 << 3) | (r5 >> 2), (g6 << 2) | (g6 >> 4), (b5 << 3) | (b5 >> 2))


def quantise(img):
    out = img.copy()
    px = out.load()
    for y in range(out.height):
        for x in range(out.width):
            p = px[x, y]
            px[x, y] = rgb565(p) + ((p[3],) if len(p) == 4 else ())
    return out


def meter_png(name, mode, w, h, out_dir):
    """brand/name.svg -> brand/name.png (meter-ready); out_dir/name-onmeter.png (mock), name-preview.png/.txt."""
    base = os.path.join(BRAND, name)
    raw = os.path.join(out_dir, name + ".raw.png")
    subprocess.run(["rsvg-convert", "-w", str(w), "-h", str(h), base + ".svg", "-o", raw], check=True)
    src = Image.open(raw).convert("RGBA")
    os.remove(raw)
    if mode == "alpha":
        # Pre-blend edges onto navy, then make alpha binary: looks anti-aliased on the meter, no partial alpha.
        flat = Image.new("RGBA", src.size, NAVY + (255,))
        flat.alpha_composite(src)
        out = Image.new("RGBA", src.size, (0, 0, 0, 0))
        sp, fp, op = src.load(), flat.load(), out.load()
        for y in range(h):
            for x in range(w):
                if sp[x, y][3] >= 64:
                    op[x, y] = fp[x, y][:3] + (255,)
        alphas = set(out.getchannel("A").tobytes())
        assert alphas <= {0, 255}, alphas
    else:
        assert w % 2 == 0 and w <= 240 and h <= 240
        out = Image.new("RGB", src.size, NAVY)
        out.paste(src, mask=src.split()[3])
    out.save(base + ".png")

    # mock: centred on a 240x240 navy screen, RGB565-quantised
    screen = Image.new("RGBA", (SCREEN, SCREEN), NAVY + (255,))
    screen.alpha_composite(quantise(out.convert("RGBA")), ((SCREEN - w) // 2, (SCREEN - h) // 2))
    mock = quantise(screen.convert("RGB"))
    mock.save(os.path.join(out_dir, name + "-onmeter.png"))

    # powerz logo preview (no meter)
    preview = os.path.join(out_dir, name + "-preview")
    args = [POWERZ, "logo", "preview"] + (["--alpha"] if mode == "alpha" else []) + [base + ".png", "-o", preview + ".png"]
    r = subprocess.run(args, capture_output=True, text=True)
    with open(preview + ".txt", "w") as f:
        f.write("$ powerz " + " ".join(a if not a.startswith("/") else os.path.basename(a) for a in args[1:]) + "\n")
        f.write(r.stdout + r.stderr + f"(exit {r.returncode})\n")
    print(f"== {name}: exit {r.returncode}\n{r.stdout}{r.stderr}")
    if r.returncode != 0:
        sys.exit(f"powerz logo preview rejected {base}.png")
    return mock


def svg_png(svg, png, w, h=None):
    args = ["rsvg-convert", "-w", str(w)] + (["-h", str(h)] if h else []) + [svg, "-o", png]
    subprocess.run(args, check=True)
    return Image.open(png).convert("RGBA")


def kit(out_dir):
    """SVGs from kit_svgs.py, the brand PNGs, then the icon strip and kit-sheet.png in out_dir."""
    kit_svgs.write_all()
    B = lambda name: os.path.join(BRAND, name)
    O = lambda name: os.path.join(out_dir, name)
    mocks = [
        meter_png("meter-logo", "alpha", 200, 200, out_dir),
        meter_png("meter-logo-wordmark", "alpha", 200, 200, out_dir),
    ]
    icons = {1024: svg_png(B("app-icon.svg"), B("app-icon-1024.png"), 1024)}
    for n in (256, 64, 32):
        icons[n] = svg_png(B("app-icon.svg"), O(f"app-icon-{n}.png"), n)
    fav = {n: svg_png(B("favicon.svg"), B(f"favicon-{n}.png"), n) for n in (32, 16)}
    banner = svg_png(B("readme-banner.svg"), B("readme-banner.png"), 1280)
    social = svg_png(B("social-preview.svg"), B("social-preview.png"), 1280)

    # legibility strip: app icon and favicon at 16/32/64 actual size and 4x nearest, on dark and light
    # (the 16 px icon and 64 px favicon are strip-only renders, written to out_dir)
    strip_src = {
        "app-icon": {16: svg_png(B("app-icon.svg"), O("app-icon-16.png"), 16), 32: icons[32], 64: icons[64]},
        "favicon": {16: fav[16], 32: fav[32], 64: svg_png(B("favicon.svg"), O("favicon-64.png"), 64)},
    }
    strip = Image.new("RGB", (2400, 2 * (64 * 4 + 40) + 20), (40, 40, 44))
    d = ImageDraw.Draw(strip)
    y = 10
    for name, sizes in strip_src.items():
        d.text((10, y), name, fill=(230, 230, 230), font_size=16)
        x = 10
        for bg in ((13, 17, 23), (246, 248, 250)):
            for n in (16, 32, 64):
                im = sizes[n]
                big = Image.new("RGBA", (n * 4, n * 4), bg + (255,))
                big.alpha_composite(im.resize((n * 4, n * 4), Image.NEAREST))
                cell = Image.new("RGBA", (n + 6 + n * 4, n * 4), bg + (255,))
                cell.alpha_composite(im, (0, 0))
                cell.alpha_composite(big, (n + 6, 0))
                strip.paste(cell.convert("RGB"), (x, y + 24))
                x += cell.width + 12
        y += 64 * 4 + 40
    strip = strip.crop((0, 0, x + 10, y))
    strip.save(O("app-icon-strip.png"))

    # kit sheet
    pad, bg = 24, (32, 32, 36)
    parts = [
        ("meter-logo / meter-logo-wordmark (on-meter mocks, 2x)", [m.resize((480, 480), Image.NEAREST) for m in mocks]
         + [icons[256], fav[32].resize((128, 128), Image.NEAREST)]),
        ("app-icon / favicon legibility strip", [strip]),
        ("readme-banner 1280x400", [banner]),
        ("social-preview 1280x640", [social]),
    ]
    W = pad + max(sum(i.width + pad for i in ims) for _, ims in parts)
    H = pad + sum(30 + max(i.height for i in ims) + pad for _, ims in parts)
    sheet = Image.new("RGB", (W, H), bg)
    d = ImageDraw.Draw(sheet)
    y = pad
    for label, ims in parts:
        d.text((pad, y), label, fill=(235, 235, 235), font_size=20)
        y += 30
        x = pad
        for im in ims:
            if im.mode == "RGBA":
                sheet.paste(im, (x, y), im)
            else:
                sheet.paste(im, (x, y))
            x += im.width + pad
        y += max(i.height for i in ims) + pad
    sheet.save(O("kit-sheet.png"))


def main():
    p = argparse.ArgumentParser(description="Regenerate the brand kit in assets/brand.")
    p.add_argument("out_dir", help="folder for on-meter mocks, preview checks and the kit sheet (outside the repo)")
    out_dir = os.path.abspath(p.parse_args().out_dir)
    if os.path.commonpath([out_dir, REPO]) == REPO:
        sys.exit(f"out-dir must be outside the repository ({REPO})")
    os.makedirs(out_dir, exist_ok=True)
    kit(out_dir)


if __name__ == "__main__":
    main()
