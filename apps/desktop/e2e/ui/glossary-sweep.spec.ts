/**
 * FR 62VEDM, AC .4: a sweep of the views on the mock build. On each view every visible
 * text node is matched against the spellings of each glossary term entry; a match outside
 * a glossary term (and outside controls and table data cells) is a problem when its
 * section holds no wrapped copy of that term. Text the FR Notes keep plain on purpose
 * (backend hint text) opts out with a `data-glossary-plain` attribute on its element.
 */

import { expect, test, type Page } from "@playwright/test";
import { GLOSSARY } from "../../src/lib/glossary";
import { open, streamDemo } from "./helpers";
import { capture, dataRows, showPd } from "./pd";

test.use({ viewport: { width: 1600, height: 1000 } });

interface TermSpellings {
  key: string;
  spellings: string[];
}

/** Each term.* entry with the spellings its term carries. */
const TERMS: TermSpellings[] = Object.entries(GLOSSARY)
  .filter(([key]) => key.startsWith("term."))
  .map(([key, e]) => ({
    key,
    spellings: e.term.split(/[/,]|\s+/).filter((s) => s !== ""),
  }));

/**
 * Problems on the page under `root`: "<view>: <key> plain in <section>: <text>".
 * Runs in the browser.
 */
function scanPage(
  page: Page,
  view: string,
  root = "body",
  terms: TermSpellings[] = TERMS,
): Promise<string[]> {
  return page.evaluate(
    ({ view, root, terms }) => {
      const SKIP_ANCESTOR = [
        "[data-glossary-key]",
        "button",
        '[role="tab"]',
        "select",
        "option",
        "input",
        "label",
        "textarea",
        "td",
        '[role="gridcell"]',
        // Text the FR Notes keep plain on purpose (backend hint text), marked in the markup.
        "[data-glossary-plain]",
      ].join(",");
      const GROUP = 'section, [role="region"], [role="dialog"], [role="tabpanel"]';
      const POPOVER = '.popover[role="dialog"]';

      const esc = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
      const matchers = terms.map((t) => ({
        key: t.key,
        res: t.spellings.map(
          (s) => new RegExp(`(?<![A-Za-z0-9])${esc(s)}(?![A-Za-z0-9])`, /[a-z]/.test(s) ? "i" : ""),
        ),
      }));

      function visible(el: Element): boolean {
        if (el.closest('script, style, [hidden], [aria-hidden="true"]')) return false;
        if (el.closest(POPOVER)) return false;
        const rect = el.getBoundingClientRect();
        if (rect.width === 0 || rect.height === 0) return false;
        const check = (el as Element & { checkVisibility?: (o?: object) => boolean })
          .checkVisibility;
        if (check && !check.call(el, { visibilityProperty: true, opacityProperty: false }))
          return false;
        const style = getComputedStyle(el);
        return style.visibility !== "hidden" && style.display !== "none";
      }

      function groupOf(el: Element): Element {
        let g: Element | null = el.closest(GROUP);
        while (g && g.matches(POPOVER)) g = g.parentElement?.closest(GROUP) ?? null;
        return g ?? document.body;
      }

      function labelOf(g: Element): string {
        const aria = g.getAttribute("aria-label");
        if (aria) return aria;
        const by = g.getAttribute("aria-labelledby");
        if (by) {
          const t = by
            .split(/\s+/)
            .map((id) => document.getElementById(id)?.textContent?.trim() ?? "")
            .join(" ")
            .trim();
          if (t) return t;
        }
        return g.id ? `#${g.id}` : g.tagName.toLowerCase();
      }

      const base = document.querySelector(root);
      if (!base) return [`${view}: no ${root} on the page`];
      const problems = new Set<string>();
      const walker = document.createTreeWalker(base, NodeFilter.SHOW_TEXT);
      for (let n = walker.nextNode(); n; n = walker.nextNode()) {
        const text = n.textContent ?? "";
        if (text.trim() === "") continue;
        const parent = n.parentElement;
        if (!parent || !visible(parent)) continue;
        if (parent.closest(SKIP_ANCESTOR)) continue;
        for (const m of matchers) {
          if (!m.res.some((re) => re.test(text))) continue;
          const group = groupOf(parent);
          const wrapped = [...group.querySelectorAll(`[data-glossary-key="${m.key}"]`)].some(
            (w) => !w.closest(POPOVER),
          );
          if (!wrapped) {
            problems.add(
              `${view}: ${m.key} plain in ${labelOf(group)}: ${text.trim().slice(0, 80)}`,
            );
          }
        }
      }
      return [...problems];
    },
    { view, root, terms },
  );
}

