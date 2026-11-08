/**
 * FR QFTCX1 on Chromium: the built Pages site served at `/open-powerz/`.
 *
 * AC-QFTCX1.1: every asset URL of the page resolves under the base path and the wasm
 * module loads (the demo meter streams through it).
 * AC-QFTCX1.2: the brand favicons, title and description are in the served page.
 * AC-QFTCX1.4: open the page, connect the demo meter, see live readings and the chart,
 * start and stop a recording that downloads a CSV, run a PD capture, pass an axe scan.
 */

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import { CSV_HEADER } from "../../src/lib/web/csv";
import { badge, DEMO_LOAD, readoutValue } from "../ui/helpers";
import { capture, messageTexts, showPd, stopCapture } from "../ui/pd";

const BRAND = fileURLToPath(new URL("../../../../assets/brand/", import.meta.url));
const BASE = "/open-powerz/";

/** Open the site at its base path and wait until the picker lists the demo meter. */
async function openSite(page: Page): Promise<void> {
  await page.goto(BASE);
  const picker = page.getByLabel("Device", { exact: true });
  await expect(picker).toBeEnabled();
  await expect(picker.locator(`option[value="${DEMO_LOAD}"]`)).toHaveCount(1);
}

/** Connect the demo meter and wait until it streams. */
async function streamDemo(page: Page): Promise<void> {
  await page.getByLabel("Device", { exact: true }).selectOption(DEMO_LOAD);
  await page.getByRole("button", { name: "Connect", exact: true }).click();
  await expect(badge(page)).toHaveText("Streaming", { timeout: 15_000 });
}

const chart = (page: Page) => page.getByRole("application", { name: /^Chart\./ });

test("every asset resolves under /open-powerz/ and the wasm module loads (AC-QFTCX1.1)", async ({
  page,
}) => {
  const failed: string[] = [];
  const wasm: number[] = [];
  page.on("response", (r) => {
    if (r.status() >= 400) failed.push(`${r.status()} ${r.url()}`);
    if (new URL(r.url()).pathname.endsWith(".wasm")) wasm.push(r.status());
  });
  page.on("requestfailed", (r) => failed.push(`failed ${r.url()}`));
  page.on("pageerror", (e) => failed.push(`pageerror ${e.message}`));

  await openSite(page);
  await streamDemo(page);
  await expect.poll(() => readoutValue(page, "VBUS")).not.toBeNaN();

  expect(failed).toEqual([]);
  expect(wasm, "the wasm module was fetched").toContain(200);
  // Everything the page loaded came from under the base path.
  const outside = await page.evaluate(
    (base) =>
      performance
        .getEntriesByType("resource")
        .map((e) => new URL(e.name))
        .filter((u) => u.origin === location.origin && !u.pathname.startsWith(base))
        .map((u) => u.pathname),
    BASE,
  );
  expect(outside).toEqual([]);
});

test("the page carries the brand favicons, a title and a description (AC-QFTCX1.2)", async ({
  page,
  request,
}) => {
  await openSite(page);
  await expect(page).toHaveTitle(/open-powerz/i);
  await expect(page.locator('meta[name="description"]')).toHaveAttribute("content", /KM003C/);

  const icons: [selector: string, brand: string][] = [
    ['link[rel="icon"][type="image/svg+xml"]', "favicon.svg"],
    ['link[rel="icon"][type="image/png"][sizes="16x16"]', "favicon-16.png"],
    ['link[rel="icon"][type="image/png"][sizes="32x32"]', "favicon-32.png"],
  ];
  for (const [selector, brand] of icons) {
    const link = page.locator(selector);
    await expect(link, selector).toHaveCount(1);
    const href = await link.evaluate((l) => (l as HTMLLinkElement).href);
    expect(new URL(href).pathname.startsWith(BASE), `${brand} is under ${BASE}`).toBe(true);
    const res = await request.get(href);
    expect(res.status(), href).toBe(200);
    expect(Buffer.from(await res.body()).equals(readFileSync(`${BRAND}${brand}`))).toBe(true);
  }
});

test("the demo meter streams live readings and draws the chart (AC-QFTCX1.4)", async ({ page }) => {
  await openSite(page);
  await expect(page.getByRole("heading", { name: /not supported/i })).toHaveCount(0);
  await streamDemo(page);

  // The 5 V / 1 A demo load.
  await expect.poll(() => readoutValue(page, "VBUS")).toBeGreaterThan(4.5);
  await expect.poll(() => readoutValue(page, "VBUS")).toBeLessThan(5.5);
  await expect.poll(() => readoutValue(page, "IBUS")).toBeGreaterThan(0.5);

  await expect(chart(page)).toBeVisible();
  await expect(chart(page).locator("canvas").first()).toBeVisible();
  // The chart is live: what it shows changes as samples arrive.
  const first = await chart(page).screenshot();
  await expect
    .poll(async () => !(await chart(page).screenshot()).equals(first), { timeout: 10_000 })
    .toBe(true);
});

test("a recording started and stopped downloads a CSV (AC-QFTCX1.4)", async ({ page }) => {
  await openSite(page);
  await streamDemo(page);

  await page.getByRole("button", { name: "Record", exact: true }).click();
  await expect(page.getByRole("timer")).toContainText(/[1-9][\d,]* rows?/);

  const download = page.waitForEvent("download");
  await page.getByRole("button", { name: "Stop", exact: true }).click();
  const file = await download;

  expect(file.suggestedFilename()).toMatch(/\.csv$/);
  const path = await file.path();
  const text = readFileSync(path, "utf8");
  const lines = text.trim().split(/\r?\n/);
  expect(lines[0]).toBe(CSV_HEADER);
  expect(lines.length).toBeGreaterThan(1);
  await expect(page.getByRole("button", { name: "Record", exact: true })).toBeVisible();
});

test("a PD capture on the demo meter lists the source's capabilities (AC-QFTCX1.4)", async ({
  page,
}) => {
  await openSite(page);
  await streamDemo(page);
  await showPd(page);
  await capture(page);
  await stopCapture(page);
  expect(await messageTexts(page)).toContain("Source_Capabilities");
});

test("the streaming page passes an axe scan (AC-QFTCX1.4)", async ({ page }) => {
  await openSite(page);
  await streamDemo(page);
  await expect.poll(() => readoutValue(page, "VBUS")).not.toBeNaN();
  const results = await new AxeBuilder({ page })
    .withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa", "best-practice"])
    .analyze();
  const blocking = results.violations.filter(
    (v) => v.impact === "serious" || v.impact === "critical",
  );
  expect(
    blocking.map(
      (v) =>
        `${v.impact} ${v.id}: ${v.help} at ${v.nodes.map((n) => n.target.join(" ")).join("; ")}`,
    ),
    "no serious or critical axe violations",
  ).toEqual([]);
});
