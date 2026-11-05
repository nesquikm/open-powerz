/** The demo backend's recording file (FR AXHK2S): no meter needed, nothing sent to one, a
 * known row count and known skipped rows; `recordingFileFault` scripts a refusal, a slow
 * open that only Cancel ends, and a file past the row limit. */

import { describe, expect, it } from "vitest";
import { MOCK_RECORDING_FILE, MockBackend, recordingFileFaultFromUrl } from "./mockBackend";

const opts = (
  recordingFileFault: "refused" | "slow" | "truncated" | "dialog" | "unknown-total",
) => ({ recordingFileFault });

describe("MockBackend recording file (AC-AXHK2S.1, .2)", () => {
  it("opens with no meter connected, with its known rows and skipped rows", async () => {
    const b = new MockBackend(null);
    const seen: unknown[] = [];
    const s = await b.openRecordingFile((p) => seen.push(p));
    expect(s?.name).toBe(MOCK_RECORDING_FILE.name);
    expect(s?.rows).toBe(MOCK_RECORDING_FILE.rows);
    expect(s?.skipped).toBe(MOCK_RECORDING_FILE.skipped);
    expect(s?.left_out).toBe(0);
    expect(s?.stats.samples).toBe(MOCK_RECORDING_FILE.rows);
    expect(seen.length).toBeGreaterThan(0);
  });

  it("reads buckets of the open file and none once it is closed", async () => {
    const b = new MockBackend(null);
    const s = await b.openRecordingFile(() => {});
    const [vbus] = await b.recordingFileRange(["vbus"], 0, s?.stats.elapsed_ms ?? 0, 64);
    expect(vbus?.series).toBe("vbus");
    expect(vbus?.buckets.length).toBeGreaterThan(0);
    expect(vbus?.buckets.length).toBeLessThanOrEqual(64);

    await b.closeRecordingFile();
    await expect(b.recordingFileRange(["vbus"], 0, 1000, 64)).rejects.toMatchObject({
      kind: "usage",
    });
  });

  it("refuses in plain words on `refused`", async () => {
    const b = new MockBackend(null, opts("refused"));
    await expect(b.openRecordingFile(() => {})).rejects.toMatchObject({
      message: expect.stringMatching(/header/i),
    });
  });

  it("`slow` stays open until Cancel, which ends it in null", async () => {
    const b = new MockBackend(null, opts("slow"));
    const seen: { done: number; total: number }[] = [];
    const opening = b.openRecordingFile((p) => seen.push(p));
    await Promise.resolve();
    await b.cancelRecordingFile();
    await expect(opening).resolves.toBeNull();
    expect(seen.length).toBeGreaterThan(0);
  });

  it("`dialog` reports nothing until Cancel, like an open dialog", async () => {
    const b = new MockBackend(null, opts("dialog"));
    const seen: unknown[] = [];
    const opening = b.openRecordingFile((p) => seen.push(p));
    await Promise.resolve();
    expect(seen).toEqual([]);
    await b.cancelRecordingFile();
    await expect(opening).resolves.toBeNull();
  });

  it("`unknown-total` reports a total of 0 until Cancel", async () => {
    const b = new MockBackend(null, opts("unknown-total"));
    const seen: { done: number; total: number }[] = [];
    const opening = b.openRecordingFile((p) => seen.push(p));
    await Promise.resolve();
    expect(seen.length).toBeGreaterThan(0);
    expect(seen.every((p) => p.total === 0)).toBe(true);
    await b.cancelRecordingFile();
    await expect(opening).resolves.toBeNull();
  });

  it("reports at 0 first, as the backend does once a file is chosen", async () => {
    const b = new MockBackend(null);
    const seen: unknown[] = [];
    await b.openRecordingFile((p) => seen.push(p));
    expect(seen[0]).toEqual({ done: 0, total: 0 });
  });

  it("`truncated` says how many rows were left out", async () => {
    const b = new MockBackend(null, opts("truncated"));
    const s = await b.openRecordingFile(() => {});
    expect(s?.left_out).toBeGreaterThan(0);
  });

  it("reads ?recordingfile=refused|slow|truncated and ignores anything else", () => {
    expect(recordingFileFaultFromUrl("?recordingfile=refused")).toBe("refused");
    expect(recordingFileFaultFromUrl("?mock=replug&recordingfile=slow")).toBe("slow");
    expect(recordingFileFaultFromUrl("?recordingfile=truncated")).toBe("truncated");
    expect(recordingFileFaultFromUrl("?recordingfile=dialog")).toBe("dialog");
    expect(recordingFileFaultFromUrl("?recordingfile=unknown-total")).toBe("unknown-total");
    expect(recordingFileFaultFromUrl("?recordingfile=other")).toBeNull();
  });
});

describe("MockBackend Export .db (AC-AXHK2S.6)", () => {
  it("needs a meter, reports progress and says how many rows it wrote", async () => {
    const b = new MockBackend(null);
    await expect(b.exportSessionDb("run.db", () => {})).rejects.toBeDefined();

    await b.connect("demo:load-5v-1a", 50, () => {});
    await new Promise((r) => setTimeout(r, 60));
    const seen: { done: number; total: number }[] = [];
    const res = await b.exportSessionDb("run.db", (p) => seen.push(p));
    expect(res?.path).toBe("run.db");
    expect(seen.at(-1)?.done).toBe(res?.rows);
    await b.disconnect();
  });
});
