/**
 * FR CW66T1 on the mocked backend: (i) buttons and dotted-underline terms open short
 * popovers from one glossary (`src/lib/glossary.ts`).
 *
 * - AC .2: an (i) button beside each listed section's title, its popover says what the
 *   section does; the firmware, trigger, console and settings ones state their risk; the
 *   ruler, segment marks, E.D. and sub-chart ones sit beside their own controls in the
 *   Chart options panel, and the chart toolbar keeps only the chart's own (i);
 * - AC .3: abbreviations are dotted-underline terms that open the same popover;
 * - AC .4: click, Enter and Space open; Escape, an outside click and a second press
 *   close; focus returns to the trigger; that Escape closes nothing else;
 * - AC .5: only one popover is open at a time;
 * - AC .6: aria-expanded, aria-controls, role=dialog with a name, axe-clean; hover shows
 *   only the native title;
 * - AC .7: inside the window at 375 and 1280 px, readable in both themes, off its trigger;
 * - AC .8: the "Show (i) icons" switch beside "Hide serial numbers", on by default,
 *   remembered over a reload; off hides the (i) buttons and keeps the terms.
 *
 * Triggers carry `data-glossary-key="<key>"`; the popover is the element the trigger's
 * `aria-controls` names.
 */

import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Locator, type Page } from "@playwright/test";
import { blur, open, streamDemo, useTheme, type Theme } from "./helpers";

const ADVANCED_KEY = "open-powerz.advanced-firmware";
const SHOW_KEY = "open-powerz.show-info-icons";
const SWITCH = "Show (i) icons";

type Entry = { term: string; short: string; long?: string; source: string };
/** A glossary entry, loaded on first use (the module the popovers read their text from). */
async function entry(key: string): Promise<Entry> {
  const { GLOSSARY } = (await import("../../src/lib/glossary")) as {
    GLOSSARY: Record<string, Entry>;
  };
  const e = GLOSSARY[key];
  if (!e) throw new Error(`no glossary entry ${key}`);
  return e;
}

/** The (i) button of a section. */
function info(scope: Page | Locator, key: string): Locator {
  return scope.locator(`button[data-glossary-key="${key}"]`).first();
}

/** A dotted-underline term trigger. */
function term(scope: Page | Locator, key: string): Locator {
  return scope.locator(`[data-glossary-key="${key}"]`).first();
}

/** The popover a trigger controls. */
async function popoverOf(page: Page, trigger: Locator): Promise<Locator> {
  const id = await trigger.getAttribute("aria-controls");
  expect(id, "the trigger points at its popover").toBeTruthy();
  return page.locator(`[id="${id}"]`);
}

async function useAdvancedFirmware(page: Page): Promise<void> {
  await page.addInitScript((key) => localStorage.setItem(key, "true"), ADVANCED_KEY);
}

async function showTab(page: Page, name: string, panel: string): Promise<void> {
  await page.getByRole("tab", { name }).click();
  await expect(page.locator(panel)).toBeVisible();
}

async function detect(page: Page): Promise<void> {
  await showTab(page, "Trigger", "#view-trigger");
  await page.locator("#view-trigger").getByRole("button", { name: "Detect", exact: true }).click();
  await expect(
    page.locator("#view-trigger").getByRole("region", { name: "Console", exact: true }),
  ).toBeVisible();
}

/** The live chart's region. */
function liveChart(page: Page): Locator {
  return page.getByRole("region", { name: "Chart", exact: true });
}

/** The live chart's toolbar (the row that holds "Chart options"). */
function chartToolbar(page: Page): Locator {
  return liveChart(page).locator(".chart-wrap > .toolbar").first();
}

/** Opens the live chart's "Chart options" panel and returns it. */
async function chartOptionsPanel(page: Page): Promise<Locator> {
  const toggle = liveChart(page).getByRole("button", { name: "Chart options", exact: true });
  if ((await toggle.getAttribute("aria-expanded")) !== "true") await toggle.click();
  await expect(toggle).toHaveAttribute("aria-expanded", "true");
  const panel = liveChart(page).getByRole("group", { name: "Chart options", exact: true });
  await expect(panel).toBeVisible();
  return panel;
}

async function openChartOptions(page: Page): Promise<void> {
  await chartOptionsPanel(page);
}

