/**
 * FR AXHK2V on the mocked backend: axis presets, each series' look, the chart title,
 * Save image, the settings kept for the app, the firmware card clear of the chart and
 * settings, the ruler's edge tag and segment-mark labels clear of the tick labels.
 *
 * UI pinned here, beyond the FR's Technical Design:
 *
 * - the live chart's "Chart options" group gains selects "VBUS range" and "IBUS range"
 *   (option texts the presets' labels), a textbox "Chart title", and a group "Series
 *   style" holding, per series, inputs "<label> colour" (a colour input), "<label> width"
 *   and "<label> transparency" (percent) and a button "Reset <label> style"; a colour
 *   with low contrast shows a sentence naming the contrast;
 * - a non-empty title shows above the plot in the chart region as `.chart-title`;
 * - the legend's swatch line (the "Series" group) takes each series' colour
 *   (`stroke`), width (`stroke-width`) and transparency (`stroke-opacity`);
 * - a "Save image" button in the chart region opens a dialog "Save image" with
 *   spinbuttons "Width" and "Height", a select "Format" (`png`, `svg`, `pdf`), a
 *   checkbox "White background", "Save" and "Cancel" buttons; a size outside 320 to 7680 px
 *   shows a sentence naming both limits and disables Save;
 * - the mock records each saved image on `window.__OPEN_POWERZ_MOCK__.chartImages()`:
 *   `{ suggestedName, format, data }[]`, `data` the bytes handed to `save_chart_image`
 *   (for a PDF, the PNG the backend wraps);
 * - the test hook's `chartExtras()` gains `markLabels` (the segment-mark labels drawn:
 *   `{ label, left, top, right, bottom }`) and `tickLabels` (every axis tick label drawn:
 *   `{ left, top, right, bottom }`), both in CSS px in one frame.
 */

import { expect, test, type Locator, type Page } from "@playwright/test";
import { THEME_COLORS } from "../../src/lib/theme";
import { HOOK } from "../hook";
import { chartSeriesLengths, expectHook, lastDraw, open, streamDemo, useTheme } from "./helpers";

test.use({ viewport: { width: 1600, height: 900 } });

const MOCK_HOOK = "__OPEN_POWERZ_MOCK__";
const DEMO_PD = "demo:pd-9v-2a";
/** The demo serials (a second demo meter counts up), whole and as a tile's suffix. */
const DEMO_SERIALS = [/(?<![\d.])12345[67](?!\d)/, /…345[67]/];

