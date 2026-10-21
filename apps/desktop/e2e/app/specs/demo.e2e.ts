/**
 * The real desktop app against the demo device (FR AH2AEN):
 * - AC-AH2AEN.1: pick "Demo device — load-5v-1a", connect, VBUS within 5% of
 *   5 V and IBUS within 5% of 1 A in the readout;
 * - AC-AH2AEN.2: the chart has drawn VBUS and IBUS points (test hook of the
 *   `e2e` build) and the stats elapsed time advances;
 * - AC-AH2AEN.3: record 5 s at 50 SPS into the e2e save directory, stop,
 *   export the session; both CSVs start with the vendor header and hold
 *   rate × duration rows within 5%. The duration is the span the CSV itself
 *   records (first to last `ElapsedTime`), since the WebDriver round trips
 *   between Record and Stop add a few hundred ms to the 5 s pause.
 */

import { readFileSync, readdirSync, statSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { browser, $, expect } from "@wdio/globals";
import { HOOK, type OpenPowerzE2E } from "../../hook";

const SAVE_DIR_ENV = "OPEN_POWERZ_E2E_SAVE_DIR";
const RATE_SPS = 50;
const RECORD_S = 5;
/** The longest recorded span accepted for a 5 s pause (WebDriver and stop latency). */
const MAX_RECORD_SPAN_S = 6.5;

const here = path.dirname(fileURLToPath(import.meta.url));

/** The vendor header, read from `powerz-record` (the one writer every CSV goes through). */
function vendorHeader(): string {
  const src = readFileSync(
    path.resolve(here, "../../../../../crates/powerz-record/src/lib.rs"),
    "utf8",
  );
  const m = /pub const CSV_HEADER: &str = "([^"]+)";/.exec(src);
  if (!m?.[1]) throw new Error("CSV_HEADER not found in powerz-record");
  return m[1];
}

function saveDir(): string {
  const dir = process.env[SAVE_DIR_ENV];
  if (!dir) throw new Error(`${SAVE_DIR_ENV} is not set (see wdio.conf.ts)`);
  return dir;
}

/** CSV files in the save directory, oldest first. */
function csvFiles(): string[] {
  const dir = saveDir();
  return readdirSync(dir)
    .filter((f) => f.endsWith(".csv"))
    .map((f) => path.join(dir, f))
    .sort((a, b) => statSync(a).mtimeMs - statSync(b).mtimeMs);
}

/** `h:mm:ss.mmm` (`format_elapsed` of `powerz-record`) → seconds. */
function elapsedSeconds(cell: string): number {
  const [h, m, s] = cell.split(":").map(Number);
  return (h ?? NaN) * 3600 + (m ?? NaN) * 60 + (s ?? NaN);
}

/** Seconds between the first and the last row's `ElapsedTime` (counted from the first row). */
function recordedSpan(rows: string[][]): number {
  return elapsedSeconds(rows.at(-1)?.[0] ?? "") - elapsedSeconds(rows[0]?.[0] ?? "");
}

/** Rows within 5% of `rate × span` samples, plus the one at the start of the span. */
function expectRowsForSpan(rows: number, spanS: number): void {
  const want = RATE_SPS * spanS + 1;
  expect(rows).toBeGreaterThanOrEqual(want * 0.95);
  expect(rows).toBeLessThanOrEqual(want * 1.05);
}

function readCsv(file: string): { header: string; rows: string[][] } {
  const lines = readFileSync(file, "utf8").split(/\r?\n/);
  if (lines.at(-1) === "") lines.pop();
  const [header = "", ...rows] = lines;
  return { header, rows: rows.map((l) => l.split(",")) };
}

async function readout(name: string): Promise<number> {
  const text = await $(`[aria-label="Live readout"] [aria-label="${name}"] .digits`).getText();
  return Number.parseFloat(text.replace(/[^\d.+-]/g, ""));
}

