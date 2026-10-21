/**
 * The real desktop app across a webview reload (FR VEDDFD): the Rust backend
 * keeps running while the page reloads, so this is where the event channel of
 * the old page going away is exercised for real (the Playwright suite can only
 * remount the UI over the in-page mock).
 *
 * - AC-VEDDFD.2: after a reload while streaming, the app shows the running
 *   session without a reconnect or an "already connected" error;
 * - AC-VEDDFD.4: the chart and the stats elapsed time carry on from before;
 * - AC-VEDDFD.5: the new page keeps receiving measurements, and Disconnect
 *   works on the re-attached session.
 */

import { browser, $, $$, expect } from "@wdio/globals";
import { HOOK, type OpenPowerzE2E } from "../../hook";

const RATE_SPS = 50;

async function statsElapsed(): Promise<number> {
  const text = await $(
    '//*[@aria-label="Statistics"]//dt[normalize-space()="Elapsed"]/following-sibling::dd[1]',
  ).getText();
  const [h, m, s] = text.trim().split(":").map(Number);
  return (h ?? NaN) * 3600 + (m ?? NaN) * 60 + (s ?? NaN);
}

async function seriesLengths(): Promise<{ vbus: number; ibus: number }> {
  return browser.execute((name: string) => {
    const h = (window as unknown as Record<string, OpenPowerzE2E | undefined>)[name];
    if (!h) throw new Error(`window.${name} is missing: not an e2e build`);
    return h.chartSeriesLengths();
  }, HOOK);
}

/** The visible button labelled `label`: the top bar can also hold a copy in its "⋯" menu. */
async function button(label: string) {
  const all = await $$("button");
  for (const el of all) {
    if ((await el.getText()).trim() === label && (await el.isDisplayed())) return el;
  }
  throw new Error(`no visible "${label}" button`);
}

describe("open-powerz across a webview reload", () => {
  it("re-attaches to the running session after a reload (AC-VEDDFD.2, AC-VEDDFD.4, AC-VEDDFD.5)", async () => {
    const picker = await $('select[aria-label="Device"]');
    await picker.waitForEnabled({ timeout: 20_000 });
    await picker.selectByVisibleText("Demo device — load-5v-1a");
    await $('select[aria-label="Sample rate"]').selectByAttribute("value", String(RATE_SPS));
    await (await button("Connect")).click();
    await expect($(".badge")).toHaveText("Streaming", { wait: 10_000 });
    await browser.waitUntil(async () => (await statsElapsed()) >= 2, {
      timeout: 10_000,
      timeoutMsg: "the stats elapsed time never reached 2 s",
    });
    const before = await statsElapsed();

    await browser.refresh();

    // The session is still up: no reconnect, no "already connected" error.
    await expect($(".badge")).toHaveText("Streaming", { wait: 10_000 });
    await expect($('[role="alert"]')).not.toBeExisting();
    await expect($('select[aria-label="Sample rate"]')).toHaveValue(String(RATE_SPS));

    // The statistics carry on from before the reload, and measurements keep arriving.
    await browser.waitUntil(async () => (await statsElapsed()) > before, {
      timeout: 10_000,
      timeoutMsg: "the stats elapsed time did not carry on after the reload",
    });
    await browser.waitUntil(
      async () => {
        const n = await seriesLengths();
        return n.vbus > 0 && n.ibus > 0;
      },
      { timeout: 10_000, timeoutMsg: "the reloaded chart drew no VBUS / IBUS points" },
    );
    const first = (await seriesLengths()).vbus;
    await browser.waitUntil(async () => (await seriesLengths()).vbus > first, {
      timeout: 5_000,
      timeoutMsg: "the reloaded page stopped receiving measurements",
    });

    await (await button("Disconnect")).click();
    await expect($(".badge")).toHaveText("Disconnected", { wait: 5_000 });
  });
});