/** The control in the Chart options panel that turns each extra on (AC .2 amendment). */
const EXTRA_CONTROLS: { key: string; control: (panel: Locator) => Locator }[] = [
  {
    key: "section.ruler",
    control: (p) => p.getByRole("button", { name: "Ruler", exact: true }),
  },
  {
    key: "section.segmentMarks",
    control: (p) => p.getByRole("combobox", { name: "Segment marks", exact: true }),
  },
  { key: "section.ed", control: (p) => p.getByRole("button", { name: "E.D.", exact: true }) },
  {
    key: "section.subChart",
    control: (p) => p.getByRole("button", { name: "Sub-chart", exact: true }),
  },
];

/** Where each section of AC .2 is found, and the risk its popover must state. */
const SECTIONS: {
  key: string;
  reach: (page: Page) => Promise<void>;
  risk?: RegExp[];
}[] = [
  { key: "section.meter", reach: async () => {} },
  { key: "section.stats", reach: async () => {} },
  { key: "section.chart", reach: async () => {} },
  { key: "section.ruler", reach: openChartOptions },
  { key: "section.segmentMarks", reach: openChartOptions },
  { key: "section.ed", reach: openChartOptions },
  { key: "section.subChart", reach: openChartOptions },
  {
    key: "section.settings",
    reach: async () => {},
    risk: [/writ/i, /meter/i],
  },
  { key: "section.pd", reach: (p) => showTab(p, "PD analyzer", "#view-pd") },
  { key: "section.records", reach: (p) => showTab(p, "Records", "#view-records") },
  {
    key: "section.trigger",
    reach: (p) => showTab(p, "Trigger", "#view-trigger"),
    risk: [/(above|over|beyond|higher than|more than)\s+5\s?V/i],
  },
  { key: "section.cable", reach: detect },
  { key: "section.console", reach: detect, risk: [/raw/i, /message/i] },
  {
    key: "section.firmware",
    reach: (p) => showTab(p, "Firmware", "#view-firmware"),
    risk: [/firmware/i, /rewrit|overwrit|replac|erase/i],
  },
  {
    key: "section.recorder",
    reach: async (p) => {
      await p.getByRole("button", { name: "Recorder options", exact: true }).click();
      await expect(p.getByRole("dialog", { name: "Recorder options" })).toBeVisible();
    },
  },
];

test.describe("AC-CW66T1.2: an (i) button beside each section's title", () => {
  test.use({ viewport: { width: 1600, height: 900 } });

  for (const s of SECTIONS) {
    test(`${s.key}: its popover says what the section does`, async ({ page }) => {
      await useAdvancedFirmware(page);
      await open(page);
      await streamDemo(page);
      await s.reach(page);
      const button = info(page, s.key);
      await expect(button).toBeVisible();
      await expect(button).toHaveAccessibleName(new RegExp(`^About `));
      await button.click();
      const pop = await popoverOf(page, button);
      await expect(pop).toBeVisible();
      await expect(pop).toContainText((await entry(s.key)).short);
      for (const r of s.risk ?? []) await expect(pop).toContainText(r);
    });
  }

  for (const x of EXTRA_CONTROLS) {
    test(`${x.key}: its (i) sits beside its own control in the Chart options panel`, async ({
      page,
    }) => {
      await open(page);
      await streamDemo(page);
      const panel = await chartOptionsPanel(page);
      const button = info(panel, x.key);
      await expect(button, "the (i) is inside the Chart options panel").toBeVisible();
      const control = x.control(panel);
      await expect(control).toBeVisible();
      const handle = await button.elementHandle();
      expect(handle).not.toBeNull();
      // The nearest wrapper the (i) shares with its control holds no other control and no
      // other (i): it is that control's own row or label.
      const shared = await control.evaluate((ctrl, i) => {
        const btn = i as Element;
        let a: Element | null = ctrl.parentElement;
        while (a && !a.contains(btn)) a = a.parentElement;
        if (!a) return { found: false, controls: -1, infos: -1, inPanel: false };
        const controls = [...a.querySelectorAll("button, select, input")].filter(
          (el) => el !== btn && !el.hasAttribute("data-glossary-key"),
        ).length;
        const infos = a.querySelectorAll('button[data-glossary-key^="section."]').length;
        const inPanel = !!a.closest('[role="group"][aria-label="Chart options"]');
        return { found: true, controls, infos, inPanel };
      }, handle);
      expect(shared.found, "the (i) and its control share a wrapper").toBe(true);
      expect(shared.inPanel, "that wrapper is inside the Chart options panel").toBe(true);
      expect(shared.controls, "that wrapper holds only the control it explains").toBe(1);
      expect(shared.infos, "that wrapper holds only this (i)").toBe(1);
      // Beside: the (i) is on the same row as its control and close to it.
      const cb = await control.boundingBox();
      const bb = await button.boundingBox();
      expect(cb && bb).toBeTruthy();
      if (cb && bb) {
        const cy = cb.y + cb.height / 2;
        expect(bb.y <= cy && cy <= bb.y + bb.height + 4, "same row as its control").toBe(true);
        const gap = Math.max(bb.x - (cb.x + cb.width), cb.x - (bb.x + bb.width));
        expect(gap, "right next to its control").toBeLessThan(48);
      }
    });
  }

  test("the chart toolbar holds only the chart's own (i)", async ({ page }) => {
    await open(page);
    await streamDemo(page);
    const toolbar = chartToolbar(page);
    await expect(toolbar).toBeVisible();
    const keys = await toolbar
      .locator('button[data-glossary-key^="section."]')
      .evaluateAll((els) => els.map((el) => el.getAttribute("data-glossary-key")));
    expect(keys).toEqual(["section.chart"]);
    await chartOptionsPanel(page);
    const after = await toolbar
      .locator('button[data-glossary-key^="section."]')
      .evaluateAll((els) => els.map((el) => el.getAttribute("data-glossary-key")));
    expect(after).toEqual(["section.chart"]);
  });
});