interface Box {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

interface StyleProbe {
  segmentMarks: { percent: number; tMs: number; label: string }[];
  markLabels?: (Box & { label: string })[];
  tickLabels?: Box[];
}

interface SavedImage {
  suggestedName: string;
  format: string;
  data: number[];
}

function chart(page: Page): Locator {
  return page.getByRole("region", { name: "Chart", exact: true });
}

function button(scope: Page | Locator, name: string): Locator {
  return scope.getByRole("button", { name, exact: true });
}

/** The "Chart options" group of the live chart, opened. */
async function options(page: Page): Promise<Locator> {
  const toggle = button(chart(page), "Chart options");
  if ((await toggle.getAttribute("aria-expanded")) !== "true") await toggle.click();
  await expect(toggle).toHaveAttribute("aria-expanded", "true");
  const group = chart(page).getByRole("group", { name: "Chart options", exact: true });
  await expect(group).toBeVisible();
  return group;
}

/** Sets an input's value as a user would, for any input type (colour, range, …). */
async function setValue(input: Locator, value: string): Promise<void> {
  await input.evaluate((el, v) => {
    const i = el as HTMLInputElement;
    i.value = v;
    i.dispatchEvent(new Event("input", { bubbles: true }));
    i.dispatchEvent(new Event("change", { bubbles: true }));
  }, value);
}

const plain = (text: string): string => text.replace(/–/g, "-").trim();

async function optionTexts(select: Locator): Promise<string[]> {
  return (await select.locator("option").allInnerTexts()).map(plain);
}

/** Picks the option whose text reads `label` (an en dash read as a hyphen). */
async function pick(select: Locator, label: string): Promise<void> {
  const opts = select.locator("option");
  const texts = (await opts.allInnerTexts()).map(plain);
  const k = texts.indexOf(label);
  expect(k, `option ${label} in ${texts.join(", ")}`).toBeGreaterThanOrEqual(0);
  await select.selectOption({ index: k });
}

/** The legend swatch line of the series named `label`. */
function swatch(page: Page, label: string): Locator {
  return chart(page)
    .getByRole("group", { name: "Series", exact: true })
    .getByRole("button", { name: new RegExp(`^${label}\\b`) })
    .locator("line");
}

async function extras(page: Page): Promise<StyleProbe> {
  await expectHook(page);
  return page.evaluate((name) => {
    const hook = (window as unknown as Record<string, Record<string, unknown>>)[name];
    const fn = hook?.chartExtras;
    if (typeof fn !== "function") throw new Error("the test hook has no chartExtras()");
    return (fn as () => StyleProbe).call(hook);
  }, HOOK);
}

async function savedImages(page: Page): Promise<SavedImage[]> {
  return page.evaluate((key) => {
    const mock = (window as unknown as Record<string, Record<string, unknown> | undefined>)[key];
    const fn = mock?.chartImages;
    if (typeof fn !== "function") throw new Error("the mock has no chartImages()");
    return (fn as () => SavedImage[]).call(mock);
  }, MOCK_HOOK);
}

function saveDialog(page: Page): Locator {
  return page.getByRole("dialog", { name: "Save image" });
}

async function openSaveDialog(page: Page): Promise<Locator> {
  await button(chart(page), "Save image").click();
  const dialog = saveDialog(page);
  await expect(dialog).toBeVisible();
  return dialog;
}

/** Saves the chart through the dialog; returns what the backend received. */
async function saveImage(
  page: Page,
  {
    width,
    height,
    format,
    white = false,
  }: {
    width: number;
    height: number;
    format: "png" | "svg" | "pdf";
    white?: boolean;
  },
): Promise<SavedImage> {
  const before = (await savedImages(page)).length;
  const dialog = await openSaveDialog(page);
  await dialog.getByRole("spinbutton", { name: "Width" }).fill(String(width));
  await dialog.getByRole("spinbutton", { name: "Height" }).fill(String(height));
  await dialog.getByLabel("Format", { exact: true }).selectOption(format);
  await dialog.getByRole("checkbox", { name: "White background" }).setChecked(white);
  await button(dialog, "Save").click();
  await expect.poll(async () => (await savedImages(page)).length).toBe(before + 1);
  await expect(dialog).toBeHidden();
  const saved = (await savedImages(page)).at(-1);
  if (saved === undefined) throw new Error("no image saved");
  return saved;
}

/** A PNG's width and height from its IHDR. */
function pngSize(data: readonly number[]): { width: number; height: number } {
  const sig = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
  expect(data.slice(0, 8), "a PNG signature").toEqual(sig);
  const u32 = (at: number): number =>
    (((data[at] ?? 0) << 24) |
      ((data[at + 1] ?? 0) << 16) |
      ((data[at + 2] ?? 0) << 8) |
      (data[at + 3] ?? 0)) >>>
    0;
  return { width: u32(16), height: u32(20) };
}

function utf8(data: readonly number[]): string {
  return Buffer.from(data).toString("utf8");
}

interface SvgFacts {
  error: boolean;
  root: string;
  width: number;
  height: number;
  paths: number;
  scripts: number;
  texts: string[];
  backgrounds: string[];
}

/** The SVG as the browser's own XML parser reads it. */
async function readSvg(page: Page, text: string): Promise<SvgFacts> {
  return page.evaluate((src) => {
    const doc = new DOMParser().parseFromString(src, "image/svg+xml");
    const root = doc.documentElement;
    const width = Number.parseFloat(root.getAttribute("width") ?? "");
    return {
      error: doc.getElementsByTagName("parsererror").length > 0,
      root: root.localName,
      width,
      height: Number.parseFloat(root.getAttribute("height") ?? ""),
      paths: root.getElementsByTagName("path").length,
      scripts: root.getElementsByTagName("script").length,
      texts: [...root.getElementsByTagName("text")].map((t) => (t.textContent ?? "").trim()),
      backgrounds: [...root.getElementsByTagName("rect")]
        .filter((r) => {
          const w = r.getAttribute("width") ?? "";
          return w === "100%" || Number.parseFloat(w) === width;
        })
        .map((r) => r.getAttribute("fill") ?? ""),
    };
  }, text);
}

/** `#rrggbb` of a hex, `rgb()` or `rgba()` colour, lower case. */
function hex(color: string): string {
  const h = /^#([0-9a-f]{6})/i.exec(color.trim());
  if (h) return `#${(h[1] ?? "").toLowerCase()}`;
  const fn = /^rgba?\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)/i.exec(color.trim());
  if (!fn) return color;
  return `#${[fn[1], fn[2], fn[3]].map((n) => Number(n).toString(16).padStart(2, "0")).join("")}`;
}

