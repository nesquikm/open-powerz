/**
 * FR SX1VTC: dashed series crawl while the chart scrolls (AC .5, .6).
 *
 * - AC .6: two frames of the scrolling live chart, taken a known Δt apart, are compared
 *   at the same data time. Only IBUS (a [12, 4] dash) is shown; each plot column of the
 *   main canvas is "on" where IBUS ink is in it, and the on/off edges of the two frames,
 *   mapped to session ms through each draw's x range (the e2e hook's `chartDraws()`),
 *   must line up within 1 px. Before the fix the pattern restarted at the first point in
 *   view, so the edges slid by up to half a pattern. The frames go to
 *   `../qa-runs/DASH-visual/` (outside the repo) for review.
 * - AC .5: at 1000 SPS the Meter tab keeps its frame rate within 5% of the build before
 *   the fix ({@link BASELINE_FPS}).
 *
 * Both run at DPR 1 so a canvas px is a CSS px.
 */

import { mkdirSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { expect, test, type Page } from "@playwright/test";
import { THEME_COLORS } from "../../src/lib/theme";
import { HOOK } from "../hook";
import { blur, chartDraws, chartSeriesLengths, open, streamDemo, useTheme } from "./helpers";

test.use({ viewport: { width: 1600, height: 900 }, deviceScaleFactor: 1 });

/** Where the frame pair for review goes: `qa-runs/DASH-visual/` next to the repo. */
const QA_DIR = fileURLToPath(new URL("../../../../../qa-runs/DASH-visual/", import.meta.url));

/**
 * AC-SX1VTC.5: frames a second on the Meter tab at 1000 SPS, WebKit, measured on the
 * build before this fix: 60.1 fps (HEAD eb534f8, 2026-11-03, this Mac, headless WebKit,
 * 5 s window, 1600 × 900 at DPR 1; WebKit caps rAF at 60). The fix may cost at most 5%.
 */
const BASELINE_FPS = 60;
const FPS_TOLERANCE = 0.05;
/** One frame-rate window, ms. */
const FPS_WINDOW_MS = 5_000;

/** Largest mismatch, px, between the two frames' dash edges at the same data time. */
const MAX_EDGE_SHIFT_PX = 1;

/** rAF frames a second over the next `ms`. */
async function framesPerSecond(page: Page, ms: number): Promise<number> {
  return page.evaluate(
    (windowMs) =>
      new Promise<number>((resolve) => {
        let frames = 0;
        let t0 = -1;
        const tick = (t: number): void => {
          if (t0 < 0) t0 = t;
          else frames++;
          if (t - t0 < windowMs) requestAnimationFrame(tick);
          else resolve(frames / ((t - t0) / 1000));
        };
        requestAnimationFrame(tick);
      }),
    ms,
  );
}

interface Shot {
  /** The draw's x range, session ms. */
  t0Ms: number;
  t1Ms: number;
  /** Plot area in canvas px. */
  plotLeft: number;
  plotWidth: number;
  /** Per plot column: IBUS ink in it. */
  on: boolean[];
  /** The whole plot canvas, PNG data URL. */
  png: string;
}

/** The main plot canvas and the draw it shows, read in one task so they match. */
async function shoot(page: Page, rgb: [number, number, number]): Promise<Shot> {
  return page.evaluate(
    async ({ hook, color }) => {
      await new Promise((r) => requestAnimationFrame(() => setTimeout(r, 0)));
      const canvas = document.querySelector<HTMLCanvasElement>(".plot canvas");
      const over = document.querySelector<HTMLElement>(".plot .u-over");
      const ctx = canvas?.getContext("2d");
      if (!canvas || !over || !ctx) throw new Error("no plot canvas");
      const h = (
        window as unknown as Record<string, { chartDraws(): { t0Ms: number; t1Ms: number }[] }>
      )[hook]!;
      const draw = h.chartDraws().at(-1);
      if (!draw) throw new Error("no draw yet");
      const cr = canvas.getBoundingClientRect();
      const or = over.getBoundingClientRect();
      const ratio = canvas.width / cr.width;
      const plotLeft = Math.round((or.left - cr.left) * ratio);
      const plotTop = Math.round((or.top - cr.top) * ratio);
      const plotWidth = Math.round(or.width * ratio);
      const plotHeight = Math.round(or.height * ratio);
      const { data } = ctx.getImageData(plotLeft, plotTop, plotWidth, plotHeight);
      const on: boolean[] = [];
      for (let x = 0; x < plotWidth; x++) {
        let hit = false;
        for (let y = 0; y < plotHeight && !hit; y++) {
          const k = (y * plotWidth + x) * 4;
          const d = Math.max(
            Math.abs(data[k]! - color[0]),
            Math.abs(data[k + 1]! - color[1]),
            Math.abs(data[k + 2]! - color[2]),
          );
          hit = d <= 48 && data[k + 3]! > 128;
        }
        on.push(hit);
      }
      return {
        t0Ms: draw.t0Ms,
        t1Ms: draw.t1Ms,
        plotLeft,
        plotWidth,
        on,
        png: canvas.toDataURL("image/png"),
      };
    },
    { hook: HOOK, color: rgb },
  );
}

/** The on/off edges of a shot, as session ms (column boundaries), and whether each is a
 * dash's start (off → on) or its end. */
function edgesMs(s: Shot): { t: number; start: boolean }[] {
  const msPerPx = (s.t1Ms - s.t0Ms) / s.plotWidth;
  const out: { t: number; start: boolean }[] = [];
  for (let x = 1; x < s.on.length; x++) {
    if (s.on[x] !== s.on[x - 1]) out.push({ t: s.t0Ms + x * msPerPx, start: s.on[x] === true });
  }
  return out;
}

function hexRgb(hex: string): [number, number, number] {
  const n = Number.parseInt(hex.slice(1), 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

/** Shows only `keep` in the main chart's legend. */
async function showOnly(page: Page, keep: string): Promise<void> {
  const legend = page.getByRole("group", { name: "Series", exact: true }).first();
  for (const label of ["VBUS", "IBUS", "Power", "D\\+", "D−", "CC1", "CC2"]) {
    const b = legend.getByRole("button", { name: new RegExp(`^${label}\\s`) });
    const want = label === keep ? "true" : "false";
    if ((await b.getAttribute("aria-pressed")) !== want) await b.click();
    await expect(b).toHaveAttribute("aria-pressed", want);
  }
}

test("AC-SX1VTC.6: IBUS dashes stay on the same data time from one live frame to the next", async ({
  page,
}) => {
  test.setTimeout(90_000);
  await useTheme(page, "light");
  await open(page);
  await streamDemo(page);
  await showOnly(page, "IBUS");
  await blur(page);
  // The 10 s live window is full once the session is older than it: the span holds, so
  // the frames differ by a scroll only.
  await expect
    .poll(
      async () => {
        const d = (await chartDraws(page)).filter((x) => x.mode === "live").at(-1);
        return d ? d.t1Ms - d.t0Ms : 0;
      },
      { timeout: 30_000 },
    )
    .toBeGreaterThanOrEqual(10_000);
  await expect.poll(async () => (await chartSeriesLengths(page)).ibus).toBeGreaterThan(50);

  const rgb = hexRgb(THEME_COLORS.light.chart.current!);
  const a = await shoot(page, rgb);
  // A known Δt later: the draws carry their own x ranges, so the gap need not be exact.
  await page.waitForTimeout(700);
  const b = await shoot(page, rgb);

  mkdirSync(QA_DIR, { recursive: true });
  writeFileSync(`${QA_DIR}frame-a.png`, Buffer.from(a.png.split(",")[1]!, "base64"));
  writeFileSync(`${QA_DIR}frame-b.png`, Buffer.from(b.png.split(",")[1]!, "base64"));
  writeFileSync(
    `${QA_DIR}frames.json`,
    JSON.stringify(
      { a: { t0Ms: a.t0Ms, t1Ms: a.t1Ms }, b: { t0Ms: b.t0Ms, t1Ms: b.t1Ms } },
      null,
      2,
    ),
  );

  const dt = b.t0Ms - a.t0Ms;
  const msPerPx = (a.t1Ms - a.t0Ms) / a.plotWidth;
  expect(dt, "the view scrolled between the frames").toBeGreaterThan(5 * msPerPx);
  expect(Math.abs(b.t1Ms - b.t0Ms - (a.t1Ms - a.t0Ms)), "same span").toBeLessThan(msPerPx);

  // Edges at the same data time, away from both frames' plot edges.
  const lo = b.t0Ms + 4 * msPerPx;
  const hi = a.t1Ms - 4 * msPerPx;
  const ea = edgesMs(a).filter((e) => e.t >= lo && e.t <= hi);
  const eb = edgesMs(b);
  expect(ea.length, "dash edges in both frames").toBeGreaterThan(40);
  const shifts = ea.map(
    (e) =>
      eb.reduce((m, o) => (o.start === e.start ? Math.min(m, Math.abs(o.t - e.t)) : m), Infinity) /
      msPerPx,
  );
  const worst = Math.max(...shifts);
  const off = shifts.filter((s) => s > MAX_EDGE_SHIFT_PX).length;
  expect(
    worst,
    `Δt ${dt.toFixed(1)} ms: ${off} of ${shifts.length} dash edges moved more than ${MAX_EDGE_SHIFT_PX} px (worst ${worst.toFixed(2)} px)`,
  ).toBeLessThanOrEqual(MAX_EDGE_SHIFT_PX);
});

test("AC-SX1VTC.5: the Meter tab at 1000 SPS keeps its frame rate", async ({ page }) => {
  test.setTimeout(90_000);
  await open(page);
  await streamDemo(page);
  await blur(page);
  await page.keyboard.press("4");
  await expect(page.getByLabel("Sample rate")).toHaveValue("1000");
  await expect.poll(async () => (await chartSeriesLengths(page)).vbus).toBeGreaterThan(1000);
  const fps = await framesPerSecond(page, FPS_WINDOW_MS);
  const floor = BASELINE_FPS * (1 - FPS_TOLERANCE);
  console.log(
    `AC-SX1VTC.5: ${fps.toFixed(1)} fps (baseline ${BASELINE_FPS}, floor ${floor.toFixed(1)})`,
  );
  expect(fps, `${fps.toFixed(1)} fps at 1000 SPS`).toBeGreaterThanOrEqual(floor);
});
