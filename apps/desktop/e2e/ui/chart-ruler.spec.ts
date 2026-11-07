/**
 * FR AXHK2T on the mocked backend: the two-cursor ruler on the live chart and on an
 * opened recording, by mouse and by keyboard, its values against the raw samples, segment
 * marks, the E.D. series, the sub-chart, Reverse current, and the options kept across a
 * reload with the chart drawn as before when they are all off.
 *
 * UI pinned here, beyond the FR's Technical Design:
 *
 * - each chart (the live one in the "Chart" region, the one in the "Recording file"
 *   region) has a "Chart options" button (`aria-expanded`) that shows a "Chart options"
 *   group: toggle buttons "Ruler", "E.D.", "Sub-chart" and "Reverse current"
 *   (`aria-pressed`), and selects "Segment marks" (`off`, `charge`, `energy`) and
 *   "Mark step" (`5`, `10`, `20`, `25`);
 * - the ruler's cursors are sliders "Ruler cursor T1" / "Ruler cursor T2", focusable,
 *   `aria-valuenow` in ms since the session (or file) start, `aria-valuetext` a time;
 *   ← / → move the focused one (Shift: further), PageUp / PageDown a big step, Home /
 *   End to the ends of the session (or file), and nothing else; a cursor out of view
 *   stays focusable and shows at the near edge with its name and time;
 * - the values are in a group named "Ruler readout", each in an element labelled
 *   "Delta time", "Delta charge", "Delta energy", "Average voltage", "Average current",
 *   "Average power", next to a "Reset ruler" button; a polite status "Ruler
 *   announcement" reads them out once they settle;
 * - with E.D. on the chart shows `ED_DEFINITION`;
 * - the sub-chart is a region "Sub-chart" below the main plot, with its own legend, a
 *   group "Sub-chart series";
 * - the test hook gains `chartExtras()`: `{ segmentMarks: { percent, tMs, label }[],
 *   ampsDir: 1 | -1, edPoints: number, sub: { t0Ms, t1Ms, playheadMs, playheadX } | null,
 *   playheadX }` for the live chart's last draw (marks drawn, the amps scale's
 *   direction, E.D. points drawn, the sub-chart's x range and playhead, the playhead's
 *   viewport x in each plot), and `window.__OPEN_POWERZ_E2E_RECORDING__()` the same for
 *   an opened recording.
 *
 * WebKit never reaches uPlot's own drag-select, so drags here are plain mouse presses
 * and moves, which the components handle themselves.
 */

import { expect, test, type Locator, type Page } from "@playwright/test";
import { ED_DEFINITION } from "../../src/lib/chartExtras";
import { MOCK_RECORDING_FILE } from "../../src/lib/mockBackend";
import { HOOK, RECORDING_HOOK, type RecordingExtrasState } from "../hook";
import {
  chartPlayhead,
  chartSeriesLengths,
  expectHook,
  lastDraw,
  open,
  plotAreas,
  settled,
  streamDemo,
} from "./helpers";

test.use({ viewport: { width: 1600, height: 900 } });

interface ChartExtrasProbe {
  segmentMarks: { percent: number; tMs: number; label: string }[];
  ampsDir: 1 | -1;
  edPoints: number;
  sub: { t0Ms: number; t1Ms: number; playheadMs: number | null; playheadX: number | null } | null;
  playheadX: number | null;
}

async function recordingExtras(page: Page): Promise<RecordingExtrasState> {
  return page.evaluate((name) => {
    const fn = (window as unknown as Record<string, unknown>)[name];
    if (typeof fn !== "function") throw new Error("no recording probe");
    return (fn as () => RecordingExtrasState)();
  }, RECORDING_HOOK);
}

async function extras(page: Page): Promise<ChartExtrasProbe> {
  await expectHook(page);
  return page.evaluate((name) => {
    const hook = (window as unknown as Record<string, Record<string, unknown>>)[name]!;
    const fn = hook.chartExtras;
    if (typeof fn !== "function") throw new Error("the test hook has no chartExtras()");
    return (fn as () => ChartExtrasProbe).call(hook);
  }, HOOK);
}

function chart(page: Page): Locator {
  return page.getByRole("region", { name: "Chart", exact: true });
}

function fileView(page: Page): Locator {
  return page.getByRole("region", { name: "Recording file" });
}

function button(scope: Page | Locator, name: string): Locator {
  return scope.getByRole("button", { name, exact: true });
}

/** The "Chart options" group of `scope`, opened. */
async function options(scope: Locator): Promise<Locator> {
  const toggle = button(scope, "Chart options");
  if ((await toggle.getAttribute("aria-expanded")) !== "true") await toggle.click();
  await expect(toggle).toHaveAttribute("aria-expanded", "true");
  const group = scope.getByRole("group", { name: "Chart options", exact: true });
  await expect(group).toBeVisible();
  return group;
}