function overlaps(a: Box, b: Box): boolean {
  return a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom;
}

function hasSerial(text: string): boolean {
  return DEMO_SERIALS.some((re) => re.test(text));
}

async function streaming(page: Page): Promise<void> {
  await open(page);
  await streamDemo(page);
  await expect.poll(async () => (await chartSeriesLengths(page)).vbus).toBeGreaterThan(50);
}

test("AC-AXHK2V.1: the VBUS and IBUS ranges offer their presets", async ({ page }) => {
  await streaming(page);
  const group = await options(page);
  const vbus = group.getByLabel("VBUS range", { exact: true });
  const ibus = group.getByLabel("IBUS range", { exact: true });
  expect(await optionTexts(vbus)).toEqual(["Auto", "Auto from zero", "0-6 V", "0-12 V", "0-24 V"]);
  expect(await optionTexts(ibus)).toEqual(["Auto", "Auto from zero", "0-2 A", "0-5 A", "0-8 A"]);
  expect(plain(await vbus.locator("option:checked").innerText())).toBe("Auto");
  expect(plain(await ibus.locator("option:checked").innerText())).toBe("Auto");
});

test("AC-AXHK2V.2: each series' colour, width and transparency show in the legend, with a Reset", async ({
  page,
}) => {
  await useTheme(page, "light");
  await streaming(page);
  const group = await options(page);
  const style = group.getByRole("group", { name: "Series style", exact: true });
  await expect(style).toBeVisible();

  const line = swatch(page, "VBUS");
  const before = {
    stroke: await line.getAttribute("stroke"),
    width: await line.getAttribute("stroke-width"),
    opacity: (await line.getAttribute("stroke-opacity")) ?? "1",
  };
  expect(hex(before.stroke ?? "")).toBe(THEME_COLORS.light.chart.voltage);

  await setValue(style.getByLabel("VBUS colour", { exact: true }), "#ff0000");
  await setValue(style.getByLabel("VBUS width", { exact: true }), "3");
  await setValue(style.getByLabel("VBUS transparency", { exact: true }), "50");
  await expect.poll(async () => hex((await line.getAttribute("stroke")) ?? "")).toBe("#ff0000");
  await expect(line).toHaveAttribute("stroke-width", "3");
  await expect(line).toHaveAttribute("stroke-opacity", "0.5");

  // Dash patterns stay apart: IBUS in VBUS's colour is still dashed, VBUS still solid.
  await setValue(style.getByLabel("IBUS colour", { exact: true }), "#ff0000");
  await expect
    .poll(async () => hex((await swatch(page, "IBUS").getAttribute("stroke")) ?? ""))
    .toBe("#ff0000");
  expect(await swatch(page, "IBUS").getAttribute("stroke-dasharray")).toBeTruthy();
  expect(await line.getAttribute("stroke-dasharray")).toBeFalsy();

  await button(style, "Reset VBUS style").click();
  await expect
    .poll(async () => hex((await line.getAttribute("stroke")) ?? ""))
    .toBe(THEME_COLORS.light.chart.voltage);
  expect(await line.getAttribute("stroke-width")).toBe(before.width);
  expect((await line.getAttribute("stroke-opacity")) ?? "1").toBe(before.opacity);
  // Reset touched VBUS alone.
  expect(hex((await swatch(page, "IBUS").getAttribute("stroke")) ?? "")).toBe("#ff0000");
});

test("AC-AXHK2V.3: a low-contrast colour warns but is allowed", async ({ page }) => {
  await useTheme(page, "light");
  await streaming(page);
  const group = await options(page);
  const style = group.getByRole("group", { name: "Series style", exact: true });
  await expect(group.getByText(/contrast/i)).toHaveCount(0);
  await setValue(style.getByLabel("VBUS colour", { exact: true }), "#ffffff");
  await expect(group.getByText(/contrast/i).first()).toBeVisible();
  await expect
    .poll(async () => hex((await swatch(page, "VBUS").getAttribute("stroke")) ?? ""))
    .toBe("#ffffff");
});

