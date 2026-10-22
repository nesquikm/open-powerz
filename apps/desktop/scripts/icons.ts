// Regenerates the app icon set from the SVG sources (FR 5MTE4V).
//
//   bun run icons
//
// 1. `tauri icon src-tauri/icons/icon.svg` renders the 1024 px master
//    (icon.svg) into every platform file.
// 2. `tauri icon src-tauri/icons/icon-small.svg -p 16,32` renders the
//    simplified small-size artwork; it replaces 32x32.png and the 16/32 px
//    slots in icon.icns and icon.ico, which are rewritten by the pure
//    writers below; icon.png is replaced by a 1024 px master render.
// 3. Anything tauri writes that is not part of the desktop icon set
//    (android/, ios/) is removed.
import { execFileSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { inflateSync } from "node:zlib";

const PNG_SIG = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];

/** Channels per pixel for each supported 8-bit colour type (grey, RGB, grey+alpha, RGBA). */
const PNG_CHANNELS: Record<number, number> = { 0: 1, 2: 3, 4: 2, 6: 4 };

function isPng(png: Uint8Array): boolean {
  return png.length >= 24 && PNG_SIG.every((b, i) => png[i] === b);
}

function view(bytes: Uint8Array): DataView {
  return new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
}

/** Width and height from a PNG's IHDR chunk. */
export function pngSize(png: Uint8Array): { width: number; height: number } {
  if (!isPng(png)) throw new Error("not a PNG");
  const v = view(png);
  return { width: v.getUint32(16), height: v.getUint32(20) };
}

/** Decodes an 8-bit, non-interlaced PNG (grey, RGB, grey+alpha or RGBA) to RGBA. */
export function decodePng(png: Uint8Array): { width: number; height: number; rgba: Uint8Array } {
  if (!isPng(png)) throw new Error("not a PNG");
  const v = view(png);
  let off = 8;
  let width = 0;
  let height = 0;
  let bitDepth = 0;
  let colorType = 0;
  let interlace = 0;
  const idat: Uint8Array[] = [];
  while (off + 8 <= png.length) {
    const len = v.getUint32(off);
    const type = String.fromCharCode(...png.subarray(off + 4, off + 8));
    const data = png.subarray(off + 8, off + 8 + len);
    if (type === "IHDR") {
      const h = view(data);
      width = h.getUint32(0);
      height = h.getUint32(4);
      bitDepth = data[8] ?? 0;
      colorType = data[9] ?? 0;
      interlace = data[12] ?? 0;
    } else if (type === "IDAT") {
      idat.push(data);
    } else if (type === "IEND") {
      break;
    }
    off += 12 + len;
  }
  if (bitDepth !== 8 || interlace !== 0) throw new Error("unsupported PNG layout");
  const channels = PNG_CHANNELS[colorType];
  if (channels === undefined) throw new Error("unsupported PNG colour type");
  const grey = channels <= 2;
  const raw = new Uint8Array(inflateSync(Buffer.concat(idat)));
  const stride = width * channels;
  const rgba = new Uint8Array(width * height * 4);
  let prev = new Uint8Array(stride);
  let cur = new Uint8Array(stride);
  for (let row = 0; row < height; row++) {
    const base = row * (stride + 1);
    const filter = raw[base] ?? 0;
    for (let i = 0; i < stride; i++) {
      const r = raw[base + 1 + i] ?? 0;
      const a = i >= channels ? (cur[i - channels] ?? 0) : 0;
      const b = prev[i] ?? 0;
      const c = i >= channels ? (prev[i - channels] ?? 0) : 0;
      let val: number;
      switch (filter) {
        case 0:
          val = r;
          break;
        case 1:
          val = r + a;
          break;
        case 2:
          val = r + b;
          break;
        case 3:
          val = r + ((a + b) >> 1);
          break;
        case 4: {
          const p = a + b - c;
          const pa = Math.abs(p - a);
          const pb = Math.abs(p - b);
          const pc = Math.abs(p - c);
          val = r + (pa <= pb && pa <= pc ? a : pb <= pc ? b : c);
          break;
        }
        default:
          throw new Error(`bad PNG filter ${filter}`);
      }
      cur[i] = val & 0xff;
    }
    for (let x = 0; x < width; x++) {
      const px = cur.subarray(x * channels, x * channels + channels);
      const o = (row * width + x) * 4;
      rgba[o] = px[0] ?? 0;
      rgba[o + 1] = (grey ? px[0] : px[1]) ?? 0;
      rgba[o + 2] = (grey ? px[0] : px[2]) ?? 0;
      rgba[o + 3] = channels === 4 ? (px[3] ?? 0) : channels === 2 ? (px[1] ?? 0) : 255;
    }
    [prev, cur] = [cur, prev];
  }
  return { width, height, rgba };
}

/** Alpha of pixel (x, y) in an 8-bit, non-interlaced PNG (255 when it has no alpha). */
export function alphaAt(png: Uint8Array, x: number, y: number): number {
  const { width, height, rgba } = decodePng(png);
  if (x < 0 || y < 0 || x >= width || y >= height) throw new Error("pixel out of range");
  return rgba[(y * width + x) * 4 + 3] ?? 0;
}