async function turnOn(scope: Locator, name: string): Promise<void> {
  const group = await options(scope);
  const b = button(group, name);
  if ((await b.getAttribute("aria-pressed")) !== "true") await b.click();
  await expect(b).toHaveAttribute("aria-pressed", "true");
}

function cursor(scope: Locator, id: "T1" | "T2"): Locator {
  return scope.getByRole("slider", { name: `Ruler cursor ${id}`, exact: true });
}

function readout(scope: Locator): Locator {
  return scope.getByRole("group", { name: "Ruler readout", exact: true });
}

function announcer(scope: Locator): Locator {
  return scope.getByRole("status", { name: "Ruler announcement", exact: true });
}

async function valueNow(slider: Locator): Promise<number> {
  return Number(await slider.getAttribute("aria-valuenow"));
}

/** The number a readout value shows (true minus signs read back as plain ones). */
async function shown(scope: Locator, name: string): Promise<number> {
  const text = (await readout(scope).getByLabel(name, { exact: true }).innerText()).replace(
    /\u2212/g,
    "-",
  );
  const m = /-?\d+(?:\.\d+)?/.exec(text);
  return m === null ? NaN : Number(m[0]);
}

/** Drags the centre of `el` by `dx` CSS px with plain mouse moves. */
async function dragBy(page: Page, el: Locator, dx: number): Promise<void> {
  const box = await el.boundingBox();
  if (box === null) throw new Error("nothing to drag");
  const x = box.x + box.width / 2;
  const y = box.y + box.height / 2;
  await page.mouse.move(x, y);
  await page.mouse.down();
  await page.mouse.move(x + dx, y, { steps: 6 });
  await page.mouse.up();
}

/** Stream the demo device, then pause the live chart on what it holds. */
async function streamAndPause(page: Page): Promise<void> {
  await open(page);
  await streamDemo(page);
  await expect.poll(async () => (await chartSeriesLengths(page)).vbus).toBeGreaterThan(50);
  await button(chart(page), "Live").click();
  await expect(button(chart(page), "Paused · Go live")).toBeVisible();
}

/** The `load-5v-1a` demo reading at `tMs` (the scenario's formula). */
function load(tMs: number): { v: number; i: number } {
  const t = tMs / 1000;
  return { v: 5 + 0.01 * Math.sin(t * 7), i: 1 + 0.02 * Math.sin(t * 3) };
}

/** Charge, energy and mean current of the whole-ms file rows `t0..=t1`, from the raw
 * readings by the trapezoid rule. */
function reference(t0: number, t1: number): { ah: number; wh: number; a: number } {
  let ah = 0;
  let wh = 0;
  let sum = 0;
  let n = 0;
  let prev: { v: number; i: number } | null = null;
  for (let t = Math.ceil(t0); t <= Math.floor(t1); t++) {
    const s = load(t);
    if (prev) {
      ah += (prev.i + s.i) / 2 / 3_600_000;
      wh += (prev.v * prev.i + s.v * s.i) / 2 / 3_600_000;
    }
    prev = s;
    sum += s.i;
    n++;
  }
  return { ah, wh, a: sum / n };
}

const near = (got: number, want: number): number => Math.abs(got - want) / Math.abs(want);

test("Ruler places T1 and T2 and shows the six values between them (AC-AXHK2T.1)", async ({
  page,
}) => {
  await streamAndPause(page);
  const c = chart(page);
  await expect(cursor(c, "T1")).toHaveCount(0);
  await turnOn(c, "Ruler");

  const t1 = cursor(c, "T1");
  const t2 = cursor(c, "T2");
  await expect(t1).toBeVisible();
  await expect(t2).toBeVisible();
  expect(await valueNow(t2)).toBeGreaterThan(await valueNow(t1));

  const r = readout(c);
  await expect(r.getByLabel("Delta time", { exact: true })).toHaveText(/\d\.\d{3}( s)?$/);
  for (const [name, unit] of [
    ["Delta charge", "Ah"],
    ["Delta energy", "Wh"],
    ["Average voltage", "V"],
    ["Average current", "A"],
    ["Average power", "W"],
  ] as const) {
    await expect(r.getByLabel(name, { exact: true })).toHaveText(new RegExp(`\\d ${unit}$`));
  }
  // The 5 V / 1 A load, averaged between the cursors.
  await expect.poll(() => shown(c, "Average voltage")).toBeCloseTo(5, 1);
  await expect.poll(() => shown(c, "Average current")).toBeCloseTo(1, 1);

  // Dragging T2 right widens the span and grows the charge.
  const before = await valueNow(t2);
  const timeBefore = await r.getByLabel("Delta time", { exact: true }).innerText();
  const chargeBefore = await shown(c, "Delta charge");
  await dragBy(page, t2, 120);
  await expect.poll(() => valueNow(t2)).toBeGreaterThan(before);
  await expect(r.getByLabel("Delta time", { exact: true })).not.toHaveText(timeBefore);
  await expect.poll(() => shown(c, "Delta charge")).toBeGreaterThan(chargeBefore);

  // Dragging T1 moves T1, not T2.
  const t1Before = await valueNow(t1);
  const t2Now = await valueNow(t2);
  await dragBy(page, t1, -80);
  await expect.poll(() => valueNow(t1)).toBeLessThan(t1Before);
  expect(await valueNow(t2)).toBe(t2Now);
});