test("AC-AXHK2V.4: an editable title shows above the chart as text, never markup", async ({
  page,
}) => {
  await streaming(page);
  const c = chart(page);
  await expect(c.locator(".chart-title")).toHaveCount(0);
  const group = await options(page);
  const title = '<b>Bold</b> & <img src=x onerror="window.__xss=1">';
  await group.getByRole("textbox", { name: "Chart title" }).fill(title);
  const shown = c.locator(".chart-title");
  await expect(shown).toHaveText(title);
  await expect(shown.locator("b, img")).toHaveCount(0);
  expect(await page.evaluate(() => "__xss" in window)).toBe(false);
  const titleBox = await shown.boundingBox();
  const plotBox = await c.locator(".uplot").first().boundingBox();
  expect(titleBox).not.toBeNull();
  expect(plotBox).not.toBeNull();
  expect((titleBox?.y ?? 0) + (titleBox?.height ?? 0)).toBeLessThanOrEqual((plotBox?.y ?? 0) + 1);

  // And in the export, as text.
  const saved = await saveImage(page, { width: 800, height: 600, format: "svg" });
  const svg = await readSvg(page, utf8(saved.data));
  expect(svg.error).toBe(false);
  expect(svg.texts).toContain(title);
});

test("AC-AXHK2V.5: Save image asks for the size and format and refuses sizes outside 320 to 7680 px", async ({
  page,
}) => {
  await streaming(page);
  const dialog = await openSaveDialog(page);
  const width = dialog.getByRole("spinbutton", { name: "Width" });
  const height = dialog.getByRole("spinbutton", { name: "Height" });
  const format = dialog.getByLabel("Format", { exact: true });
  expect(
    await format
      .locator("option")
      .evaluateAll((os) => os.map((o) => (o as HTMLOptionElement).value)),
  ).toEqual(["png", "svg", "pdf"]);
  const save = button(dialog, "Save");
  for (const [w, h] of [
    ["319", "600"],
    ["800", "319"],
    ["7681", "600"],
    ["800", "7681"],
  ] as const) {
    await width.fill(w);
    await height.fill(h);
    await expect(save, `${w} × ${h} is refused`).toBeDisabled();
    await expect(dialog.getByText(/320[^]*7680/).first()).toBeVisible();
  }
  await width.fill("800");
  await height.fill("600");
  await expect(save).toBeEnabled();
  await button(dialog, "Cancel").click();
  await expect(dialog).toBeHidden();
  expect(await savedImages(page)).toEqual([]);

  const png = await saveImage(page, { width: 800, height: 600, format: "png" });
  expect(png.format).toBe("png");
  expect(pngSize(png.data)).toEqual({ width: 800, height: 600 });
  expect(png.suggestedName).toMatch(/\.png$/);

  const svg = await saveImage(page, { width: 1024, height: 512, format: "svg" });
  expect(svg.format).toBe("svg");
  expect(svg.suggestedName).toMatch(/\.svg$/);
  const facts = await readSvg(page, utf8(svg.data));
  expect([facts.width, facts.height]).toEqual([1024, 512]);

  const pdf = await saveImage(page, { width: 320, height: 7680, format: "pdf" });
  expect(pdf.format).toBe("pdf");
  expect(pdf.suggestedName).toMatch(/\.pdf$/);
  // The backend wraps this PNG in the PDF's one page.
  expect(pngSize(pdf.data)).toEqual({ width: 320, height: 7680 });
});

