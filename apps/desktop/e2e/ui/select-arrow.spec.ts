/**
 * FR K1GZTR: every drop-down draws the app's own chevron with a clear gap to the
 * box's right edge, in Chromium and WebKit, light and dark, and gives the native
 * arrow back in forced-colors mode.
 *
 * The gap is measured on pixels (a screenshot of each select, decoded in the page),
 * so it holds whatever way each engine serialises background positions: the
 * rightmost non-background column inside the right border is the chevron's right end.
 */

import { expect, test, type Locator, type Page } from "@playwright/test";
import { open, useTheme, type Theme } from "./helpers";

/** AC-K1GZTR.2: chevron right end to the box's inner right edge, CSS px. A half pixel of
 * slack each side for anti-aliasing at the chevron's tip. */
const GAP_MIN = 10;
const GAP_MAX = 12;
const AA = 0.5;

/** A column is "ink" when any pixel in it differs this much (max channel) from the
 * box's background. */
const INK_DIFF = 48;

interface Profile {
  /** Border-box width, CSS px. */
  width: number;
  /** The border's first image column from the left on the right side (image px). */
  innerRight: number;
  borderRight: number;
  paddingRight: number;
  devicePixelRatio: number;
  /** Rightmost ink column inside the right border, image px; -1 when none. */
  chevronRight: number;
  /** Leftmost column of that ink run, image px. */
  chevronLeft: number;
  /** Rightmost ink column left of the chevron (the label), image px; -1 when none. */
  textRight: number;
  /** The chevron pixel nearest the text color: its distance to the text color and to
   * the background (max channel). */
  nearestTextDistance: number;
  nearestTextBgDistance: number;
}

/** Visible selects on the page right now. */
async function visibleSelects(page: Page): Promise<Locator[]> {
  const all = await page.locator("select").all();
  const visible: Locator[] = [];
  for (const s of all) if (await s.isVisible()) visible.push(s);
  return visible;
}

/** Keeps hover and focus off the measured select so its border is the resting one. */
async function rest(page: Page): Promise<void> {
  await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
  await page.mouse.move(1, 1);
  // Border-color transition (120 ms) must finish before the screenshot.
  await page.waitForTimeout(200);
}