test("the ruler on an opened recording matches the raw samples within 0.1% (AC-AXHK2T.2)", async ({
  page,
}) => {
  await open(page);
  await button(page, "Open recording").click();
  const view = fileView(page);
  await expect(view).toBeVisible();
  await turnOn(view, "Ruler");

  const check = async (): Promise<void> => {
    const a = await valueNow(cursor(view, "T1"));
    const b = await valueNow(cursor(view, "T2"));
    const want = reference(Math.min(a, b), Math.max(a, b));
    await expect
      .poll(async () => near(await shown(view, "Delta charge"), want.ah), {
        message: "ΔAh against the raw rows",
      })
      .toBeLessThanOrEqual(1e-3);
    expect(near(await shown(view, "Delta energy"), want.wh), "ΔWh").toBeLessThanOrEqual(1e-3);
    expect(near(await shown(view, "Average current"), want.a), "Avg A").toBeLessThanOrEqual(1e-3);
  };

  const t1 = await valueNow(cursor(view, "T1"));
  const t2 = await valueNow(cursor(view, "T2"));
  expect(t1).toBeGreaterThanOrEqual(0);
  expect(t2).toBeLessThanOrEqual(MOCK_RECORDING_FILE.rows);
  await check();

  // Still the raw samples after a drag.
  await dragBy(page, cursor(view, "T2"), 150);
  await expect.poll(() => valueNow(cursor(view, "T2"))).toBeGreaterThan(t2);
  await check();
});

test("the ruler works by keyboard and reads out to a screen reader (AC-AXHK2T.3)", async ({
  page,
}) => {
  await streamAndPause(page);
  const c = chart(page);
  await turnOn(c, "Ruler");
  const t1 = cursor(c, "T1");
  const t2 = cursor(c, "T2");
  await expect(t1).toHaveAttribute("tabindex", "0");
  await expect(t2).toHaveAttribute("tabindex", "0");
  await expect(t1).toHaveAttribute("aria-valuetext", /\d\.\d{3} s|\d:\d{2}\.\d{3}/);

  const r = readout(c);
  await expect(r).toBeVisible();
  for (const name of [
    "Delta time",
    "Delta charge",
    "Delta energy",
    "Average voltage",
    "Average current",
    "Average power",
  ]) {
    await expect(r.getByLabel(name, { exact: true })).toHaveCount(1);
  }

  const playhead = await chartPlayhead(page);
  await t1.focus();
  const start = await valueNow(t1);
  const t2Start = await valueNow(t2);
  const timeText = await r.getByLabel("Delta time", { exact: true }).innerText();
  await page.keyboard.press("ArrowRight");
  await page.keyboard.press("ArrowRight");
  await page.keyboard.press("ArrowRight");
  await expect.poll(() => valueNow(t1)).toBeGreaterThan(start);
  const fine = (await valueNow(t1)) - start;
  await expect(r.getByLabel("Delta time", { exact: true })).not.toHaveText(timeText);

  // Shift steps further than a plain arrow.
  const beforeCoarse = await valueNow(t1);
  await page.keyboard.press("Shift+ArrowRight");
  await expect.poll(() => valueNow(t1)).toBeGreaterThan(beforeCoarse + fine / 3);

  const beforeLeft = await valueNow(t1);
  await page.keyboard.press("ArrowLeft");
  await expect.poll(() => valueNow(t1)).toBeLessThan(beforeLeft);

  // The arrows moved the focused cursor only: not T2, not the playhead.
  expect(await valueNow(t2)).toBe(t2Start);
  expect(await chartPlayhead(page)).toBe(playhead);

  // T2 takes the arrows once it has the focus.
  await t2.focus();
  await page.keyboard.press("ArrowLeft");
  await expect.poll(() => valueNow(t2)).toBeLessThan(t2Start);

  // The screen reader hears the values once they settle, from a polite status.
  await expect(announcer(c)).toHaveAttribute("aria-live", "polite");
  const dt = await r.getByLabel("Delta time", { exact: true }).innerText();
  await expect(announcer(c)).toContainText(`Delta time ${dt}`);
  await expect(announcer(c)).not.toContainText("…");
});