test("AC-AXHK2V.6: the export shows the current view with title, axes and legend, in the theme or on white", async ({
  page,
}) => {
  await useTheme(page, "dark");
  await streaming(page);
  const c = chart(page);
  await button(c, "Live").click();
  await expect(button(c, "Paused · Go live")).toBeVisible();
  const plot = c.locator('[aria-label^="Chart. Click"]');
  await plot.focus();
  for (let k = 0; k < 3; k++) await page.keyboard.press("ArrowUp");
  const view = await lastDraw(page);
  const group = await options(page);
  await group.getByRole("textbox", { name: "Chart title" }).fill("Bench run");

  const themed = await readSvg(
    page,
    utf8((await saveImage(page, { width: 1200, height: 600, format: "svg" })).data),
  );
  expect(themed.error).toBe(false);
  expect(themed.texts).toContain("Bench run");
  expect(themed.texts).toContain("Volts (V)");
  expect(themed.texts).toContain("Amps (A)");
  expect(themed.texts.some((t) => t.startsWith("VBUS"))).toBe(true);
  expect(themed.texts.some((t) => t.startsWith("IBUS"))).toBe(true);
  expect(themed.backgrounds.map(hex)).toContain(THEME_COLORS.dark.bg);

  // Only the view's span: every time tick lies inside the drawn range.
  const seconds = themed.texts
    .map((t) => /^(\d+(?:\.\d+)?) s$/.exec(t))
    .filter((m): m is RegExpExecArray => m !== null)
    .map((m) => Number(m[1]) * 1000);
  expect(seconds.length).toBeGreaterThanOrEqual(2);
  for (const ms of seconds) {
    expect(ms).toBeGreaterThanOrEqual(view.t0Ms - 50);
    expect(ms).toBeLessThanOrEqual(view.t1Ms + 50);
  }

  const white = await readSvg(
    page,
    utf8((await saveImage(page, { width: 1200, height: 600, format: "svg", white: true })).data),
  );
  expect(white.backgrounds.map(hex)).toContain("#ffffff");
  expect(white.backgrounds.map(hex)).not.toContain(THEME_COLORS.dark.bg);
  expect(white.texts).toContain("Bench run");
});

test("AC-AXHK2V.7: the saved SVG parses as XML with the series as paths", async ({ page }) => {
  await streaming(page);
  const saved = await saveImage(page, { width: 800, height: 600, format: "svg" });
  const svg = await readSvg(page, utf8(saved.data));
  expect(svg.error).toBe(false);
  expect(svg.root).toBe("svg");
  // Six series are shown by default (power is off).
  expect(svg.paths).toBeGreaterThanOrEqual(6);
  expect(svg.scripts).toBe(0);
});

interface PngProbe {
  corner: number[];
  titleInk: number;
  legendInk: number;
  /** Per colour asked for: the least per-channel distance of a legend-row pixel to it. */
  legendDistance: number[];
}

/** Decodes a saved PNG with the browser's own decoder and probes its pixels: the top-left
 * corner, the dark pixels in the title band and in the legend row, and how close the
 * legend row comes to each of `colors`. */
async function probePng(
  page: Page,
  data: readonly number[],
  bands: { title: number; legend: number },
  colors: string[],
): Promise<PngProbe> {
  return page.evaluate(
    async ({ bytes, bands, colors }) => {
      const bitmap = await createImageBitmap(
        new Blob([new Uint8Array(bytes)], { type: "image/png" }),
      );
      const canvas = document.createElement("canvas");
      canvas.width = bitmap.width;
      canvas.height = bitmap.height;
      const ctx = canvas.getContext("2d");
      if (ctx === null) throw new Error("no canvas");
      ctx.drawImage(bitmap, 0, 0);
      const { width, height } = canvas;
      const px = ctx.getImageData(0, 0, width, height).data;
      const at = (x: number, y: number): number[] => {
        const k = (y * width + x) * 4;
        return [px[k] ?? 0, px[k + 1] ?? 0, px[k + 2] ?? 0, px[k + 3] ?? 0];
      };
      const ink = (y0: number, y1: number): number => {
        let n = 0;
        for (let y = y0; y < y1; y++) {
          for (let x = 0; x < width; x++) {
            const [r = 0, g = 0, b = 0] = at(x, y);
            if (r + g + b < 3 * 160) n += 1;
          }
        }
        return n;
      };
      const rgb = colors.map((c) => {
        const n = Number.parseInt(c.slice(1), 16);
        return [(n >> 16) & 0xff, (n >> 8) & 0xff, n & 0xff];
      });
      const legendDistance = rgb.map(() => 255);
      for (let y = height - bands.legend; y < height; y++) {
        for (let x = 0; x < width; x++) {
          const p = at(x, y);
          rgb.forEach((c, i) => {
            const d = Math.max(...c.map((v, j) => Math.abs(v - (p[j] ?? 0))));
            legendDistance[i] = Math.min(legendDistance[i] ?? 255, d);
          });
        }
      }
      return {
        corner: at(1, 1),
        titleInk: ink(0, bands.title),
        legendInk: ink(height - bands.legend, height),
        legendDistance,
      };
    },
    { bytes: [...data], bands, colors },
  );
}