function trigger(page: Page) {
  return page.locator("#view-trigger");
}

test.describe("glossary sweep (FR 62VEDM)", () => {
  test.beforeEach(async ({ page }) => {
    await page.addInitScript(() => {
      try {
        localStorage.setItem("open-powerz.advanced-firmware", "true");
      } catch {
        // A page without storage still runs the sweep.
      }
    });
    await open(page);
  });

  test("the scanner flags a plain term in a section without one, not in a button (AC-62VEDM.4)", async ({
    page,
  }) => {
    await page.evaluate(() => {
      const box = document.createElement("div");
      box.id = "glossary-sweep-synthetic";
      box.innerHTML =
        '<section aria-label="Plain">Ask for VFCP and VBUS here.</section>' +
        '<section aria-label="Buttons"><button type="button">VFCP</button>' +
        '<button type="button">VBUS</button></section>' +
        '<section aria-label="Cells"><table><tr><td>VBUS</td></tr></table></section>';
      document.body.prepend(box);
    });
    const vfcp = TERMS.find((t) => t.spellings.includes("VFCP"));
    expect(vfcp, "VFCP has a glossary entry").toBeDefined();
    const problems = await scanPage(page, "Synthetic", "#glossary-sweep-synthetic");
    expect(problems.sort()).toEqual(
      [
        `Synthetic: ${vfcp?.key} plain in Plain: Ask for VFCP and VBUS here.`,
        "Synthetic: term.vbus plain in Plain: Ask for VFCP and VBUS here.",
      ].sort(),
    );
  });

  test("the scanner accepts a section that holds a wrapped copy (AC-62VEDM.4)", async ({
    page,
  }) => {
    await page.evaluate(() => {
      const box = document.createElement("div");
      box.id = "glossary-sweep-synthetic";
      box.innerHTML =
        '<section aria-label="Wrapped"><h2><button type="button" data-glossary-key="term.vbus">VBUS</button></h2>' +
        "<p>VBUS again in plain text.</p></section>";
      document.body.prepend(box);
    });
    expect(await scanPage(page, "Synthetic", "#glossary-sweep-synthetic")).toEqual([]);
  });

  test("Meter while streaming (AC-62VEDM.3, .4)", async ({ page }) => {
    await streamDemo(page);
    await expect(page.locator("#view-meter")).toBeVisible();
    expect(await scanPage(page, "Meter")).toEqual([]);
  });

  test("PD analyzer with a capture and a row open (AC-62VEDM.3, .4)", async ({ page }) => {
    await streamDemo(page);
    await showPd(page);
    await capture(page);
    await dataRows(page).first().click();
    expect(await scanPage(page, "PD analyzer")).toEqual([]);
  });

  for (const tab of ["Records", "Startup logo", "Firmware"]) {
    test(`${tab} (AC-62VEDM.3, .4)`, async ({ page }) => {
      await streamDemo(page);
      await page.getByRole("tab", { name: tab, exact: true }).click();
      await expect(page.getByRole("tabpanel", { name: tab, exact: true })).toBeVisible();
      expect(await scanPage(page, tab)).toEqual([]);
    });
  }

  test("Trigger after Detect (AC-62VEDM.1, .2, .3, .4)", async ({ page }) => {
    await streamDemo(page);
    await page.getByRole("tab", { name: "Trigger", exact: true }).click();
    const detect = trigger(page).getByRole("button", { name: "Detect", exact: true });
    await detect.click();
    await expect(detect).toBeEnabled({ timeout: 15_000 });
    await expect(trigger(page).getByRole("region", { name: "Console", exact: true })).toBeVisible();
    await expect(trigger(page).getByRole("region", { name: "USB PD", exact: true })).toBeVisible();
    expect(await scanPage(page, "Trigger")).toEqual([]);
  });
});