/** Screenshot `select` and profile its right end in the page (canvas decode). */
async function profile(page: Page, select: Locator): Promise<Profile> {
  await select.scrollIntoViewIfNeeded();
  const png = (await select.screenshot({ animations: "disabled" })).toString("base64");
  const metrics = await select.evaluate((el) => {
    const s = getComputedStyle(el);
    const textRgb = s.color
      .match(/\d+(\.\d+)?/g)
      ?.slice(0, 3)
      .map(Number) ?? [0, 0, 0];
    return {
      width: el.getBoundingClientRect().width,
      borderRight: parseFloat(s.borderRightWidth),
      paddingRight: parseFloat(s.paddingRight),
      devicePixelRatio: window.devicePixelRatio,
      textRgb,
    };
  });
  const scan = await page.evaluate(
    async ({ png, m, inkDiff }) => {
      const img = new Image();
      img.src = `data:image/png;base64,${png}`;
      await img.decode();
      const canvas = document.createElement("canvas");
      canvas.width = img.naturalWidth;
      canvas.height = img.naturalHeight;
      const ctx = canvas.getContext("2d");
      if (!ctx) throw new Error("no 2d context");
      ctx.drawImage(img, 0, 0);
      const { data, width: w, height: h } = ctx.getImageData(0, 0, canvas.width, canvas.height);
      const px = (x: number, y: number) => {
        const i = (y * w + x) * 4;
        return [data[i], data[i + 1], data[i + 2]];
      };
      const diff = (a: number[], b: number[]) =>
        Math.max(Math.abs(a[0] - b[0]), Math.abs(a[1] - b[1]), Math.abs(a[2] - b[2]));
      // Image px per CSS px. Not w / m.width: WebKit snaps an element shot of a
      // fractional-width box outward to whole device pixels (484 px for 241.19 CSS px
      // at 2x), which would put the inner edge past the border.
      const scale = m.devicePixelRatio;
      const innerRight0 = Math.floor((m.width - m.borderRight) * scale);
      const innerLeft = Math.ceil(m.borderRight * scale);
      const top = Math.floor(h * 0.3);
      const bottom = Math.ceil(h * 0.7);
      // Background: the most common color inside the border, in the middle band.
      const counts = new Map<string, number>();
      for (let y = top; y < bottom; y++)
        for (let x = innerLeft; x < Math.min(innerRight0, w); x++) {
          const k = px(x, y).join(",");
          counts.set(k, (counts.get(k) ?? 0) + 1);
        }
      let bg = [255, 255, 255];
      let best = -1;
      for (const [k, n] of counts)
        if (n > best) {
          best = n;
          bg = k.split(",").map(Number);
        }
      const ink = (x: number) => {
        for (let y = top; y < bottom; y++) if (diff(px(x, y), bg) > inkDiff) return true;
        return false;
      };
      // The inner right edge, from the pixels: skip any margin and then the border.
      let x = w - 1;
      while (x >= innerLeft && !ink(x)) x--;
      while (x >= innerLeft && ink(x)) x--;
      const innerRight = x + 1;
      while (x >= innerLeft && !ink(x)) x--;
      const chevronRight = x;
      while (x >= innerLeft && ink(x)) x--;
      const chevronLeft = x + 1;
      while (x >= innerLeft && !ink(x)) x--;
      const textRight = x;
      let nearestTextDistance = 255;
      let nearestTextBgDistance = 0;
      if (chevronRight >= 0)
        for (let cx = chevronLeft; cx <= chevronRight; cx++)
          for (let y = top; y < bottom; y++) {
            const p = px(cx, y);
            const d = diff(p, m.textRgb);
            if (d < nearestTextDistance) {
              nearestTextDistance = d;
              nearestTextBgDistance = diff(p, bg);
            }
          }
      return {
        chevronRight,
        chevronLeft,
        textRight,
        nearestTextDistance,
        nearestTextBgDistance,
        innerRight,
        scale,
      };
    },
    { png, m: metrics, inkDiff: INK_DIFF },
  );
  return {
    width: metrics.width,
    borderRight: metrics.borderRight,
    paddingRight: metrics.paddingRight,
    devicePixelRatio: scan.scale,
    innerRight: scan.innerRight,
    chevronRight: scan.chevronRight,
    chevronLeft: scan.chevronLeft,
    textRight: scan.textRight,
    nearestTextDistance: scan.nearestTextDistance,
    nearestTextBgDistance: scan.nearestTextBgDistance,
  };
}

/** Gap from the chevron's right end to the inner right edge, CSS px. */
function gapOf(p: Profile): number {
  return (p.innerRight - (p.chevronRight + 1)) / p.devicePixelRatio;
}

