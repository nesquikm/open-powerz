// FR 5MTE4V: the app icon set is generated from the SVG source and the
// 1024 px master follows the macOS grid (~100 px transparent margin around
// an opaque ~824 px rounded body).
import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { deflateSync } from "node:zlib";
import { describe, expect, it } from "vitest";
import {
  alphaAt,
  buildIco,
  buildIcns,
  decodeIcnsRle,
  decodePng,
  legacyIcnsChunks,
  pngSize,
  readIco,
  readIcns,
  readIcnsChunks,
} from "./icons";

const here = dirname(fileURLToPath(import.meta.url));
const desktopDir = join(here, "..");
const tauriDir = join(desktopDir, "src-tauri");
const iconsDir = join(tauriDir, "icons");
const icon = (name: string): Uint8Array => new Uint8Array(readFileSync(join(iconsDir, name)));

// ---- test-only PNG encoder (8-bit RGBA, non-interlaced, filter 0) ----------

const crcTable = (() => {
  const t = new Uint32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    t[n] = c >>> 0;
  }
  return t;
})();

function crc32(bytes: Uint8Array): number {
  let c = 0xffffffff;
  for (const b of bytes) c = crcTable[(c ^ b) & 0xff]! ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

function chunk(type: string, data: Uint8Array): Uint8Array {
  const out = new Uint8Array(12 + data.length);
  const view = new DataView(out.buffer);
  view.setUint32(0, data.length);
  for (let i = 0; i < 4; i++) out[4 + i] = type.charCodeAt(i);
  out.set(data, 8);
  view.setUint32(8 + data.length, crc32(out.subarray(4, 8 + data.length)));
  return out;
}

/** A width×height RGBA PNG whose alpha is `alpha(x, y)`. */
function makePng(
  width: number,
  height: number,
  alpha: (x: number, y: number) => number = () => 255,
  rgb: (x: number, y: number) => [number, number, number] = () => [0x20, 0x60, 0xc0],
): Uint8Array {
  const ihdr = new Uint8Array(13);
  const v = new DataView(ihdr.buffer);
  v.setUint32(0, width);
  v.setUint32(4, height);
  ihdr[8] = 8; // bit depth
  ihdr[9] = 6; // RGBA
  const raw = new Uint8Array(height * (1 + width * 4));
  for (let y = 0; y < height; y++) {
    const row = y * (1 + width * 4);
    raw[row] = 0;
    for (let x = 0; x < width; x++) {
      const p = row + 1 + x * 4;
      [raw[p]!, raw[p + 1]!, raw[p + 2]!] = rgb(x, y);
      raw[p + 3] = alpha(x, y);
    }
  }
  const sig = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
  const parts = [
    sig,
    chunk("IHDR", ihdr),
    chunk("IDAT", new Uint8Array(deflateSync(raw))),
    chunk("IEND", new Uint8Array(0)),
  ];
  const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
  let o = 0;
  for (const p of parts) {
    out.set(p, o);
    o += p.length;
  }
  return out;
}

// ---- helpers ---------------------------------------------------------------

describe("icon helpers (AC-5MTE4V.4)", () => {
  it("pngSize reads width and height from the PNG header", () => {
    expect(pngSize(makePng(37, 21))).toEqual({ width: 37, height: 21 });
  });

  it("pngSize rejects data that is not a PNG", () => {
    expect(() => pngSize(new Uint8Array([1, 2, 3, 4, 5, 6, 7, 8]))).toThrow();
  });

  it("alphaAt decodes RGBA pixels", () => {
    const png = makePng(8, 8, (x, y) => (x === 3 && y === 5 ? 0 : 255));
    expect(alphaAt(png, 3, 5)).toBe(0);
    expect(alphaAt(png, 0, 0)).toBe(255);
    expect(alphaAt(png, 7, 7)).toBe(255);
  });

  it("buildIcns → readIcns round-trips chunk types and PNG sizes", () => {
    const entries = [
      { type: "ic07", png: makePng(128, 128) },
      { type: "ic11", png: makePng(32, 32) },
    ];
    const icns = buildIcns(entries);
    // 'icns' magic + total length in the file header.
    expect(String.fromCharCode(...icns.subarray(0, 4))).toBe("icns");
    expect(new DataView(icns.buffer, icns.byteOffset).getUint32(4)).toBe(icns.length);
    const back = readIcns(icns);
    expect(back.map((e) => [e.type, e.width, e.height])).toEqual([
      ["ic07", 128, 128],
      ["ic11", 32, 32],
    ]);
  });

  // macOS reads the 16 and 32 px slots as run-length RGB plus an 8-bit mask
  // (is32/s8mk, il32/l8mk); PNG data in icp4/icp5 decodes as noise (FR 5MTE4V).
  it("the RLE planes decode back to the image's own pixels, across run and literal limits", () => {
    // One 32 px plane is 1024 bytes: runs of 2, 3, 129, 130, 131 and 300, then a
    // 150-byte literal stretch (longer than one 128-byte literal packet).
    const plane: number[] = [];
    let v = 0;
    for (const run of [2, 3, 129, 130, 131, 300]) {
      for (let i = 0; i < run; i++) plane.push(v);
      v = (v + 37) % 256;
    }
    for (let i = 0; plane.length < 1024; i++) plane.push((i * 53 + 7) % 256);
    const at = (x: number, y: number) => plane[y * 32 + x] ?? 0;
    const png = makePng(
      32,
      32,
      () => 255,
      (x, y) => [at(x, y), 255 - at(x, y), (at(x, y) * 3) % 256],
    );
    const rgb = legacyIcnsChunks(png)[0];
    if (!rgb) throw new Error("no RGB chunk");
    const decoded = decodeIcnsRle(rgb.data, 1024);
    const { rgba } = decodePng(png);
    for (let c = 0; c < 3; c++) {
      for (let i = 0; i < 1024; i++) {
        if (decoded[c * 1024 + i] !== rgba[i * 4 + c]) {
          throw new Error(`plane ${c} byte ${i}: ${decoded[c * 1024 + i]} != ${rgba[i * 4 + c]}`);
        }
      }
    }
  });

  it.each([
    [16, "is32", "s8mk"],
    [32, "il32", "l8mk"],
  ])("legacyIcnsChunks packs a %i px image as %s + %s", (size, rgbType, maskType) => {
    const png = makePng(size, size, (x, y) => ((x + y) % 3 === 0 ? 0 : 255));
    const chunks = legacyIcnsChunks(png);
    expect(chunks.map((c) => c.type)).toEqual([rgbType, maskType]);
    const [rgb, mask] = chunks as [
      { type: string; data: Uint8Array },
      { type: string; data: Uint8Array },
    ];
    expect(mask.data.length).toBe(size * size);
    for (const [x, y] of [
      [0, 0],
      [1, 0],
      [2, 1],
      [size - 1, size - 1],
    ] as const) {
      expect(mask.data[y * size + x]).toBe(alphaAt(png, x, y));
    }
    // The RLE planes decode back to size² values per channel.
    expect(decodeIcnsRle(rgb.data, size * size)).toHaveLength(3 * size * size);
    // Through buildIcns the raw chunks keep their bytes.
    const icns = buildIcns(chunks);
    const back = readIcnsChunks(icns);
    expect(back.map((c) => [c.type, c.data.length])).toEqual([
      [rgbType, rgb.data.length],
      [maskType, size * size],
    ]);
  });

  it("buildIco → readIco round-trips image sizes", () => {
    const ico = buildIco([16, 32, 256].map((size) => ({ size, png: makePng(size, size) })));
    // ICONDIR: reserved 0, type 1 (icon), count 3.
    const v = new DataView(ico.buffer, ico.byteOffset);
    expect(v.getUint16(0, true)).toBe(0);
    expect(v.getUint16(2, true)).toBe(1);
    expect(v.getUint16(4, true)).toBe(3);
    const back = readIco(ico);
    expect(back.map((e) => [e.size, e.width, e.height])).toEqual([
      [16, 16, 16],
      [32, 32, 32],
      [256, 256, 256],
    ]);
  });
});

// ---- committed icon files --------------------------------------------------

describe("1024 px master (AC-5MTE4V.2)", () => {
  const master = icon("icon.png");

  it("is 1024×1024", () => {
    expect(pngSize(master)).toEqual({ width: 1024, height: 1024 });
  });

  it("is transparent at the corners and in the ~100 px margin", () => {
    const clear: [number, number][] = [
      [0, 0],
      [1023, 0],
      [0, 1023],
      [1023, 1023],
      [50, 512],
      [512, 50],
      [974, 512],
      [512, 974],
    ];
    for (const [x, y] of clear) {
      expect({ x, y, a: alphaAt(master, x, y) }).toEqual({ x, y, a: 0 });
    }
  });

  it("is opaque across the ~824 px body, centred", () => {
    const solid: [number, number][] = [
      [512, 512],
      [130, 512],
      [894, 512],
      [512, 130],
      [512, 894],
    ];
    for (const [x, y] of solid) {
      expect({ x, y, a: alphaAt(master, x, y) }).toEqual({ x, y, a: 255 });
    }
  });
});

describe("platform icon files (AC-5MTE4V.4)", () => {
  const pngs: [string, number][] = [
    ["32x32.png", 32],
    ["64x64.png", 64],
    ["128x128.png", 128],
    ["128x128@2x.png", 256],
    ["Square30x30Logo.png", 30],
    ["Square44x44Logo.png", 44],
    ["Square71x71Logo.png", 71],
    ["Square89x89Logo.png", 89],
    ["Square107x107Logo.png", 107],
    ["Square142x142Logo.png", 142],
    ["Square150x150Logo.png", 150],
    ["Square284x284Logo.png", 284],
    ["Square310x310Logo.png", 310],
    ["StoreLogo.png", 50],
  ];
  it.each(pngs)("%s is %i px square", (name, size) => {
    expect(pngSize(icon(name))).toEqual({ width: size, height: size });
  });

  it("icon.icns holds the PNG chunks from 128 px up and RLE + mask chunks at 16 and 32 px", () => {
    const want: [string, number][] = [
      ["ic07", 128],
      ["ic08", 256],
      ["ic09", 512],
      ["ic10", 1024],
      ["ic11", 32],
      ["ic12", 64],
      ["ic13", 256],
      ["ic14", 512],
    ];
    const file = icon("icon.icns");
    const got = new Map(readIcns(file).map((e) => [e.type, e]));
    for (const [type, size] of want) {
      const e = got.get(type);
      expect({ type, w: e?.width, h: e?.height }).toEqual({ type, w: size, h: size });
    }
    const raw = new Map(readIcnsChunks(file).map((c) => [c.type, c.data]));
    for (const type of ["icp4", "icp5", "icp6"]) expect(raw.has(type), type).toBe(false);
    for (const [rgbType, maskType, size] of [
      ["is32", "s8mk", 16],
      ["il32", "l8mk", 32],
    ] as const) {
      const mask = raw.get(maskType);
      expect(mask?.length, maskType).toBe(size * size);
      expect(decodeIcnsRle(raw.get(rgbType) ?? new Uint8Array(), size * size)).toHaveLength(
        3 * size * size,
      );
      // Transparent corners, an opaque centre.
      expect(mask?.[0]).toBe(0);
      expect(mask?.[(size / 2) * size + size / 2]).toBe(255);
    }
  });

  it("icon.ico holds PNG images at 16, 24, 32, 48, 64 and 256", () => {
    const sizes = readIco(icon("icon.ico"))
      .map((e) => {
        expect(e.width).toBe(e.size);
        expect(e.height).toBe(e.size);
        return e.size;
      })
      .sort((a, b) => a - b);
    expect(sizes).toEqual([16, 24, 32, 48, 64, 256]);
  });

  it("the SVG sources exist", () => {
    expect(existsSync(join(iconsDir, "icon.svg"))).toBe(true);
    expect(existsSync(join(iconsDir, "icon-small.svg"))).toBe(true);
  });

  it("the bundle icon list points at existing files", () => {
    const conf = JSON.parse(readFileSync(join(tauriDir, "tauri.conf.json"), "utf8")) as {
      bundle?: { icon?: string[] };
    };
    const list = conf.bundle?.icon ?? [];
    expect(list.length).toBeGreaterThan(0);
    for (const p of list) {
      expect({ p, exists: existsSync(join(tauriDir, p)) }).toEqual({ p, exists: true });
    }
  });

  it("the icons are generated from the SVG with the Tauri icon tool", () => {
    const pkg = JSON.parse(readFileSync(join(desktopDir, "package.json"), "utf8")) as {
      scripts?: Record<string, string>;
    };
    expect(pkg.scripts?.icons).toMatch(/scripts\/icons\.ts/);
    const src = readFileSync(join(here, "icons.ts"), "utf8");
    expect(src).toMatch(/icon\.svg/);
    expect(src).toMatch(/icon-small\.svg/);
    expect(src).toMatch(/tauri[^\n]*icon/);
  });
});