/** Apple icns run-length packing of one 8-bit plane. */
function packIcnsRle(plane: Uint8Array): number[] {
  const out: number[] = [];
  let i = 0;
  while (i < plane.length) {
    let run = 1;
    while (i + run < plane.length && run < 130 && plane[i + run] === plane[i]) run++;
    if (run >= 3) {
      out.push(0x80 + run - 3, plane[i] ?? 0);
      i += run;
      continue;
    }
    // Literal span: stop before the next run of three or at 128 bytes.
    let j = i;
    while (
      j < plane.length &&
      j - i < 128 &&
      !(j + 2 < plane.length && plane[j] === plane[j + 1] && plane[j] === plane[j + 2])
    ) {
      j++;
    }
    out.push(j - i - 1, ...plane.subarray(i, j));
    i = j;
  }
  return out;
}

/** Unpacks an is32/il32 chunk into its R, G and B planes (`pixels` bytes each). */
export function decodeIcnsRle(data: Uint8Array, pixels: number): Uint8Array {
  const out = new Uint8Array(3 * pixels);
  let o = 0;
  let i = 0;
  while (o < out.length && i < data.length) {
    const ctl = data[i++] ?? 0;
    if (ctl < 0x80) {
      const n = Math.min(ctl + 1, out.length - o, data.length - i);
      out.set(data.subarray(i, i + n), o);
      i += ctl + 1;
      o += n;
    } else {
      const n = Math.min(ctl - 125, out.length - o);
      out.fill(data[i++] ?? 0, o, o + n);
      o += n;
    }
  }
  if (o !== out.length) throw new Error("truncated icns RLE data");
  return out;
}

const LEGACY_TYPES: Record<number, [string, string]> = {
  16: ["is32", "s8mk"],
  32: ["il32", "l8mk"],
};

/** The legacy RLE RGB + 8-bit mask chunks macOS reads for 16 and 32 px icons. */
export function legacyIcnsChunks(png: Uint8Array): { type: string; data: Uint8Array }[] {
  const { width, height, rgba } = decodePng(png);
  const types = width === height ? LEGACY_TYPES[width] : undefined;
  if (!types) throw new Error(`no legacy icns slot for ${width}x${height}`);
  const pixels = width * height;
  const rgb: number[] = [];
  for (let ch = 0; ch < 3; ch++) {
    const plane = new Uint8Array(pixels);
    for (let p = 0; p < pixels; p++) plane[p] = rgba[p * 4 + ch] ?? 0;
    rgb.push(...packIcnsRle(plane));
  }
  const mask = new Uint8Array(pixels);
  for (let p = 0; p < pixels; p++) mask[p] = rgba[p * 4 + 3] ?? 0;
  return [
    { type: types[0], data: new Uint8Array(rgb) },
    { type: types[1], data: mask },
  ];
}

export type IcnsEntry = { type: string; png: Uint8Array } | { type: string; data: Uint8Array };

/** An Apple .icns file holding each PNG or raw chunk under its 4-char type. */
export function buildIcns(entries: IcnsEntry[]): Uint8Array {
  const bodies = entries.map((e) => ("png" in e ? e.png : e.data));
  const total = 8 + bodies.reduce((n, b) => n + 8 + b.length, 0);
  const out = new Uint8Array(total);
  const v = view(out);
  const tag = (at: number, s: string) => {
    for (let i = 0; i < 4; i++) out[at + i] = s.charCodeAt(i);
  };
  tag(0, "icns");
  v.setUint32(4, total);
  let off = 8;
  entries.forEach((e, i) => {
    const body = bodies[i] ?? new Uint8Array();
    tag(off, e.type);
    v.setUint32(off + 4, 8 + body.length);
    out.set(body, off + 8);
    off += 8 + body.length;
  });
  return out;
}

/** Every chunk of an .icns file, raw. */
export function readIcnsChunks(icns: Uint8Array): { type: string; data: Uint8Array }[] {
  if (icns.length < 8 || String.fromCharCode(...icns.subarray(0, 4)) !== "icns") {
    throw new Error("not an icns file");
  }
  const v = view(icns);
  const total = Math.min(v.getUint32(4), icns.length);
  const out: { type: string; data: Uint8Array }[] = [];
  let off = 8;
  while (off + 8 <= total) {
    const type = String.fromCharCode(...icns.subarray(off, off + 4));
    const len = v.getUint32(off + 4);
    if (len < 8 || off + len > total) break;
    out.push({ type, data: icns.subarray(off + 8, off + len) });
    off += len;
  }
  return out;
}

/** PNG chunks of an .icns file, with their decoded sizes. */
export function readIcns(icns: Uint8Array): { type: string; width: number; height: number }[] {
  return readIcnsChunks(icns)
    .filter((c) => isPng(c.data))
    .map((c) => ({ type: c.type, ...pngSize(c.data) }));
}

export interface IcoEntry {
  size: number;
  png: Uint8Array;
}