test("an off-view cursor stays focusable, keys keep it, Reset ruler brings both back (AC-AXHK2T.3)", async ({
  page,
}) => {
  await open(page);
  await streamDemo(page);
  await expect.poll(async () => (await chartSeriesLengths(page)).vbus).toBeGreaterThan(50);
  const c = chart(page);
  await button(c, "10 s").click();
  await turnOn(c, "Ruler");
  const t1 = cursor(c, "T1");
  await expect(t1).toBeVisible();
  // The live view scrolls on: within a few seconds T1 is behind it.
  await expect(t1).toHaveAttribute("aria-valuetext", /before the view/, { timeout: 15_000 });
  await expect(t1).toBeVisible();
  await expect(t1).toContainText("T1");
  const plotBox = await c.locator(".plot .u-over").first().boundingBox();
  const t1Box = await t1.boundingBox();
  if (plotBox === null || t1Box === null) throw new Error("not laid out");
  // At the plot's left edge, with a 24 px target (WCAG 2.5.8).
  expect(Math.abs(t1Box.x + t1Box.width / 2 - plotBox.x)).toBeLessThanOrEqual(1);
  expect(t1Box.width).toBeGreaterThanOrEqual(24);

  // Focus stays on the cursor whatever the keys, and the view does not move.
  await t1.focus();
  for (const key of ["ArrowLeft", "End", "PageDown", "ArrowRight", "Home"]) {
    await page.keyboard.press(key);
    await expect(t1, key).toBeFocused();
  }
  await expect(button(c, "Paused · Go live")).toHaveCount(0);

  const before = await valueNow(t1);
  await button(c, "Reset ruler").click();
  // Both back in the live view: T1 a third and T2 two thirds into it.
  await expect.poll(() => valueNow(t1)).toBeGreaterThan(before);
  await expect(cursor(c, "T2")).not.toHaveAttribute("aria-valuetext", /the view/);
  expect(await valueNow(cursor(c, "T2"))).toBeGreaterThan(await valueNow(t1));
});

test("Home, End and the page keys move a cursor, never the view (AC-AXHK2T.3)", async ({
  page,
}) => {
  await streamAndPause(page);
  const c = chart(page);
  await turnOn(c, "Ruler");
  const t1 = cursor(c, "T1");
  const view = await lastDraw(page);
  await t1.focus();
  const start = await valueNow(t1);
  await page.keyboard.press("PageUp");
  await expect.poll(() => valueNow(t1)).toBeGreaterThan(start);
  const up = await valueNow(t1);
  await page.keyboard.press("PageDown");
  await expect.poll(() => valueNow(t1)).toBeLessThan(up);
  await page.keyboard.press("Home");
  await expect.poll(() => valueNow(t1)).toBe(Number(await t1.getAttribute("aria-valuemin")));
  await page.keyboard.press("End");
  await expect.poll(() => valueNow(t1)).toBe(Number(await t1.getAttribute("aria-valuemax")));
  await expect(t1).toBeFocused();
  // The plot's own Home / End did not run: the view is where it was.
  const now = await lastDraw(page);
  expect([now.t0Ms, now.t1Ms]).toEqual([view.t0Ms, view.t1Ms]);
  await expect(button(c, "Paused · Go live")).toBeVisible();
});

test("the cursor takes the pointer at its grip only, not on the PD marker lane (AC-AXHK2T.3)", async ({
  page,
}) => {
  await streamAndPause(page);
  const c = chart(page);
  await turnOn(c, "Ruler");
  const t1 = cursor(c, "T1");
  const box = await t1.boundingBox();
  if (box === null) throw new Error("not laid out");
  const x = box.x + box.width / 2;
  const hit = (y: number) =>
    t1.evaluate((el, [px, py]) => el.contains(document.elementFromPoint(px!, py!)), [x, y]);
  // Over the PD marker lane (the plot's top 16 px) a click reaches the plot.
  expect(await hit(box.y + 8)).toBe(false);
  // Below it the cursor takes the press.
  expect(await hit(box.y + box.height / 2)).toBe(true);
});