test("AC-AXHK2V.6: the saved PNG holds the title band and a legend row in each series' colour", async ({
  page,
}) => {
  await useTheme(page, "light");
  await streaming(page);
  const group = await options(page);
  await group.getByRole("textbox", { name: "Chart title" }).fill("Bench run");
  // The six series shown by default (power is off), in the light theme's colours.
  const colors = ["voltage", "current", "dPlus", "dMinus", "cc1", "cc2"].map(
    (token) => THEME_COLORS.light.chart[token] ?? "",
  );
  // The title band and legend row of a saved image, px.
  const bands = { title: 28, legend: 24 };

  for (const format of ["png", "pdf"] as const) {
    const saved = await saveImage(page, { width: 800, height: 600, format, white: true });
    expect(pngSize(saved.data)).toEqual({ width: 800, height: 600 });
    const probe = await probePng(page, saved.data, bands, colors);
    expect(probe.corner, `${format}: a white corner`).toEqual([255, 255, 255, 255]);
    expect(probe.titleInk, `${format}: the title is drawn`).toBeGreaterThan(20);
    expect(probe.legendInk, `${format}: the legend labels are drawn`).toBeGreaterThan(50);
    probe.legendDistance.forEach((d, i) => {
      expect(d, `${format}: the legend holds ${colors[i]}`).toBeLessThanOrEqual(40);
    });
  }
});

test("AC-AXHK2V.8: the settings persist across a restart, per app and not per meter", async ({
  page,
}) => {
  await streaming(page);
  let group = await options(page);
  await pick(group.getByLabel("VBUS range", { exact: true }), "0-24 V");
  await pick(group.getByLabel("IBUS range", { exact: true }), "0-5 A");
  await group.getByRole("textbox", { name: "Chart title" }).fill("Bench run");
  const style = group.getByRole("group", { name: "Series style", exact: true });
  await setValue(style.getByLabel("VBUS colour", { exact: true }), "#ff0000");
  await expect
    .poll(async () => hex((await swatch(page, "VBUS").getAttribute("stroke")) ?? ""))
    .toBe("#ff0000");
  expect(await page.evaluate(() => localStorage.getItem("open-powerz.chart.style"))).not.toBeNull();

  // A restart, then another meter.
  await open(page);
  await streamDemo(page, DEMO_PD);
  group = await options(page);
  expect(
    plain(
      await group.getByLabel("VBUS range", { exact: true }).locator("option:checked").innerText(),
    ),
  ).toBe("0-24 V");
  expect(
    plain(
      await group.getByLabel("IBUS range", { exact: true }).locator("option:checked").innerText(),
    ),
  ).toBe("0-5 A");
  await expect(group.getByRole("textbox", { name: "Chart title" })).toHaveValue("Bench run");
  await expect(chart(page).locator(".chart-title")).toHaveText("Bench run");
  expect(hex((await swatch(page, "VBUS").getAttribute("stroke")) ?? "")).toBe("#ff0000");
});

test.describe("on a 1280 × 800 window", () => {
  test.use({ viewport: { width: 1280, height: 800 } });

  test("AC-AXHK2V.9: the firmware warning card covers no chart, settings control or settings row", async ({
    page,
  }) => {
    await open(page);
    await streamDemo(page);
    const card = page.locator(".firmware-notice");
    await expect(card).toBeVisible();
    await expect(card).toContainText("Firmware warning");
    const settings = page.getByRole("region", { name: "Meter settings" });
    await expect(settings.locator("dt").first()).toBeVisible();
    await options(page);

    const targets: [string, Locator][] = [["the chart", chart(page)]];
    for (const b of await chart(page).getByRole("button").all())
      targets.push(["a chart control", b]);
    for (const b of await settings.getByRole("button").all())
      targets.push(["a settings control", b]);
    for (const r of await settings.locator("dl > div, dt, dd").all()) {
      targets.push(["a settings row", r]);
    }
    expect(targets.length).toBeGreaterThan(4);
    for (const [what, target] of targets) {
      if (!(await target.isVisible())) continue;
      await target.scrollIntoViewIfNeeded();
      const a = await card.boundingBox();
      const b = await target.boundingBox();
      if (a === null || b === null) continue;
      const cardBox = { left: a.x, top: a.y, right: a.x + a.width, bottom: a.y + a.height };
      const box = { left: b.x, top: b.y, right: b.x + b.width, bottom: b.y + b.height };
      const label = `${what}: ${(await target.innerText()).slice(0, 40)}`;
      expect(overlaps(cardBox, box), `the card covers ${label}`).toBe(false);
    }
  });
});

