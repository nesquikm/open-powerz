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

test("the device menu ends with Connect a meter…, with no button beside it (AC-AYR4W6.1, .2, .6)", async ({
  page,
}) => {
  await openSite(page);
  const picker = page.getByLabel("Device", { exact: true });
  await expect(picker.locator("option").last()).toHaveText("Connect a meter…");
  await expect(page.getByRole("button", { name: "Connect a meter…", exact: true })).toHaveCount(0);
});

test("the demo meter's settings and offline records show in the browser (AC-W8SHX8.1, .2, .4, .7)", async ({
  page,
}) => {
  await openSite(page);
  await streamDemo(page);

  const settings = page.getByRole("region", { name: "Meter settings" });
  await expect(settings).toBeVisible();
  // The read is on; only the edit (a meter write) is still left to the desktop app.
  await expect(settings.getByRole("button", { name: "Refresh", exact: true })).toBeVisible();
  await expect(settings.getByText("Brightness", { exact: true })).toBeVisible({ timeout: 10_000 });

  await page.getByRole("tab", { name: "Records" }).click();
  const records = page.getByRole("region", { name: "Offline records" });
  // Listing and download are on; only the delete (a meter write) is still left to the desktop app.
  await expect(records.getByRole("button", { name: "Download all", exact: true })).toBeEnabled();
  await expect(records.getByText("PHONE_5V")).toBeVisible({ timeout: 10_000 });
  await records.getByRole("button", { name: "Open PHONE_5V" }).click();
  await expect(page.getByRole("region", { name: "Offline record" })).toContainText("PHONE_5V");
});

test("a settings change on the demo meter reads back in the browser (AC-W8SHX9.1, .7)", async ({
  page,
}) => {
  await openSite(page);
  await streamDemo(page);
  const settings = page.getByRole("region", { name: "Meter settings" });
  await expect(settings.getByText("Brightness", { exact: true })).toBeVisible({ timeout: 10_000 });
  await settings.getByRole("button", { name: "Edit" }).click();
  const brightness = settings.getByLabel("Brightness", { exact: true });
  const before = await brightness.inputValue();
  const next = before === "80" ? "60" : "80";
  await brightness.fill(next);
  await settings.getByRole("button", { name: "Apply" }).click();
  await expect(settings.locator("input, select")).toHaveCount(0, { timeout: 10_000 });
  await expect(settings).toContainText(`${next}%`);
});

test("Remove on the demo meter's startup logo verifies in the browser (AC-W8SHXB.3, .7)", async ({
  page,
}) => {
  await openSite(page);
  await streamDemo(page);
  await page.getByRole("tab", { name: "Startup logo" }).click();
  const v = page.getByRole("tabpanel", { name: "Startup logo" });
  await expect(v).not.toContainText("Desktop app only");
  await v.getByRole("button", { name: /^remove$/i }).click();
  const d = page.getByRole("alertdialog", { name: "remove" });
  await d.getByRole("textbox").fill("remove");
  await d.getByRole("button", { name: /^remove$/i }).click();
  await expect(v.getByRole("status")).toContainText(/verified|removed/i, { timeout: 30_000 });
});

test("Detect in the Trigger tab runs on the demo meter in the browser (AC-W8SHXC.1, .7)", async ({
  page,
}) => {
  await openSite(page);
  await streamDemo(page);
  await page.getByRole("tab", { name: "Trigger" }).click();
  const view = page.getByRole("tabpanel", { name: "Trigger" });
  await expect(view).not.toContainText("Desktop app only");
  await view.getByRole("button", { name: "Detect", exact: true }).click();
  await expect(view.getByRole("region", { name: "Detection" })).toBeVisible({ timeout: 30_000 });
});