test("segment marks draw at each step of the run's charge or energy (AC-AXHK2T.4)", async ({
  page,
}) => {
  await open(page);
  await streamDemo(page);
  await expect.poll(async () => (await chartSeriesLengths(page)).vbus).toBeGreaterThan(50);
  const c = chart(page);
  await button(c, "Whole session").click();
  expect((await extras(page)).segmentMarks).toEqual([]);

  const group = await options(c);
  const marks = group.getByLabel("Segment marks", { exact: true });
  const step = group.getByLabel("Mark step", { exact: true });
  await expect(marks).toHaveValue("off");
  await expect(step).toHaveValue("10");

  await marks.selectOption("charge");
  const tenths = ["10%", "20%", "30%", "40%", "50%", "60%", "70%", "80%", "90%"];
  await expect
    .poll(async () => (await extras(page)).segmentMarks.map((m) => m.label))
    .toEqual(tenths);
  const drawn = (await extras(page)).segmentMarks;
  expect(drawn.map((m) => m.percent)).toEqual([10, 20, 30, 40, 50, 60, 70, 80, 90]);
  for (let k = 1; k < drawn.length; k++) {
    expect(drawn[k]!.tMs, "marks in time order").toBeGreaterThan(drawn[k - 1]!.tMs);
  }

  await step.selectOption("25");
  await expect
    .poll(async () => (await extras(page)).segmentMarks.map((m) => m.label))
    .toEqual(["25%", "50%", "75%"]);

  await marks.selectOption("energy");
  await expect
    .poll(async () => (await extras(page)).segmentMarks.map((m) => m.label))
    .toEqual(["25%", "50%", "75%"]);

  await marks.selectOption("off");
  await expect.poll(async () => (await extras(page)).segmentMarks).toEqual([]);
});

test("E.D. draws the energy share, labelled as our definition (AC-AXHK2T.5)", async ({ page }) => {
  await open(page);
  await streamDemo(page);
  await expect.poll(async () => (await chartSeriesLengths(page)).vbus).toBeGreaterThan(50);
  const c = chart(page);
  expect((await extras(page)).edPoints).toBe(0);
  await expect(c).not.toContainText(ED_DEFINITION);

  await turnOn(c, "E.D.");
  await expect.poll(async () => (await extras(page)).edPoints).toBeGreaterThan(0);
  await expect(c).toContainText(ED_DEFINITION);
});

test("the sub-chart follows the main plot's range, cursor and zoom (AC-AXHK2T.6)", async ({
  page,
}) => {
  await streamAndPause(page);
  const c = chart(page);
  await expect(c.getByRole("region", { name: "Sub-chart" })).toHaveCount(0);
  expect((await extras(page)).sub).toBeNull();

  await turnOn(c, "Sub-chart");
  const sub = c.getByRole("region", { name: "Sub-chart" });
  await expect(sub).toBeVisible();
  await expect(sub.locator("canvas").first()).toBeVisible();

  // Below the main plot. The plot shrinks over a few frames to make room (its resize
  // waits for the next frame), so both boxes are read in one go once they have settled;
  // two reads frames apart could pair the old plot with the new sub-chart (FR 82AH25).
  const boxes = await settled(page, () =>
    page.evaluate(() => {
      const plot = document.querySelector(".plot")?.getBoundingClientRect();
      const sub = document.querySelector('[aria-label="Sub-chart"]')?.getBoundingClientRect();
      return plot && sub ? { plotBottom: plot.bottom, subTop: sub.top } : null;
    }),
  );
  if (boxes === null) throw new Error("no plots");
  expect(boxes.subTop).toBeGreaterThanOrEqual(boxes.plotBottom - 1);

  // The same time range.
  await expect
    .poll(async () => {
      const [s, d] = [(await extras(page)).sub, await lastDraw(page)];
      return s === null ? Infinity : Math.abs(s.t0Ms - d.t0Ms) + Math.abs(s.t1Ms - d.t1Ms);
    })
    .toBeLessThanOrEqual(1);

  // Its own series choice: a toggle there leaves the main legend as it was.
  const subLegend = sub.getByRole("group", { name: "Sub-chart series", exact: true });
  const mainVbus = button(c.getByRole("group", { name: "Series", exact: true }), "VBUS (V)");
  const subVbus = subLegend.getByRole("button", { name: /^VBUS/ });
  const mainPressed = await mainVbus.getAttribute("aria-pressed");
  const subPressed = await subVbus.getAttribute("aria-pressed");
  await subVbus.click();
  await expect(subVbus).not.toHaveAttribute("aria-pressed", subPressed ?? "");
  await expect(mainVbus).toHaveAttribute("aria-pressed", mainPressed ?? "");

  // A zoom on the main plot zooms the sub-chart to the same range.
  const before = await lastDraw(page);
  await c.locator(".plot").first().focus();
  await page.keyboard.press("ArrowUp");
  await expect
    .poll(async () => (await lastDraw(page)).t1Ms - (await lastDraw(page)).t0Ms)
    .toBeLessThan(before.t1Ms - before.t0Ms);
  await expect
    .poll(async () => {
      const [s, d] = [(await extras(page)).sub, await lastDraw(page)];
      return s === null ? Infinity : Math.abs(s.t0Ms - d.t0Ms) + Math.abs(s.t1Ms - d.t1Ms);
    })
    .toBeLessThanOrEqual(1);

  // A click on the main plot puts the same cursor on the sub-chart.
  const over = c.locator(".plot .u-over").first();
  const box = await over.boundingBox();
  if (box === null) throw new Error("no plot area");
  await page.mouse.click(box.x + box.width * 0.5, box.y + box.height / 2);
  await expect.poll(() => chartPlayhead(page)).not.toBeNull();
  const head = await chartPlayhead(page);
  await expect.poll(async () => (await extras(page)).sub?.playheadMs ?? null).toBe(head);
});