test.describe("AC-CW66T1.3: abbreviations are dotted-underline terms", () => {
  test.use({ viewport: { width: 1600, height: 900 } });

  test("VBUS and IBUS in the readout open their popovers", async ({ page }) => {
    await open(page);
    await streamDemo(page);
    for (const key of ["term.vbus", "term.ibus"]) {
      const t = term(page.getByRole("region", { name: "Live readout" }), key);
      await expect(t).toBeVisible();
      await t.click();
      const pop = await popoverOf(page, t);
      await expect(pop).toBeVisible();
      await expect(pop).toContainText((await entry(key)).short);
      await page.keyboard.press("Escape");
      await expect(pop).toBeHidden();
    }
  });

  test("every term on the meter view is drawn with a dotted underline", async ({ page }) => {
    await open(page);
    await streamDemo(page);
    const styles = await page
      .locator('#view-meter [data-glossary-key^="term."]')
      .evaluateAll((els) =>
        els.map((el) => {
          const s = getComputedStyle(el);
          return {
            key: el.getAttribute("data-glossary-key"),
            dotted: s.textDecorationStyle === "dotted" || s.borderBottomStyle === "dotted",
            underlined: s.textDecorationLine.includes("underline") || s.borderBottomWidth !== "0px",
          };
        }),
      );
    expect(styles.length).toBeGreaterThan(1);
    for (const s of styles) {
      expect(s.dotted && s.underlined, `${s.key} has a dotted underline`).toBe(true);
    }
  });
});

test.describe("AC-CW66T1.4: open and close from mouse and keyboard", () => {
  test.use({ viewport: { width: 1600, height: 900 } });

  test("click opens; a second press closes", async ({ page }) => {
    await open(page);
    const b = info(page, "section.chart");
    await b.click();
    const pop = await popoverOf(page, b);
    await expect(pop).toBeVisible();
    await expect(b).toHaveAttribute("aria-expanded", "true");
    await b.click();
    await expect(pop).toBeHidden();
    await expect(b).toHaveAttribute("aria-expanded", "false");
  });

  test("Enter opens, Escape closes and focus returns to the trigger", async ({ page }) => {
    await open(page);
    const b = info(page, "section.chart");
    await b.focus();
    await page.keyboard.press("Enter");
    const pop = await popoverOf(page, b);
    await expect(pop).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(pop).toBeHidden();
    await expect(b).toBeFocused();
  });

  test("Space opens; an outside click closes and focus returns", async ({ page }) => {
    await open(page);
    const b = info(page, "section.stats");
    await b.focus();
    await page.keyboard.press(" ");
    const pop = await popoverOf(page, b);
    await expect(pop).toBeVisible();
    await page.getByRole("heading", { name: "Stats", exact: true }).click();
    await expect(pop).toBeHidden();
    await expect(b).toBeFocused();
  });

  test("the term trigger opens on Enter and Space too", async ({ page }) => {
    await open(page);
    await streamDemo(page);
    const t = term(page.getByRole("region", { name: "Live readout" }), "term.vbus");
    await t.focus();
    await page.keyboard.press("Enter");
    const pop = await popoverOf(page, t);
    await expect(pop).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(pop).toBeHidden();
    await expect(t).toBeFocused();
    await page.keyboard.press(" ");
    await expect(pop).toBeVisible();
  });

  test("Escape that closes a popover leaves the dialog under it open", async ({ page }) => {
    await open(page);
    await streamDemo(page);
    await page.getByRole("button", { name: "Recorder options", exact: true }).click();
    const dialog = page.getByRole("dialog", { name: "Recorder options" });
    await expect(dialog).toBeVisible();
    const b = info(dialog, "section.recorder");
    await b.click();
    const pop = await popoverOf(page, b);
    await expect(pop).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(pop).toBeHidden();
    await expect(dialog).toBeVisible();
    await expect(b).toBeFocused();
  });

  test("Escape that closes a popover leaves the shortcut list open", async ({ page }) => {
    await open(page);
    await blur(page);
    await page.keyboard.press("?");
    const overlay = page.locator("#shortcut-help");
    await expect(overlay).toBeVisible();
    const b = info(page, "section.chart");
    await b.focus();
    await page.keyboard.press("Enter");
    const pop = await popoverOf(page, b);
    await expect(pop).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(pop).toBeHidden();
    await expect(overlay).toBeVisible();
  });
});

