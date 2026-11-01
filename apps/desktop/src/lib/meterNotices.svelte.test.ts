/** FR AXHK2K: one notice store per meter, read by the tiles and the full view, so a
 * notice stays with its meter wherever that meter is shown (AC-AXHK2K.2, AC-AXHK2K.4). */

import { describe, expect, it } from "vitest";
import type { BackendEvent, DeviceListing } from "./backend";
import { Meters } from "./meters.svelte";
import { MeterNotices } from "./meterNotices.svelte";
import { recordingSummaryText } from "./recording.svelte";
import { Session } from "./session.svelte";

const A = "demo:load-5v-1a";
const B = "demo:pd-9v-2a";

function listing(id: string): DeviceListing {
  return {
    id,
    label: `Demo device — ${id}`,
    model: "KM003C",
    serial: id,
    bus: null,
    address: null,
    demo_scenario: id.slice("demo:".length),
    mode: "application",
  };
}

function stopped(rows: number, disconnected: boolean): BackendEvent {
  return { type: "recording", data: { state: "stopped", rows, gaps: [], disconnected } };
}

describe("MeterNotices (FR AXHK2K)", () => {
  it("keeps each meter's notices apart", () => {
    const notices = new MeterNotices();
    notices.raise(A, { tone: "info", text: "saved", path: "a.csv" });
    notices.raise(B, { tone: "error", text: "refused", path: null });
    expect(notices.of(A)).toEqual([expect.objectContaining({ text: "saved", path: "a.csv" })]);
    expect(notices.of(B)).toEqual([expect.objectContaining({ text: "refused", tone: "error" })]);
  });

  it("has none for a meter nothing was raised on", () => {
    expect(new MeterNotices().of(A)).toEqual([]);
  });

  it("keeps a meter's notices in the order they came", () => {
    const notices = new MeterNotices();
    notices.raise(A, { tone: "info", text: "first", path: null });
    notices.raise(A, { tone: "info", text: "second", path: null });
    expect(notices.of(A).map((n) => n.text)).toEqual(["first", "second"]);
  });

  it("dismisses one notice of one meter", () => {
    const notices = new MeterNotices();
    notices.raise(A, { tone: "info", text: "first", path: null });
    notices.raise(A, { tone: "info", text: "second", path: null });
    notices.raise(B, { tone: "info", text: "other", path: null });
    const [first] = notices.of(A);
    if (first === undefined) throw new Error("no notice");
    notices.dismiss(A, first);
    expect(notices.of(A).map((n) => n.text)).toEqual(["second"]);
    expect(notices.of(B).map((n) => n.text)).toEqual(["other"]);
  });
});

describe("A notice raised on a parked meter (AC-AXHK2K.4)", () => {
  function parked(): Meters {
    const meters = new Meters();
    meters.add(listing(A));
    meters.add(listing(B));
    meters.park(B, { rows: 3, elapsed_ms: 100, path: "b.csv" });
    return meters;
  }

  it("is kept for that meter, with its file", () => {
    const meters = parked();
    meters.handleTileEvent(B, stopped(3, true));
    const summary = { rows: 3, gaps: [], disconnected: true };
    expect(meters.notices.of(B)).toEqual([
      expect.objectContaining({ text: recordingSummaryText(summary, "b.csv"), path: "b.csv" }),
    ]);
    expect(meters.notices.of(A)).toEqual([]);
  });

  it("is still there once the meter's parked recording has been taken back", () => {
    const meters = parked();
    meters.handleTileEvent(B, stopped(3, true));
    expect(meters.unpark(B)).toBeUndefined();
    expect(meters.notices.of(B)).toHaveLength(1);
  });

  it("survives the meter moving to another session store", () => {
    const meters = parked();
    meters.handleTileEvent(B, stopped(3, true));
    meters.replace(B, new Session());
    expect(meters.notices.of(B)).toHaveLength(1);
  });

  it("goes with a recording that failed too", () => {
    const meters = parked();
    meters.handleTileEvent(B, {
      type: "recording",
      data: {
        state: "failed",
        rows: 2,
        error: { kind: "internal", message: "The disk is full.", hint: null },
      },
    });
    expect(meters.notices.of(B)).toEqual([
      expect.objectContaining({ tone: "error", text: expect.stringContaining("2 rows kept") }),
    ]);
  });
});