/** The select's computed arrow style: appearance, gradients and the text color. */
function arrowStyle(select: Locator) {
  return select.evaluate((el) => {
    const s = getComputedStyle(el);
    return {
      appearance: s.appearance || s.getPropertyValue("-webkit-appearance"),
      webkitAppearance: s.getPropertyValue("-webkit-appearance"),
      backgroundImage: s.backgroundImage,
      color: s.color,
      gradients: (s.backgroundImage.match(/linear-gradient\(/g) ?? []).length,
    };
  });
}

async function label(select: Locator): Promise<string> {
  return (await select.getAttribute("aria-label")) ?? (await select.getAttribute("id")) ?? "select";
}

for (const theme of ["light", "dark"] as Theme[]) {
  test(`every drop-down turns off the native arrow and draws the app chevron in the text color, ${theme} (AC-K1GZTR.1, AC-K1GZTR.4)`, async ({
    page,
  }) => {
    await useTheme(page, theme);
    await open(page);
    await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
    const selects = await visibleSelects(page);
    expect(selects.length, "the main page shows drop-downs").toBeGreaterThan(0);
    for (const select of selects) {
      const name = await label(select);
      const style = await arrowStyle(select);
      expect(style.appearance, `${name}: native arrow off`).toBe("none");
      expect(style.webkitAppearance || "none", `${name}: native arrow off (-webkit)`).toBe("none");
      expect(style.gradients, `${name}: chevron as two gradients`).toBeGreaterThanOrEqual(2);
      const uses = style.backgroundImage.split(style.color).length - 1;
      expect(uses, `${name}: chevron in the text color ${style.color}`).toBeGreaterThanOrEqual(2);

      await rest(page);
      const p = await profile(page, select);
      expect(p.chevronRight, `${name}: chevron drawn`).toBeGreaterThan(0);
      // The chevron pixels reach the text color and stand off the background.
      expect(p.nearestTextDistance, `${name}: chevron in the text color`).toBeLessThan(80);
      expect(p.nearestTextBgDistance, `${name}: chevron visible on the box`).toBeGreaterThan(80);
    }
  });

  test(`chevron sits 10–12 px inside the right edge and the padding clears it, ${theme} (AC-K1GZTR.2, AC-K1GZTR.3)`, async ({
    page,
  }) => {
    await useTheme(page, theme);
    await open(page);
    const selects = await visibleSelects(page);
    expect(selects.length).toBeGreaterThan(0);
    for (const select of selects) {
      const name = await label(select);
      await rest(page);
      const p = await profile(page, select);
      expect(p.chevronRight, `${name}: chevron drawn`).toBeGreaterThan(0);
      const gap = gapOf(p);
      expect(gap, `${name}: gap to the right edge`).toBeGreaterThanOrEqual(GAP_MIN - AA);
      expect(gap, `${name}: gap to the right edge`).toBeLessThanOrEqual(GAP_MAX + AA);
      // The content box ends left of the chevron: padding-right ≥ chevron + gap.
      const chevronWidth = (p.chevronRight - p.chevronLeft + 1) / p.devicePixelRatio;
      expect(p.paddingRight, `${name}: right padding holds chevron + gap`).toBeGreaterThanOrEqual(
        chevronWidth + gap,
      );
    }
  });

  test(`a compact (narrow) top bar keeps the gap and the padding, ${theme} (AC-K1GZTR.2, AC-K1GZTR.3)`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 900, height: 720 });
    await useTheme(page, theme);
    await open(page);
    const selects = await visibleSelects(page);
    expect(selects.length).toBeGreaterThan(0);
    for (const select of selects) {
      const name = await label(select);
      await rest(page);
      const p = await profile(page, select);
      const gap = gapOf(p);
      expect(gap, `${name}: compact gap`).toBeGreaterThanOrEqual(GAP_MIN - AA);
      expect(gap, `${name}: compact gap`).toBeLessThanOrEqual(GAP_MAX + AA);
      const chevronWidth = (p.chevronRight - p.chevronLeft + 1) / p.devicePixelRatio;
      expect(p.paddingRight, `${name}: compact padding`).toBeGreaterThanOrEqual(chevronWidth + gap);
    }
  });
}

test("a long option label is cut before it reaches the chevron (AC-K1GZTR.3)", async ({ page }) => {
  await open(page);
  const picker = page.getByLabel("Device", { exact: true });
  await picker.evaluate((el) => {
    const select = el as HTMLSelectElement;
    const option = select.options[select.selectedIndex] ?? select.options[0];
    option.textContent =
      "Demo device — an extremely long meter name that never fits the box ".repeat(3);
  });
  await rest(page);
  const p = await profile(page, picker);
  expect(p.chevronRight, "chevron drawn").toBeGreaterThan(0);
  const gap = gapOf(p);
  expect(gap).toBeGreaterThanOrEqual(GAP_MIN - AA);
  expect(gap).toBeLessThanOrEqual(GAP_MAX + AA);
  // The label runs right up to its content edge and stops there, with clear space
  // (at least 2 px) before the chevron.
  expect(p.textRight, "the long label is drawn").toBeGreaterThan(0);
  const clear = (p.chevronLeft - (p.textRight + 1)) / p.devicePixelRatio;
  expect(clear, "space between the cut label and the chevron").toBeGreaterThanOrEqual(2);
  const contentRight = p.width - p.borderRight - p.paddingRight;
  expect(contentRight, "content box ends before the chevron").toBeLessThanOrEqual(
    p.chevronLeft / p.devicePixelRatio,
  );
});