test("Reverse current flips the amps axis, not the data (AC-AXHK2T.7)", async ({ page }) => {
  await open(page);
  await streamDemo(page);
  await expect.poll(async () => (await chartSeriesLengths(page)).ibus).toBeGreaterThan(50);
  const c = chart(page);
  expect((await extras(page)).ampsDir).toBe(1);

  await turnOn(c, "Reverse current");
  await expect.poll(async () => (await extras(page)).ampsDir).toBe(-1);
  // The samples still draw, with their own sign: the hover reads +1 A, not −1 A.
  await expect.poll(async () => (await chartSeriesLengths(page)).ibus).toBeGreaterThan(50);
  const over = c.locator(".plot .u-over").first();
  const box = await over.boundingBox();
  if (box === null) throw new Error("no plot area");
  await page.mouse.move(box.x + box.width * 0.6, box.y + box.height / 2);
  const ibus = c.locator(".hover").first().locator(".hover-value", { hasText: "IBUS" });
  await expect(ibus).toHaveText(/IBUS\s+\+?(0\.9|1\.0)\d*(–\+?(0\.9|1\.0)\d*)? A/);
  await expect(ibus).not.toContainText("\u2212");
  // The statistics keep the sign too.
  const stats = page.getByRole("complementary", { name: "Statistics" });
  await expect(stats).not.toContainText(/\u22121\.0\d\d A/);
});

test("each option is kept across a reload (AC-AXHK2T.8)", async ({ page }) => {
  await open(page);
  const c = chart(page);
  for (const name of ["Ruler", "E.D.", "Sub-chart", "Reverse current"]) await turnOn(c, name);
  let group = await options(c);
  await group.getByLabel("Segment marks", { exact: true }).selectOption("energy");
  await group.getByLabel("Mark step", { exact: true }).selectOption("20");

  await page.reload();
  await open(page);
  group = await options(chart(page));
  for (const name of ["Ruler", "E.D.", "Sub-chart", "Reverse current"]) {
    await expect(button(group, name)).toHaveAttribute("aria-pressed", "true");
  }
  await expect(group.getByLabel("Segment marks", { exact: true })).toHaveValue("energy");
  await expect(group.getByLabel("Mark step", { exact: true })).toHaveValue("20");

  for (const name of ["Ruler", "E.D.", "Sub-chart", "Reverse current"]) {
    await button(group, name).click();
    await expect(button(group, name)).toHaveAttribute("aria-pressed", "false");
  }
  await group.getByLabel("Segment marks", { exact: true }).selectOption("off");
  await page.reload();
  await open(page);
  group = await options(chart(page));
  for (const name of ["Ruler", "E.D.", "Sub-chart", "Reverse current"]) {
    await expect(button(group, name)).toHaveAttribute("aria-pressed", "false");
  }
  await expect(group.getByLabel("Segment marks", { exact: true })).toHaveValue("off");
  await expect(group.getByLabel("Mark step", { exact: true })).toHaveValue("20");
});

test("with every option off the chart draws as before (AC-AXHK2T.8)", async ({ page }) => {
  await open(page);
  await streamDemo(page);
  await expect.poll(async () => (await chartSeriesLengths(page)).vbus).toBeGreaterThan(50);
  const c = chart(page);
  const group = await options(c);
  for (const name of ["Ruler", "E.D.", "Sub-chart", "Reverse current"]) {
    await expect(button(group, name)).toHaveAttribute("aria-pressed", "false");
  }
  await expect(group.getByLabel("Segment marks", { exact: true })).toHaveValue("off");

  expect(await extras(page)).toEqual({
    segmentMarks: [],
    ampsDir: 1,
    edPoints: 0,
    sub: null,
    playheadX: null,
  });
  await expect(c.getByRole("slider", { name: /^Ruler cursor/ })).toHaveCount(0);
  await expect(readout(c)).toHaveCount(0);
  await expect(c.getByRole("region", { name: "Sub-chart" })).toHaveCount(0);
  // One plot, as before.
  await expect(c.locator(".uplot")).toHaveCount(1);
  await expect(c).not.toContainText(ED_DEFINITION);
  // The live chart still scrolls and the existing controls are all there.
  const first = await lastDraw(page);
  await expect.poll(async () => (await lastDraw(page)).t1Ms).toBeGreaterThan(first.t1Ms);
  for (const name of ["10 s", "1 min", "10 min", "Whole session", "Zero-based axes", "Live"]) {
    await expect(button(c, name)).toBeVisible();
  }
});

