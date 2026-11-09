/**
 * FR QFTCX1 on WebKit (no WebUSB, no WebHID, as Safari): the built Pages site shows the
 * "not supported" view naming Chrome and Edge, links the desktop downloads, and still
 * offers the demo meter (AC-QFTCX1.3, AC-QFTCX1.4). No connect hint shows beside the
 * device menu there, since connecting cannot work (AC-09R2FM.3).
 */

import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";
import { RELEASES_URL } from "../../src/lib/web/features";
import { badge, DEMO_LOAD } from "../ui/helpers";

test("WebKit sees the not-supported view, no connect hint, and can still run the demo meter (AC-QFTCX1.3, .4, AC-09R2FM.3)", async ({
  page,
}) => {
  await page.goto("/open-powerz/");
  expect(await page.evaluate(() => "usb" in navigator || "hid" in navigator)).toBe(false);

  const heading = page.getByRole("heading", { name: /not supported/i });
  await expect(heading).toBeVisible();
  await expect(page.getByText(/\bChrome\b/).first()).toBeVisible();
  await expect(page.getByText(/\bEdge\b/).first()).toBeVisible();
  await expect(page.locator(`a[href="${RELEASES_URL}"]`).first()).toBeVisible();

  // The demo meter is still offered and streams.
  const picker = page.getByLabel("Device", { exact: true });
  await expect(picker.locator(`option[value="${DEMO_LOAD}"]`)).toHaveCount(1);
  // Only the demo is listed, yet no connect hint points at a picker that cannot work
  // (AC-09R2FM.3).
  await expect(page.locator(".connect-hint")).toHaveCount(0);
  await expect(page.getByRole("button", { name: /Pick “Connect a meter…”/ })).toHaveCount(0);
  await picker.selectOption(DEMO_LOAD);
  await page.getByRole("button", { name: "Connect", exact: true }).click();
  await expect(badge(page)).toHaveText("Streaming", { timeout: 15_000 });
  await expect(page.locator(".connect-hint")).toHaveCount(0);

  const results = await new AxeBuilder({ page }).withTags(["wcag2a", "wcag2aa"]).analyze();
  expect(
    results.violations
      .filter((v) => v.impact === "serious" || v.impact === "critical")
      .map((v) => `${v.impact} ${v.id}`),
  ).toEqual([]);
});