test("forced-colors mode brings back the native arrow (AC-K1GZTR.4)", async ({ page }) => {
  await page.emulateMedia({ forcedColors: "active" });
  await open(page);
  const forced = await page.evaluate(() => matchMedia("(forced-colors: active)").matches);
  test.skip(!forced, "this engine does not emulate forced-colors");
  const selects = await visibleSelects(page);
  expect(selects.length).toBeGreaterThan(0);
  for (const select of selects) {
    const name = await label(select);
    const style = await arrowStyle(select);
    expect(style.appearance, `${name}: native arrow back`).not.toBe("none");
  }
  // And outside forced colors the same selects draw the app chevron.
  await page.emulateMedia({ forcedColors: "none" });
  for (const select of selects) {
    const style = await arrowStyle(select);
    expect(style.appearance, `${await label(select)}: app chevron outside forced colors`).toBe(
      "none",
    );
  }
});

test("hover, focus and disabled states keep their look and the chevron (AC-K1GZTR.5)", async ({
  page,
}) => {
  await open(page);
  const picker = page.getByLabel("Device", { exact: true });
  const focusColor = await page.evaluate(() => {
    const probe = document.createElement("span");
    probe.style.color = "var(--focus)";
    document.body.append(probe);
    const c = getComputedStyle(probe).color;
    probe.remove();
    return c;
  });

  // Hover: the border takes the text color, the chevron stays.
  await rest(page);
  await picker.hover();
  await page.waitForTimeout(200);
  const hover = await picker.evaluate((el) => {
    const s = getComputedStyle(el);
    return { border: s.borderRightColor, color: s.color, bg: s.backgroundImage };
  });
  expect(hover.border, "hover border").toBe(hover.color);
  expect(
    (hover.bg.match(/linear-gradient\(/g) ?? []).length,
    "chevron on hover",
  ).toBeGreaterThanOrEqual(2);

  // Keyboard focus: the shared 2 px focus ring, the chevron stays.
  await page.mouse.move(1, 1);
  await picker.focus();
  const focus = await picker.evaluate((el) => {
    const s = getComputedStyle(el);
    return {
      visible: el.matches(":focus-visible"),
      style: s.outlineStyle,
      width: parseFloat(s.outlineWidth),
      color: s.outlineColor,
      bg: s.backgroundImage,
    };
  });
  if (focus.visible) {
    expect(focus.style, "focus ring").toBe("solid");
    expect(focus.width, "focus ring width").toBe(2);
    expect(focus.color, "focus ring color").toBe(focusColor);
  }
  expect(
    (focus.bg.match(/linear-gradient\(/g) ?? []).length,
    "chevron on focus",
  ).toBeGreaterThanOrEqual(2);

  // Disabled: faded as before, the chevron stays.
  await picker.evaluate((el) => {
    (el as HTMLSelectElement).blur();
    (el as HTMLSelectElement).disabled = true;
  });
  const disabled = await picker.evaluate((el) => {
    const s = getComputedStyle(el);
    return { opacity: s.opacity, cursor: s.cursor, bg: s.backgroundImage };
  });
  expect(disabled.opacity, "disabled opacity").toBe("0.55");
  expect(disabled.cursor, "disabled cursor").toBe("not-allowed");
  expect(
    (disabled.bg.match(/linear-gradient\(/g) ?? []).length,
    "chevron when disabled",
  ).toBeGreaterThanOrEqual(2);
});