test.describe("AC-CW66T1.5: one popover at a time", () => {
  test.use({ viewport: { width: 1600, height: 900 } });

  test("opening another closes the first", async ({ page }) => {
    await open(page);
    const first = info(page, "section.chart");
    const second = info(page, "section.stats");
    await first.click();
    const p1 = await popoverOf(page, first);
    await expect(p1).toBeVisible();
    await second.click();
    const p2 = await popoverOf(page, second);
    await expect(p2).toBeVisible();
    await expect(p1).toBeHidden();
    await expect(first).toHaveAttribute("aria-expanded", "false");
    await expect(page.locator('[role="dialog"]:visible')).toHaveCount(1);
  });
});

test.describe("AC-CW66T1.6: screen readers and hover", () => {
  test.use({ viewport: { width: 1600, height: 900 } });

  test("the trigger points at a named dialog, and axe finds nothing", async ({ page }) => {
    await open(page);
    await streamDemo(page);
    const b = info(page, "section.chart");
    await expect(b).toHaveAttribute("aria-expanded", "false");
    await b.click();
    await expect(b).toHaveAttribute("aria-expanded", "true");
    const pop = await popoverOf(page, b);
    await expect(pop).toHaveAttribute("role", "dialog");
    await expect(pop).toHaveAccessibleName(/\S/);
    const id = await b.getAttribute("aria-controls");
    const scoped = await new AxeBuilder({ page }).include(`[id="${id}"]`).analyze();
    expect(scoped.violations.map((v) => `${v.impact} ${v.id}: ${v.help}`)).toEqual([]);
    const page_ = await new AxeBuilder({ page })
      .withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa", "best-practice"])
      .analyze();
    expect(
      page_.violations
        .filter((v) => v.impact === "serious" || v.impact === "critical")
        .map((v) => `${v.impact} ${v.id}: ${v.help}`),
    ).toEqual([]);
  });

  test("hover gives only the native title with the short line", async ({ page }) => {
    await open(page);
    await streamDemo(page);
    const b = info(page, "section.chart");
    await expect(b).toHaveAttribute("title", (await entry("section.chart")).short);
    const t = term(page.getByRole("region", { name: "Live readout" }), "term.vbus");
    await expect(t).toHaveAttribute("title", (await entry("term.vbus")).short);
    await b.hover();
    await t.hover();
    await page.waitForTimeout(600);
    await expect(b).toHaveAttribute("aria-expanded", "false");
    await expect(t).toHaveAttribute("aria-expanded", "false");
    await expect(await popoverOf(page, b)).toBeHidden();
    await expect(await popoverOf(page, t)).toBeHidden();
  });
});