/** A Windows .ico file with PNG-compressed images. */
export function buildIco(entries: IcoEntry[]): Uint8Array {
  const head = 6 + 16 * entries.length;
  const total = head + entries.reduce((n, e) => n + e.png.length, 0);
  const out = new Uint8Array(total);
  const v = view(out);
  v.setUint16(0, 0, true);
  v.setUint16(2, 1, true);
  v.setUint16(4, entries.length, true);
  let data = head;
  entries.forEach((e, i) => {
    const d = 6 + 16 * i;
    out[d] = e.size >= 256 ? 0 : e.size;
    out[d + 1] = e.size >= 256 ? 0 : e.size;
    v.setUint16(d + 4, 1, true); // planes
    v.setUint16(d + 6, 32, true); // bits per pixel
    v.setUint32(d + 8, e.png.length, true);
    v.setUint32(d + 12, data, true);
    out.set(e.png, data);
    data += e.png.length;
  });
  return out;
}

/** Images of an .ico file: directory size plus the embedded PNG's size. */
export function readIco(ico: Uint8Array): { size: number; width: number; height: number }[] {
  const v = view(ico);
  if (ico.length < 6 || v.getUint16(0, true) !== 0 || v.getUint16(2, true) !== 1) {
    throw new Error("not an ico file");
  }
  const count = v.getUint16(4, true);
  const out: { size: number; width: number; height: number }[] = [];
  for (let i = 0; i < count; i++) {
    const d = 6 + 16 * i;
    if (d + 16 > ico.length) break;
    const size = ico[d] || 256;
    const len = v.getUint32(d + 8, true);
    const at = v.getUint32(d + 12, true);
    const data = ico.subarray(at, at + len);
    if (isPng(data)) out.push({ size, ...pngSize(data) });
  }
  return out;
}

// ---- generator -------------------------------------------------------------

// 16 and 32 px go in as legacy is32/s8mk and il32/l8mk chunks (macOS decodes
// PNG data in icp4/icp5 as noise); the rest are PNG chunks.
const ICNS_LEGACY_SIZES = [16, 32];
const ICNS_LAYOUT: [string, number][] = [
  ["ic07", 128],
  ["ic08", 256],
  ["ic09", 512],
  ["ic10", 1024],
  ["ic11", 32],
  ["ic12", 64],
  ["ic13", 256],
  ["ic14", 512],
];
const ICO_SIZES = [16, 24, 32, 48, 64, 256];
const SMALL_SIZES = [16, 32];
const MASTER_SIZES = [24, 48, 64, 128, 256, 512, 1024];

function tauriIcon(input: string, out: string, sizes?: number[]): void {
  const args = ["tauri", "icon", input, "-o", out];
  if (sizes) args.push("-p", sizes.join(","));
  // The project's own Tauri CLI (the "tauri" package script), run by this Bun, never a
  // CLI bunx would fetch.
  execFileSync(process.execPath, ["run", ...args], { stdio: "inherit" });
}

function main(): void {
  const desktopDir = join(dirname(fileURLToPath(import.meta.url)), "..");
  process.chdir(desktopDir);
  const iconsDir = join("src-tauri", "icons");
  const before = new Set(readdirSync(iconsDir));

  // Master SVG → every platform file; whatever tauri adds beyond the icon set
  // (android/, ios/) goes, even when the run fails partway.
  try {
    tauriIcon(join(iconsDir, "icon.svg"), iconsDir);
  } finally {
    for (const name of readdirSync(iconsDir)) {
      if (!before.has(name)) rmSync(join(iconsDir, name), { recursive: true, force: true });
    }
  }

  const tmp = mkdtempSync(join(tmpdir(), "powerz-icons-"));
  try {
    const smallDir = join(tmp, "small");
    const masterDir = join(tmp, "master");
    tauriIcon(join(iconsDir, "icon-small.svg"), smallDir, SMALL_SIZES);
    tauriIcon(join(iconsDir, "icon.svg"), masterDir, MASTER_SIZES);
    const render = (size: number): Uint8Array => {
      const dir = SMALL_SIZES.includes(size) ? smallDir : masterDir;
      const file = join(dir, `${size}x${size}.png`);
      if (!existsSync(file)) throw new Error(`tauri icon did not write ${file}`);
      return new Uint8Array(readFileSync(file));
    };

    // tauri writes icon.png at 512 px; the committed master is 1024 px.
    writeFileSync(join(iconsDir, "icon.png"), render(1024));
    writeFileSync(join(iconsDir, "32x32.png"), render(32));
    writeFileSync(
      join(iconsDir, "icon.icns"),
      buildIcns([
        ...ICNS_LEGACY_SIZES.flatMap((size) => legacyIcnsChunks(render(size))),
        ...ICNS_LAYOUT.map(([type, size]) => ({ type, png: render(size) })),
      ]),
    );
    writeFileSync(
      join(iconsDir, "icon.ico"),
      buildIco(ICO_SIZES.map((size) => ({ size, png: render(size) }))),
    );
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
}

// Only when run as a script: the tests import this module.
if (process.argv[1] !== undefined && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main();
}