test("an empty total says why nothing is drawn (AC-AXHK2T.4, .5)", async ({ page }) => {
  await open(page);
  await streamDemo(page, "demo:idle");
  await expect.poll(async () => (await chartSeriesLengths(page)).vbus).toBeGreaterThan(50);
  const c = chart(page);
  const group = await options(c);
  await group.getByLabel("Segment marks", { exact: true }).selectOption("charge");
  await expect(c).toContainText("No charge in this run yet, so no marks.");
  await turnOn(c, "E.D.");
  await expect(c).toContainText("No energy in this session yet, so no E.D. line.");
  expect((await extras(page)).segmentMarks).toEqual([]);
  expect((await extras(page)).edPoints).toBe(0);
});

/** How far the sub-chart's plot area is from the main plot's, in px, at each edge. */
function offsets(areas: Awaited<ReturnType<typeof plotAreas>>): { left: number; right: number } {
  if (areas === null) throw new Error("no plot areas");
  return {
    left: Math.abs(areas.sub.left - areas.main.left),
    right: Math.abs(areas.sub.right - areas.main.right),
  };
}

/** The sub-chart on, VBUS off in it: different axes on the left of the two plots. */
async function subChartWithoutVbus(page: Page): Promise<void> {
  await streamAndPause(page);
  const c = chart(page);
  await turnOn(c, "Sub-chart");
  const sub = c.getByRole("region", { name: "Sub-chart" });
  await expect(sub).toBeVisible();
  // Different axes on the left: the sub-chart drops VBUS, so it has no volts axis.
  const subVbus = sub
    .getByRole("group", { name: "Sub-chart series", exact: true })
    .getByRole("button", { name: /^VBUS/ });
  if ((await subVbus.getAttribute("aria-pressed")) === "true") await subVbus.click();
  await expect(subVbus).toHaveAttribute("aria-pressed", "false");
}

test("the sub-chart's time axis lines up with the main plot's (AC-AXHK2T.6)", async ({ page }) => {
  await subChartWithoutVbus(page);

  // AC-CWQ8NJ.1/.2: both plot areas come from one page read, once two reads a couple of
  // frames apart agree. VBUS off rebuilds the sub-chart and the main plot is padded back
  // to match a frame later, so a main box read before that frame and a sub-chart box
  // read after it would differ by the ~10 px the padding adds.
  const before = await settled(page, () => plotAreas(page));
  if (before === null) throw new Error("no plot areas");
  const width = before.main.right - before.main.left;
  await page.mouse.click(
    before.main.left + width * 0.3,
    (before.main.top + before.main.bottom) / 2,
  );
  await expect.poll(() => chartPlayhead(page)).not.toBeNull();
  await expect
    .poll(async () => {
      const e = await extras(page);
      const a = e.playheadX;
      const b = e.sub?.playheadX ?? null;
      return a === null || b === null ? Infinity : Math.abs(a - b);
    })
    .toBeLessThanOrEqual(1);
  // And the plot areas themselves start and end together.
  const after = offsets(await settled(page, () => plotAreas(page)));
  expect(after.left).toBeLessThanOrEqual(1);
  expect(after.right).toBeLessThanOrEqual(1);
});

test("a sub-chart offset by 10 px fails the alignment check (AC-CWQ8NJ.3)", async ({ page }) => {
  await subChartWithoutVbus(page);
  const aligned = offsets(await settled(page, () => plotAreas(page)));
  expect(aligned.left).toBeLessThanOrEqual(1);
  expect(aligned.right).toBeLessThanOrEqual(1);

  // A real misalignment: the sub-chart's plot area moved 10 px right by a style.
  await page.addStyleTag({
    content: '[aria-label="Sub-chart"] .u-over { transform: translateX(10px) !important; }',
  });
  const shifted = offsets(await settled(page, () => plotAreas(page)));
  expect(shifted.left).toBeGreaterThan(1);
  expect(shifted.right).toBeGreaterThan(1);
  expect(shifted.left).toBeCloseTo(10, 0);
  expect(shifted.right).toBeCloseTo(10, 0);
});

