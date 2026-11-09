/**
 * FR EAKFEC (AC-EAKFEC.7): no control in the web edition stays live and then refuses
 * with "Desktop app only" on click.
 *
 * - Every backend function that answers the web build with `desktopOnly()` belongs to a
 *   feature `WEB_FEATURES` switches off, and the components that call it render the
 *   control through `DesktopOnly` (disabled, labelled) for that feature.
 * - The controls the sweep found live and refusing now work: Open capture and Import
 *   vendor log (see `pdFiles.web.test.ts`), the cable seen in a capture, and the chart's
 *   PDF export, which downloads the desktop app's one-page PDF.
 */

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { beforeAll, describe, expect, it } from "vitest";
import type { UiError } from "../backend";
import type { PowerzWasm } from "./driver";
import { DESKTOP_ONLY_TEXT, WEB_FEATURES, type Features } from "./features";
import { loadWasm } from "./testWasm.test";
import { createWebBackend } from "./webBackend";
import { downloadSink, type Download } from "./webBackendFakes.test";

type Options = Parameters<typeof createWebBackend>[0];

const SRC = new URL("../../", import.meta.url);
const source = (rel: string): string => readFileSync(fileURLToPath(new URL(rel, SRC)), "utf8");

/** Each backend function the web build refuses, its feature and the component that
 * shows its control. */
const REFUSED: Record<string, { feature: keyof Features; component: string }> = {
  chooseRecordingFolder: {
    feature: "recordingFolder",
    component: "lib/components/RecorderOptions.svelte",
  },
  firmwareChoose: { feature: "firmwareUpdate", component: "lib/components/FirmwareView.svelte" },
  firmwareInspect: { feature: "firmwareUpdate", component: "lib/components/FirmwareView.svelte" },
  revealPath: { feature: "fileReveal", component: "App.svelte" },
};

/** The `export async function` names in backend.ts whose web branch refuses. */
function refusedInBackend(): string[] {
  const text = source("lib/backend.ts");
  const names: string[] = [];
  for (const m of text.matchAll(/export async function (\w+)[\s\S]*?\{\n([\s\S]*?)\n\}/g)) {
    if (/if \(web\) return desktopOnly\(\);/.test(m[2] ?? "")) names.push(m[1] ?? "");
  }
  return names.sort();
}

let w: PowerzWasm;
beforeAll(async () => {
  w = await loadWasm();
});

describe("refused web commands are labelled controls (AC-EAKFEC.7)", () => {
  it("only the listed functions refuse in the web build", () => {
    expect(refusedInBackend()).toEqual(Object.keys(REFUSED).sort());
  });

  it("each refused function belongs to a feature the web build switches off", () => {
    for (const [name, { feature }] of Object.entries(REFUSED)) {
      expect(WEB_FEATURES[feature], name).toBe(false);
    }
  });

  it("its component shows the control through DesktopOnly or hides it for that feature", () => {
    for (const [name, { feature, component }] of Object.entries(REFUSED)) {
      const text = source(component);
      const labelled = new RegExp(`<DesktopOnly available=\\{features\\.${feature}\\}`).test(text);
      const hidden = new RegExp(`\\{#if features\\.${feature}\\}`).test(text);
      expect(labelled || hidden, `${name} in ${component}`).toBe(true);
    }
  });

  it("the open and import buttons are not refused any more", () => {
    const text = source("lib/backend.ts");
    for (const name of ["pdOpen", "pdImport", "pdReleaseOpened"]) {
      const body = new RegExp(`export async function ${name}\\([^)]*\\)[^{]*\\{\\n([^}]*)\\}`).exec(
        text,
      )?.[1];
      expect(body, name).toMatch(/if \(web\) return web\.then/);
    }
  });
});

// ---- the chart's PDF export ----

const CRC_TABLE = Array.from({ length: 256 }, (_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});
function crc32(bytes: Uint8Array): number {
  let c = 0xffffffff;
  for (const b of bytes) c = (CRC_TABLE[(c ^ b) & 0xff] ?? 0) ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}