function luminance([r, g, b]: number[]): number {
  const [R, G, B] = [r, g, b].map((c) => {
    const s = (c ?? 0) / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * (R ?? 0) + 0.7152 * (G ?? 0) + 0.0722 * (B ?? 0);
}

/** The popover's text colour and the first opaque background behind it. */
async function colours(pop: Locator): Promise<{ fg: number[]; bg: number[] }> {
  return pop.evaluate((el) => {
    const rgb = (s: string) => (s.match(/[\d.]+/g) ?? []).map(Number);
    const fg = rgb(getComputedStyle(el).color);
    let node: Element | null = el;
    let bg = [255, 255, 255];
    while (node) {
      const c = rgb(getComputedStyle(node).backgroundColor);
      if (c.length >= 3 && (c.length === 3 || (c[3] ?? 1) > 0.9)) {
        bg = c.slice(0, 3);
        break;
      }
      node = node.parentElement;
    }
    return { fg: fg.slice(0, 3), bg };
  });
}

test.describe("AC-CW66T1.7: inside the window, readable, beside its trigger", () => {
  const THEMES: readonly Theme[] = ["light", "dark"];
  for (const width of [375, 1280] as const) {
    for (const theme of THEMES) {
      test(`${width} px, ${theme}`, async ({ page }) => {
        await page.setViewportSize({ width, height: 800 });
        await useTheme(page, theme);
        await open(page);
        await streamDemo(page);
        await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
        for (const key of ["section.meter", "section.chart", "section.stats"]) {
          const b = info(page, key);
          await b.scrollIntoViewIfNeeded();
          await b.click();
          const pop = await popoverOf(page, b);
          await expect(pop).toBeVisible();
          const box = (await pop.boundingBox())!;
          const tb = (await b.boundingBox())!;
          expect(box.x, `${key} left edge`).toBeGreaterThanOrEqual(0);
          expect(box.y, `${key} top edge`).toBeGreaterThanOrEqual(0);
          expect(box.x + box.width, `${key} right edge`).toBeLessThanOrEqual(width);
          expect(box.y + box.height, `${key} bottom edge`).toBeLessThanOrEqual(800);
          const overlap =
            box.x < tb.x + tb.width &&
            tb.x < box.x + box.width &&
            box.y < tb.y + tb.height &&
            tb.y < box.y + box.height;
          expect(overlap, `${key} popover does not cover its trigger`).toBe(false);
          const { fg, bg } = await colours(pop);
          const [hi, lo] = [luminance(fg), luminance(bg)].sort((a, b) => b - a);
          expect((hi! + 0.05) / (lo! + 0.05), `${key} text contrast`).toBeGreaterThanOrEqual(4.5);
          await page.keyboard.press("Escape");
          await expect(pop).toBeHidden();
        }
      });
    }
  }
});

test.describe("AC-CW66T1.8: the Show (i) icons switch", () => {
  test.use({ viewport: { width: 1600, height: 900 } });

  async function openAbout(page: Page): Promise<Locator> {
    await page.locator("#top-bar-about").click();
    const dialog = page.getByRole("dialog", { name: "About" });
    await expect(dialog).toBeVisible();
    return dialog;
  }

  test("sits beside Hide serial numbers and is on by default", async ({ page }) => {
    await open(page);
    const about = await openAbout(page);
    const show = about.getByRole("switch", { name: SWITCH });
    const hide = about.getByRole("switch", { name: "Hide serial numbers" });
    await expect(show).toHaveAttribute("aria-checked", "true");
    const sameGroup = await show.evaluate(
      (el, hideEl) => el.parentElement === (hideEl as Element).parentElement,
      await hide.elementHandle(),
    );
    expect(sameGroup).toBe(true);
    await expect(info(page, "section.chart")).toBeVisible();
  });

  test("off hides every (i) button, keeps the terms, and survives a reload", async ({ page }) => {
    await open(page);
    await streamDemo(page);
    const about = await openAbout(page);
    await about.getByRole("switch", { name: SWITCH }).click();
    await expect(about.getByRole("switch", { name: SWITCH })).toHaveAttribute(
      "aria-checked",
      "false",
    );
    await page.keyboard.press("Escape");
    await expect(page.locator('button[data-glossary-key^="section."]:visible')).toHaveCount(0);
    await expect(
      term(page.getByRole("region", { name: "Live readout" }), "term.vbus"),
    ).toBeVisible();
    expect(await page.evaluate((k) => localStorage.getItem(k), SHOW_KEY)).toBe("false");

    await page.reload();
    await expect(page.getByLabel("Device", { exact: true })).toBeEnabled();
    await expect(page.locator('button[data-glossary-key^="section."]:visible')).toHaveCount(0);
    const again = await openAbout(page);
    await expect(again.getByRole("switch", { name: SWITCH })).toHaveAttribute(
      "aria-checked",
      "false",
    );
  });
});