test("every tool works on an opened recording (AC-AXHK2T.4–.7)", async ({ page }) => {
  await open(page);
  await button(page, "Open recording").click();
  const view = fileView(page);
  await expect(view).toBeVisible();
  expect((await recordingExtras(page)).segmentMarks).toEqual([]);

  const group = await options(view);
  await group.getByLabel("Segment marks", { exact: true }).selectOption("charge");
  await expect
    .poll(async () => (await recordingExtras(page)).segmentMarks.map((m) => m.label))
    .toEqual(["10%", "20%", "30%", "40%", "50%", "60%", "70%", "80%", "90%"]);
  // From the raw rows: the demo load is nearly flat, so 50% falls near the middle.
  const half = (await recordingExtras(page)).segmentMarks[4]!.tMs;
  expect(Math.abs(half - (MOCK_RECORDING_FILE.rows - 1) / 2)).toBeLessThan(100);

  await turnOn(view, "E.D.");
  await expect.poll(async () => (await recordingExtras(page)).edPoints).toBeGreaterThan(0);
  await expect(view).toContainText(ED_DEFINITION);

  await turnOn(view, "Reverse current");
  await expect.poll(async () => (await recordingExtras(page)).ampsDir).toBe(-1);

  await turnOn(view, "Sub-chart");
  const sub = view.getByRole("region", { name: "Sub-chart" });
  await expect(sub).toBeVisible();
  await expect
    .poll(async () => {
      const e = await recordingExtras(page);
      return e.sub === null
        ? Infinity
        : Math.abs(e.sub.t0Ms - e.t0Ms) + Math.abs(e.sub.t1Ms - e.t1Ms);
    })
    .toBeLessThanOrEqual(1);
  // A zoom on the file's plot zooms the sub-chart too.
  const over = view.locator(".chart .u-over").first();
  const box = await over.boundingBox();
  if (box === null) throw new Error("no plot area");
  await page.mouse.move(box.x + box.width * 0.2, box.y + box.height / 2);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width * 0.6, box.y + box.height / 2, { steps: 6 });
  await page.mouse.up();
  await expect(button(view, "Whole file")).toBeEnabled();
  await expect
    .poll(async () => {
      const e = await recordingExtras(page);
      return e.sub === null
        ? Infinity
        : Math.abs(e.sub.t0Ms - e.t0Ms) + Math.abs(e.sub.t1Ms - e.t1Ms);
    })
    .toBeLessThanOrEqual(1);

  for (const name of ["E.D.", "Sub-chart", "Reverse current"]) await button(group, name).click();
  await expect(view.getByRole("region", { name: "Sub-chart" })).toHaveCount(0);
  await expect.poll(async () => (await recordingExtras(page)).ampsDir).toBe(1);
});

test("on an opened recording the cursors reach the file's last record (AC-AXHK2T.1)", async ({
  page,
}) => {
  await open(page);
  await button(page, "Open recording").click();
  const view = fileView(page);
  await turnOn(view, "Ruler");
  const t2 = cursor(view, "T2");
  await t2.focus();
  await page.keyboard.press("End");
  // One row a millisecond from 0: the last record is at rows - 1 ms, past the start of
  // the last drawn bucket.
  await expect.poll(() => valueNow(t2)).toBe(MOCK_RECORDING_FILE.rows - 1);
  await page.keyboard.press("Home");
  await expect.poll(() => valueNow(t2)).toBe(0);
});

test("the ruler turned off on one chart is cleared on the other too (AC-AXHK2T.8)", async ({
  page,
}) => {
  await streamAndPause(page);
  const c = chart(page);
  await turnOn(c, "Ruler");
  const t1 = cursor(c, "T1");
  await t1.focus();
  await page.keyboard.press("PageUp");
  const view = await lastDraw(page);
  const third = (view.t1Ms - view.t0Ms) / 3;
  const gap = async () => (await valueNow(cursor(c, "T2"))) - (await valueNow(t1));
  await expect.poll(gap).toBeLessThan(third - 10);

  await button(page, "Open recording").click();
  const file = fileView(page);
  const group = await options(file);
  await button(group, "Ruler").click();
  await expect(button(group, "Ruler")).toHaveAttribute("aria-pressed", "false");
  await button(group, "Ruler").click();
  await page.getByRole("tab", { name: "Meter", exact: true }).click();
  // The live chart's ruler was cleared and placed anew: T1 a third into the view again.
  await expect.poll(async () => Math.abs((await gap()) - third)).toBeLessThanOrEqual(2);
});

test("Reverse current leaves an export unchanged (AC-AXHK2T.7)", async ({ page }) => {
  await open(page);
  await streamDemo(page);
  await expect.poll(async () => (await chartSeriesLengths(page)).ibus).toBeGreaterThan(50);
  const exported = () =>
    page.evaluate(() => {
      const mock = (window as unknown as Record<string, { exportedCsv(): string[] }>)
        .__OPEN_POWERZ_MOCK__!;
      return mock.exportedCsv();
    });
  await button(page, "Export").click();
  await expect.poll(async () => (await exported()).length).toBe(1);
  await turnOn(chart(page), "Reverse current");
  await button(page, "Export").click();
  await expect.poll(async () => (await exported()).length).toBe(2);
  const [off, on] = (await exported()).map((csv) => csv.split("\n"));
  expect(off!.length).toBeGreaterThan(10);
  // The rows both exports hold are the same, sign and all.
  expect(on!.slice(0, off!.length)).toEqual(off);
});
