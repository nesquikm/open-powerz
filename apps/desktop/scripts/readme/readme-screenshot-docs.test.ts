// FR SPYPSD (AC-SPYPSD.1): the README's desktop screenshot is a real KM003C
// recording capture, not the old demo-meter image, and its alt text says what the
// recording shows: an Apple 140 W charger stepped over USB PD through 5, 9, 15 and 20 V.
import { createHash } from "node:crypto";
import { existsSync, readFileSync, statSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, "../../../..");

const SCREENSHOT = "docs/images/desktop.png";

/** sha256 of the demo-meter screenshot the README showed before the docs review
 * (1280 x 800, commit 59fc1af). The new capture must not be it. */
const OLD_DEMO_SHA256 = "d2b7a9922f8f1a338503588fd6439a6548ce7d45b2554e1225390cdcdd4c44bb";

const PNG_SIGNATURE = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);

/** The README image whose target is the desktop screenshot: `![alt](target)`. */
function desktopImage(readme: string): { alt: string; target: string } | undefined {
  for (const m of readme.matchAll(/!\[([^\]]*)\]\(([^)\s]+)(?:\s+"[^"]*")?\)/g)) {
    const target = m[2] ?? "";
    if (target.replace(/^\.\//, "") === SCREENSHOT) return { alt: m[1] ?? "", target };
  }
  return undefined;
}

/** Width and height from the PNG's IHDR chunk. */
function pngSize(png: Buffer): { width: number; height: number } {
  expect(png.subarray(12, 16).toString("ascii")).toBe("IHDR");
  return { width: png.readUInt32BE(16), height: png.readUInt32BE(20) };
}

describe("README desktop screenshot (AC-SPYPSD.1)", () => {
  const readme = readFileSync(join(repoRoot, "README.md"), "utf8");
  const image = desktopImage(readme);
  const path = join(repoRoot, SCREENSHOT);

  it("the README shows docs/images/desktop.png", () => {
    expect(image, `README has no ![…](${SCREENSHOT})`).toBeDefined();
  });

  it("the file exists", () => {
    expect(existsSync(path)).toBe(true);
  });

  it("is a PNG of a real HiDPI window capture, small enough for the README", () => {
    const png = readFileSync(path);
    expect(png.subarray(0, 8).equals(PNG_SIGNATURE)).toBe(true);
    const { width, height } = pngSize(png);
    expect(width).toBeGreaterThanOrEqual(1200);
    expect(height).toBeGreaterThanOrEqual(600);
    expect(statSync(path).size).toBeLessThanOrEqual(400 * 1024);
  });

  it("is not the old demo-meter screenshot", () => {
    const sha = createHash("sha256").update(readFileSync(path)).digest("hex");
    expect(sha).not.toBe(OLD_DEMO_SHA256);
  });

  it("the alt text says it is a real KM003C recording, not the demo meter", () => {
    const alt = image?.alt ?? "";
    expect(alt).toMatch(/real KM003C recording/i);
    expect(alt).not.toMatch(/demo/i);
  });

  it("the alt text names the Apple 140 W charger", () => {
    expect(image?.alt ?? "").toMatch(/Apple 140\s?W charger/);
  });

  it("the alt text names the 5, 9, 15 and 20 V steps", () => {
    const alt = image?.alt ?? "";
    expect(alt).toMatch(/\b5, 9, 15 and 20 V\b/);
    expect(alt).toMatch(/USB PD/);
  });
});