function adler32(bytes: Uint8Array): number {
  let a = 1;
  let b = 0;
  for (const x of bytes) {
    a = (a + x) % 65521;
    b = (b + a) % 65521;
  }
  return ((b << 16) | a) >>> 0;
}
const be32 = (n: number): number[] => [
  (n >>> 24) & 255,
  (n >>> 16) & 255,
  (n >>> 8) & 255,
  n & 255,
];

/** zlib with stored blocks only. */
function zlibStored(raw: Uint8Array): Uint8Array {
  const out: number[] = [0x78, 0x01];
  for (let at = 0; at < raw.length || at === 0; at += 65535) {
    const block = raw.subarray(at, at + 65535);
    const last = at + 65535 >= raw.length ? 1 : 0;
    const len = block.length;
    out.push(last, len & 255, len >>> 8, ~len & 255, (~len >>> 8) & 255, ...block);
    if (last) break;
  }
  out.push(...be32(adler32(raw)));
  return new Uint8Array(out);
}

/** An 8-bit RGB PNG of `width × height`, all one grey. */
function png(width: number, height: number): Uint8Array {
  const raw = new Uint8Array(height * (width * 3 + 1)).fill(0x80);
  for (let y = 0; y < height; y++) raw[y * (width * 3 + 1)] = 0;
  const chunk = (kind: string, data: Uint8Array): number[] => {
    const body = new Uint8Array([...new TextEncoder().encode(kind), ...data]);
    return [...be32(data.length), ...body, ...be32(crc32(body))];
  };
  const ihdr = new Uint8Array([...be32(width), ...be32(height), 8, 2, 0, 0, 0]);
  return new Uint8Array([
    0x89,
    0x50,
    0x4e,
    0x47,
    0x0d,
    0x0a,
    0x1a,
    0x0a,
    ...chunk("IHDR", ihdr),
    ...chunk("IDAT", zlibStored(raw)),
    ...chunk("IEND", new Uint8Array()),
  ]);
}

async function webBackend(download: (name: string, blob: Blob) => void) {
  return createWebBackend({
    wasm: w,
    usb: null,
    hid: null,
    serial: null,
    pickDirectory: null,
    crossOriginIsolated: false,
    pageEvents: null,
    download,
  } as unknown as Options);
}

describe("the chart's PDF export in the browser (AC-EAKFEC.7)", () => {
  it("downloads the desktop app's one-page PDF of the chart PNG", async () => {
    const sink = downloadSink();
    const b = await webBackend(sink.download);
    const image = png(320, 320);
    expect(await b.saveChartImage("chart.pdf", "pdf", image)).toBe("chart.pdf");
    const file = sink.downloads[0] as Download;
    expect(file.name).toBe("chart.pdf");
    expect(file.blob.type).toBe("application/pdf");
    const pdf = new Uint8Array(await file.blob.arrayBuffer());
    expect(pdf).toEqual(w.chartPdfFromPng(image));
    const text = new TextDecoder("latin1").decode(pdf);
    expect(text.startsWith("%PDF-1.4")).toBe(true);
    expect(text).toContain("/MediaBox [0 0 320 320]");
  });

  it("refuses a picture out of range in the desktop's words, as no desktop-only refusal", async () => {
    const sink = downloadSink();
    const b = await webBackend(sink.download);
    for (const format of ["pdf", "png"] as const) {
      const e = (await b.saveChartImage("c", format, png(100, 100)).then(
        () => null,
        (err: unknown) => err as UiError,
      )) as UiError;
      expect(e.kind).toBe("usage");
      expect(e.message).toMatch(/^The image was not saved: the image is 100 × 100 px/);
      expect(e.message).not.toContain(DESKTOP_ONLY_TEXT);
    }
    expect(sink.downloads).toEqual([]);
  });
});
