/**
 * FR AXHK2W: the PD capture's status samples stay within their cap.
 *
 * - AC .2: the page's buffer drops its oldest samples past the cap, with a count.
 * - AC .3: a drop the backend reports reaches the page, on a `pd_status` batch
 *   (`data.dropped`: the backend's count since the last clear) and in the attach
 *   snapshot of a page that attaches later (`pd_status_dropped`), which
 *   `PdCapture.adopt(snapshot)` applies with `pd_capturing`.
 * - AC .6: a meter's parked capture obeys the same caps.
 *
 * The backend's count and the page's own count describe the same oldest samples (both
 * keep the newest 100,000), so the page shows the larger of the two, never their sum.
 */

import { describe, expect, it } from "vitest";
import type { AttachedSession, BackendEvent, PdRow, PdStatusSample } from "./backend";
import { Meters } from "./meters.svelte";
import { PdCapture } from "./pdCapture.svelte";
import { PD_MAX_ROWS } from "./pdStore";
import { PD_STATUS_MAX_SAMPLES, pdStatusDroppedText } from "./pdTimeline";

function sample(ms: number): PdStatusSample {
  return { session_ms: ms, vbus_v: 5, ibus_a: 0.1, cc1_v: 1.6, cc2_v: 0 };
}

function samples(from: number, n: number): PdStatusSample[] {
  return Array.from({ length: n }, (_, i) => sample(from + i));
}

function statusEvent(list: PdStatusSample[], dropped?: number): BackendEvent {
  return {
    type: "pd_status",
    data: dropped === undefined ? { samples: list } : { samples: list, dropped },
  };
}

function row(index: number): PdRow {
  return {
    index,
    elapsed_ms: index,
    kind: "message",
    sop: "SOP",
    name: "Accept",
    message_id: 1,
    direction: "source_to_sink",
    objects: 0,
    spec_revision: "3.x",
    vbus_v: 5,
    ibus_a: 0,
    summary: "Accept",
  };
}

function snapshot(extra: Partial<AttachedSession>): AttachedSession {
  return {
    device_id: "demo:pd-9v-2a",
    model: "KM003C",
    firmware_version: "2.0.6",
    serial: null,
    rate_sps: 1000,
    rate_locked: false,
    session_id: 1,
    recording: null,
    ...extra,
  };
}

describe("the live capture's status cap (AC-AXHK2W.2)", () => {
  it("AC-AXHK2W.2: pd_status batches past the cap drop the oldest samples, counted", () => {
    const pd = new PdCapture();
    pd.handleEvent(statusEvent(samples(1, PD_STATUS_MAX_SAMPLES)));
    pd.handleEvent(statusEvent(samples(1 + PD_STATUS_MAX_SAMPLES, 4)));
    expect(pd.status.length).toBe(PD_STATUS_MAX_SAMPLES);
    expect(pd.status.dropped).toBe(4);
    expect(pd.status.range(0, 5).t).toEqual([5]);
  });
});

describe("a drop the backend reports (AC-AXHK2W.3)", () => {
  it("AC-AXHK2W.3: a pd_status batch's count reaches the page's notice", () => {
    const pd = new PdCapture();
    pd.handleEvent(statusEvent(samples(1, 3), 42));
    expect(pd.status.length).toBe(3);
    expect(pd.status.dropped).toBe(42);
    expect(pdStatusDroppedText(pd.status.dropped)).toContain("42");
  });

  it("AC-AXHK2W.3: the page shows the larger count, not the sum of its own and the backend's", () => {
    const pd = new PdCapture();
    pd.handleEvent(statusEvent(samples(1, PD_STATUS_MAX_SAMPLES)));
    pd.handleEvent(statusEvent(samples(1 + PD_STATUS_MAX_SAMPLES, 5), 5));
    expect(pd.status.dropped).toBe(5);
    pd.handleEvent(statusEvent(samples(6 + PD_STATUS_MAX_SAMPLES, 1), 6));
    expect(pd.status.dropped).toBe(6);
  });

  it("AC-AXHK2W.3: a page that attaches later learns the backend's count from the snapshot", () => {
    const pd = new PdCapture();
    pd.adopt(snapshot({ pd_capturing: true, pd_status_dropped: 1_234 }));
    expect(pd.capturing).toBe(true);
    expect(pd.status.length).toBe(0);
    expect(pd.status.dropped).toBe(1_234);
    expect(pdStatusDroppedText(pd.status.dropped)).toContain("1,234");
    // Later batches go on from there.
    pd.handleEvent(statusEvent(samples(10, 2), 1_240));
    expect(pd.status.dropped).toBe(1_240);
  });

  it("AC-AXHK2W.3: a snapshot from a backend that dropped nothing shows no notice", () => {
    const pd = new PdCapture();
    pd.adopt(snapshot({ pd_capturing: false, pd_status_dropped: 0 }));
    expect(pd.capturing).toBe(false);
    expect(pdStatusDroppedText(pd.status.dropped)).toBeNull();
  });

  it("AC-AXHK2W.3: a clear starts the count over", () => {
    const pd = new PdCapture();
    pd.handleEvent(statusEvent(samples(1, 2), 9));
    expect(pd.status.dropped).toBe(9);
    pd.clear();
    expect(pd.status.dropped).toBe(0);
  });
});

describe("parked meters (AC-AXHK2W.6)", () => {
  it("AC-AXHK2W.6: a parked capture's status samples obey the cap", () => {
    const meters = new Meters();
    const parked = new PdCapture();
    parked.capturing = true;
    meters.parkPd("demo:pd-9v-2a", parked);
    meters.handleTileEvent("demo:pd-9v-2a", statusEvent(samples(1, PD_STATUS_MAX_SAMPLES)));
    meters.handleTileEvent("demo:pd-9v-2a", statusEvent(samples(1 + PD_STATUS_MAX_SAMPLES, 3)));
    const back = meters.unparkPd("demo:pd-9v-2a");
    expect(back).toBe(parked);
    expect(parked.status.length).toBe(PD_STATUS_MAX_SAMPLES);
    expect(parked.status.dropped).toBe(3);
    expect(parked.status.range(0, 4).t).toEqual([4]);
  });

  it("AC-AXHK2W.6: a parked capture's rows obey the row cap", () => {
    const meters = new Meters();
    const parked = new PdCapture();
    parked.capturing = true;
    meters.parkPd("demo:pd-9v-2a", parked);
    const rows = Array.from({ length: PD_MAX_ROWS + 2 }, (_, i) => row(i));
    meters.handleTileEvent("demo:pd-9v-2a", { type: "pd_rows", data: { rows } });
    expect(parked.store.length).toBe(PD_MAX_ROWS);
    expect(parked.store.dropped).toBe(2);
  });

  it("AC-AXHK2W.6: a parked capture learns the backend's drop count too", () => {
    const meters = new Meters();
    const parked = new PdCapture();
    meters.parkPd("demo:pd-9v-2a", parked);
    meters.handleTileEvent("demo:pd-9v-2a", statusEvent(samples(1, 1), 77));
    expect(parked.status.dropped).toBe(77);
  });
});