async function openAbout(page: Page): Promise<Locator> {
  await page.locator("#top-bar-about").click();
  const dialog = page.getByRole("dialog");
  await expect(dialog).toBeVisible();
  return dialog;
}

test("AC-AXHK2V.10: saved images, their file names and the default title carry no serial, hidden or not", async ({
  page,
}) => {
  await streaming(page);
  for (const hidden of [false, true]) {
    if (hidden) {
      await (await openAbout(page)).getByRole("switch", { name: "Hide serial numbers" }).click();
      await page.keyboard.press("Escape");
    }
    const group = await options(page);
    const title = await group.getByRole("textbox", { name: "Chart title" }).inputValue();
    expect(hasSerial(title), `default title ${title}`).toBe(false);
    for (const format of ["png", "svg", "pdf"] as const) {
      const saved = await saveImage(page, { width: 800, height: 600, format });
      expect(saved.suggestedName).toMatch(new RegExp(`^powerz-chart-\\d{8}-\\d{6}\\.${format}$`));
      expect(hasSerial(saved.suggestedName), saved.suggestedName).toBe(false);
      if (format === "svg") expect(hasSerial(utf8(saved.data))).toBe(false);
    }
  }
});

test("AC-AXHK2V.11: an edge cursor's tag separates its name from its time", async ({ page }) => {
  await streaming(page);
  const c = chart(page);
  await button(c, "10 s").click();
  const group = await options(page);
  const ruler = button(group, "Ruler");
  if ((await ruler.getAttribute("aria-pressed")) !== "true") await ruler.click();
  const t1 = c.getByRole("slider", { name: "Ruler cursor T1", exact: true });
  // The live view scrolls on: within a few seconds T1 is behind it.
  await expect(t1).toHaveAttribute("aria-valuetext", /before the view/, { timeout: 15_000 });
  const tag = t1.locator(".tag");
  await expect
    .poll(async () => (await tag.innerText()).replace(/\s+/g, " ").trim())
    .toMatch(/^◀ T1 · (\d+\.\d{3} s|\d+:\d{2}\.\d{3})$/);
});

for (const width of [1280, 375]) {
  test(`AC-AXHK2V.12: segment-mark labels never overlap a tick label at ${width} px`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 800 });
    await streaming(page);
    const c = chart(page);
    await button(c, "Whole session").click();
    const group = await options(page);
    await group.getByLabel("Segment marks", { exact: true }).selectOption("charge");
    await group.getByLabel("Mark step", { exact: true }).selectOption("5");
    // The dashed lines stay: all 19 marks are drawn, whatever their labels do.
    await expect.poll(async () => (await extras(page)).segmentMarks.length).toBe(19);
    const probe = await extras(page);
    expect(Array.isArray(probe.markLabels), "the hook reports the mark labels drawn").toBe(true);
    expect(Array.isArray(probe.tickLabels), "the hook reports the tick labels drawn").toBe(true);
    const marks = probe.markLabels ?? [];
    const ticks = probe.tickLabels ?? [];
    expect(ticks.length).toBeGreaterThan(0);
    const all = probe.segmentMarks.map((m) => m.label);
    for (const m of marks) expect(all).toContain(m.label);
    for (const m of marks) {
      for (const t of ticks) expect(overlaps(m, t), `${m.label} over a tick label`).toBe(false);
      for (const n of marks) {
        if (n !== m) expect(overlaps(m, n), `${m.label} over ${n.label}`).toBe(false);
      }
    }
    if (width === 375) {
      // Nineteen labels cannot fit across a phone-wide plot: they thin out or go.
      expect(marks.length).toBeLessThan(19);
    } else {
      expect(marks.length).toBeGreaterThan(0);
    }
  });
}
