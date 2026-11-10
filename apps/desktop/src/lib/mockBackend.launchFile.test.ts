/** FR M078V5 on the demo backend: `?launchfile=1` hands the app a recording at launch,
 * taken once; it opens through the same scripted reader as Open recording. */

import { describe, expect, it } from "vitest";
import { MOCK_RECORDING_FILE, MockBackend, launchFileFromUrl } from "./mockBackend";

describe("launchFileFromUrl", () => {
  it("reads ?launchfile=1 and ignores anything else", () => {
    expect(launchFileFromUrl("?launchfile=1")).toBe(true);
    expect(launchFileFromUrl("?mock=x&launchfile=1")).toBe(true);
    expect(launchFileFromUrl("?launchfile=0")).toBe(false);
    expect(launchFileFromUrl("?recordingfile=refused")).toBe(false);
    expect(launchFileFromUrl("")).toBe(false);
    expect(launchFileFromUrl(undefined)).toBe(false);
  });
});

describe("MockBackend launch file (AC-M078V5.1, .2, .4)", () => {
  it("opens the demo recording once, with progress, then has none", async () => {
    const b = new MockBackend(null, { launchFile: true });
    const seen: unknown[] = [];
    const s = await b.openLaunchRecordingFile((p) => seen.push(p));
    expect(s?.name).toBe(MOCK_RECORDING_FILE.name);
    expect(s?.rows).toBe(MOCK_RECORDING_FILE.rows);
    expect(seen.length).toBeGreaterThan(0);
    const [vbus] = await b.recordingFileRange(["vbus"], 0, s?.stats.elapsed_ms ?? 0, 64);
    expect(vbus?.buckets.length).toBeGreaterThan(0);
    expect(await b.openLaunchRecordingFile(() => {})).toBeNull();
  });

  it("has no launch file without the option", async () => {
    const b = new MockBackend(null, {});
    expect(await b.openLaunchRecordingFile(() => {})).toBeNull();
  });

  it("a refused launch file rejects with Open recording's message", async () => {
    const opened = new MockBackend(null, { recordingFileFault: "refused" });
    const dialogError = await opened.openRecordingFile(() => {}).catch((e: unknown) => e);
    const b = new MockBackend(null, { launchFile: true, recordingFileFault: "refused" });
    const launchError = await b.openLaunchRecordingFile(() => {}).catch((e: unknown) => e);
    expect(launchError).toEqual(dialogError);
    expect((launchError as { message?: string }).message).toMatch(/header/);
  });
});
