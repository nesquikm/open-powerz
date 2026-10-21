/**
 * FR AH2AEM, the Live button (milestone hardware review: "What does 'live'
 * button do? Can't stop it"). FR DDDYDJ: Live follows the newest data;
 * clicking it while live pauses the chart (the stream and the recording keep
 * running), any pan or zoom pauses it too, and while paused the button reads
 * "Paused · Go live" — clicking it, L or End jumps back.
 */

import { expect, test } from "@playwright/test";
import { blur, chartDraws, chartSeriesLengths, open, streamDemo } from "./helpers";

test.use({ viewport: { width: 1600, height: 900 } });

test("AC-DDDYDJ.1/.2/.4/.5/.6: clicking Live pauses the chart, clicking it again goes live", async ({
  page,
}) => {
  await open(page);
  await streamDemo(page);
  await expect.poll(async () => (await chartSeriesLengths(page)).vbus).toBeGreaterThan(10);
  const live = page.locator(".chart-wrap button.live");
  await expect(live).toHaveText("Live");
  await expect(live).toHaveAttribute("aria-pressed", "true");

  await live.click();
  await expect(live).toHaveText("Paused · Go live");
  await expect(live).toHaveAttribute("aria-pressed", "false");
  await expect(live).toHaveAttribute("title", /End/);
  const status = page.locator('.chart-wrap p[role="status"][aria-live="polite"]');
  await expect(status).toHaveText(/^Paused: .+ to .+$/);
  // The view stays put while the stream goes on.
  await expect.poll(async () => (await chartDraws(page)).at(-1)?.mode).toBe("history");
  const frozen = (await chartDraws(page)).at(-1);
  // Batches keep arriving and the chart keeps drawing, but the range stays put.
  // Wait for a few more draws (by time, the probe keeps only the newest 1000).
  const frozenAt = frozen?.at ?? 0;
  await expect
    .poll(async () => (await chartDraws(page)).filter((d) => d.at > frozenAt).length)
    .toBeGreaterThan(3);
  const later = (await chartDraws(page)).at(-1);
  expect(later?.t1Ms).toBe(frozen?.t1Ms);
  await expect(page.locator(".badge")).toHaveText("Streaming");

  await live.click();
  await expect(live).toHaveText("Live");
  await expect(live).toHaveAttribute("aria-pressed", "true");
  await expect(status).toHaveText(/^Live: \S/);
  await expect
    .poll(async () => (await chartDraws(page)).at(-1)?.t1Ms ?? 0)
    .toBeGreaterThan(frozen?.t1Ms ?? Infinity);
});

test("AC-DDDYDJ.2: a recording keeps running while the chart is paused", async ({ page }) => {
  await open(page);
  await streamDemo(page);
  await page.getByRole("button", { name: "Record", exact: true }).click();
  await expect(page.getByRole("timer")).toBeVisible();
  await page.locator(".chart-wrap button.live").click();
  await expect(page.locator(".chart-wrap button.live")).toHaveText("Paused · Go live");
  await expect(page.getByRole("button", { name: "Stop", exact: true })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
});

test("AC-DDDYDJ.3/.5: a wheel zoom pauses, and End jumps back to live", async ({ page }) => {
  await open(page);
  await streamDemo(page);
  await expect.poll(async () => (await chartSeriesLengths(page)).vbus).toBeGreaterThan(10);
  await expect
    .poll(async () => (await chartDraws(page)).filter((d) => d.mode === "live").length)
    .toBeGreaterThan(0);
  const plot = page.getByRole("application");
  await plot.focus();
  const box = await page.locator(".plot .u-over").boundingBox();
  if (!box) throw new Error("no plot area");
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.wheel(0, -200);
  const live = page.locator(".chart-wrap button.live");
  await expect(live).toHaveText("Paused · Go live");
  await plot.focus();
  await page.keyboard.press("End");
  await expect(live).toHaveText("Live");
  // End also works with the focus elsewhere.
  await live.click();
  await expect(live).toHaveText("Paused · Go live");
  await blur(page);
  await page.keyboard.press("End");
  await expect(live).toHaveText("Live");
});

test("AC-DDDYDJ.2: the Live button waits for data before it can pause anything", async ({
  page,
}) => {
  await open(page);
  const live = page.locator(".chart-wrap button.live");
  await expect(live).toBeDisabled();
  await streamDemo(page);
  await expect(live).toBeEnabled();
});

test("AC-DDDYDJ.4: the paused button stands out in the accent colour", async ({ page }) => {
  await open(page);
  await streamDemo(page);
  const live = page.locator(".chart-wrap button.live");
  await expect(live).toBeEnabled();
  const accent = await page.evaluate(() =>
    getComputedStyle(document.documentElement).getPropertyValue("--accent").trim(),
  );
  const rgb = await page.evaluate((hex) => {
    const el = document.createElement("span");
    el.style.color = hex;
    document.body.append(el);
    const c = getComputedStyle(el).color;
    el.remove();
    return c;
  }, accent);
  await live.click();
  await expect(live).toHaveText("Paused · Go live");
  await expect(live).toHaveCSS("border-top-color", rgb);
  await expect(live).toHaveCSS("color", rgb);
});