async function statsElapsed(): Promise<number> {
  const text = await $('[aria-label="Statistics"] .totals dd').getText();
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

async function button(label: string) {
  return $(`button=${label}`);
}

describe("open-powerz against the demo device", () => {
  it("connects to the 5 V / 1 A demo device and shows its values (AC-AH2AEN.1)", async () => {
    const picker = await $('select[aria-label="Device"]');
    await picker.waitForEnabled({ timeout: 20_000 });
    await picker.selectByVisibleText("Demo device — load-5v-1a");
    await $('select[aria-label="Sample rate"]').selectByAttribute("value", String(RATE_SPS));
    await (await button("Connect")).click();
    await expect($(".badge")).toHaveText("Streaming", { wait: 10_000 });

    await browser.waitUntil(async () => Math.abs((await readout("VBUS")) - 5) / 5 <= 0.05, {
      timeout: 5_000,
      timeoutMsg: "VBUS never within 5% of 5 V",
    });
    await browser.waitUntil(async () => Math.abs((await readout("IBUS")) - 1) <= 0.05, {
      timeout: 5_000,
      timeoutMsg: "IBUS never within 5% of 1 A",
    });
  });

  it("draws VBUS and IBUS and advances the elapsed time (AC-AH2AEN.2)", async () => {
    await browser.waitUntil(
      async () => {
        const n = await seriesLengths();
        return n.vbus > 0 && n.ibus > 0;
      },
      { timeout: 5_000, timeoutMsg: "the chart drew no VBUS / IBUS points" },
    );
    const before = await statsElapsed();
    await browser.waitUntil(async () => (await statsElapsed()) > before, {
      timeout: 5_000,
      timeoutMsg: "the stats elapsed time did not advance",
    });
    const later = await seriesLengths();
    expect(later.vbus).toBeGreaterThan(0);
    expect(later.ibus).toBeGreaterThan(0);
  });

  it("records 5 s at 50 SPS and exports the session as vendor CSVs (AC-AH2AEN.3)", async () => {
    const header = vendorHeader();
    expect(csvFiles()).toHaveLength(0);

    await (await button("Record")).click();
    await expect($('[role="timer"]')).toBeDisplayed({ wait: 5_000 });
    await browser.pause(RECORD_S * 1000);
    await (await button("Stop")).click();
    await expect(await button("Record")).toBeDisplayed({ wait: 5_000 });
    await browser.waitUntil(() => csvFiles().length === 1, {
      timeout: 5_000,
      timeoutMsg: "the recording was not written to the save directory",
    });

    // A new second, so the proposed export name differs from the recording's.
    await browser.pause(1100);
    await (await button("Export")).click();
    await browser.waitUntil(() => csvFiles().length === 2, {
      timeout: 10_000,
      timeoutMsg: "the export was not written to the save directory",
    });
    await expect($(".export-progress")).not.toBeExisting({ wait: 10_000 });

    const [recordingFile, exportFile] = csvFiles() as [string, string];
    const recording = readCsv(recordingFile);
    const exported = readCsv(exportFile);
    expect(recording.header).toBe(header);
    expect(exported.header).toBe(header);

    // The recording spans the 5 s pause plus the WebDriver latency around it,
    // and holds rate × that span rows.
    const span = recordedSpan(recording.rows);
    expect(span).toBeGreaterThanOrEqual(RECORD_S);
    expect(span).toBeLessThanOrEqual(MAX_RECORD_SPAN_S);
    expectRowsForSpan(recording.rows.length, span);

    // The export holds the whole session: at least the recording, at the
    // session's own rate × duration.
    expect(exported.rows.length).toBeGreaterThanOrEqual(recording.rows.length);
    const exportSpan = recordedSpan(exported.rows);
    expect(exportSpan).toBeGreaterThanOrEqual(span);
    expectRowsForSpan(exported.rows.length, exportSpan);

    await (await button("Disconnect")).click();
    await expect($(".badge")).toHaveText("Disconnected", { wait: 5_000 });
  });
});
