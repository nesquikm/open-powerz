/**
 * Visual-check capture: screenshots of the web build running against the
 * mock backend, in both themes, at a narrow and a wide window, with no
 * device and with the demo device streaming (and at 1000 SPS, where the
 * meter reports no temperature).
 *
 * Usage: start `VITE_BACKEND=mock vite --port 1420`, then
 *   bun e2e/visual/capture.ts <out-dir> [base-url]
 */

import { mkdirSync } from "node:fs";
import { join } from "node:path";
import { chromium, type Page } from "@playwright/test";

const out = process.argv[2] ?? "visual-shots";
const base = process.argv[3] ?? "http://localhost:1420/";
const SIZES = [
  { name: "narrow", width: 900, height: 600 },
  { name: "mid", width: 1280, height: 800 },
  { name: "wide", width: 1600, height: 900 },
] as const;
const THEMES = ["light", "dark"] as const;

async function streamDemo(page: Page, scenario: string): Promise<void> {
  await page.getByLabel("Device", { exact: true }).selectOption(`demo:${scenario}`);
  await page.getByRole("button", { name: "Connect" }).click();
  await page.getByText("Streaming", { exact: false }).first().waitFor();
  await page.waitForTimeout(600);
}

/** Readout values whose text is wider than their cell (they would overlap a neighbour). */
function overflowingValues(page: Page): Promise<string[]> {
  return page.evaluate(() =>
    [...document.querySelectorAll<HTMLElement>(".readout .cell")]
      .filter((cell) => cell.scrollWidth > cell.clientWidth + 1)
      .map((cell) => cell.innerText.replace(/\s+/g, " ").trim()),
  );
}

async function main(): Promise<void> {
  mkdirSync(out, { recursive: true });
  // The installed Google Chrome: no browser download needed.
  const browser = await chromium.launch({ channel: "chrome" });
  const problems: string[] = [];
  for (const theme of THEMES) {
    for (const size of SIZES) {
      const page = await browser.newPage({ viewport: size });
      page.on("pageerror", (e) => problems.push(`${theme}/${size.name}: ${e.message}`));
      page.on("console", (m) => {
        if (m.type() === "error") problems.push(`${theme}/${size.name}: console: ${m.text()}`);
      });
      await page.addInitScript((t) => localStorage.setItem("open-powerz.theme", t), theme);
      await page.goto(base);
      await page.waitForLoadState("networkidle");
      const shot = (label: string) =>
        page.screenshot({ path: join(out, `${theme}-${size.name}-${label}.png`) });
      const hScroll = await page.evaluate(
        () => document.documentElement.scrollWidth > document.documentElement.clientWidth,
      );
      if (hScroll) problems.push(`${theme}/${size.name}: horizontal scroll`);
      await shot("no-device");
      await streamDemo(page, "load-5v-1a");
      await shot("demo-streaming");
      for (const cell of await overflowingValues(page)) {
        problems.push(`${theme}/${size.name}: readout overflows its cell: ${cell}`);
      }
      await page.keyboard.press("4");
      await page.waitForTimeout(600);
      await shot("demo-1000sps");
      // Space on a focused button presses it; record from the page instead.
      await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
      await page.keyboard.press(" ");
      await page.waitForTimeout(1600);
      await shot("demo-recording");
      await page.keyboard.press(" ");
      await page.waitForTimeout(400);
      await shot("demo-recording-stopped");
      if (size.name === "narrow") {
        await page.getByRole("button", { name: "More: theme and About" }).click();
        await page.waitForTimeout(200);
        await shot("menu-open");
        await page.keyboard.press("Escape");
      }
      // Reverse current: the arrow flips.
      await page.getByRole("button", { name: "Disconnect" }).click();
      await page.getByText("Disconnected", { exact: false }).first().waitFor();
      await streamDemo(page, "noisy-negative");
      await shot("demo-negative");
      await page.close();
    }
  }
  await browser.close();
  console.log(problems.length ? problems.join("\n") : "no page errors, no horizontal scroll");
}

await main();
