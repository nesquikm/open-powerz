/**
 * FR TGE83C (AC .1, .2, .7): the PD view's capture state. It takes the backend's
 * `pd_state` / `pd_rows` events off the session channel, keeps the rows in its store,
 * and says when Start and Stop are available and, when they are not, why.
 */

import { describe, expect, it } from "vitest";
import type { BackendEvent, PdRow, PdStatusSample } from "./backend";
import { PdCapture } from "./pdCapture.svelte";
import { PD_MAX_ROWS, pdDroppedText } from "./pdStore";

function row(index: number, name = "Accept"): PdRow {
  return {
    index,
    elapsed_ms: index,
    kind: "message",
    sop: "SOP",
    name,
    message_id: 1,
    direction: "source_to_sink",
    objects: 0,
    spec_revision: "3.x",
    vbus_v: 5,
    ibus_a: 0,
    summary: name,
  };
}

const started: BackendEvent = { type: "pd_state", data: { capturing: true } };
const stopped: BackendEvent = { type: "pd_state", data: { capturing: false } };

describe("Start and Stop (AC-TGE83C.1)", () => {
  it("are both disabled with a reason when no meter is connected", () => {
    const pd = new PdCapture();
    const c = pd.controls(false);
    expect(c.canStart).toBe(false);
    expect(c.canStop).toBe(false);
    expect(c.reason).toMatch(/connect/i);
  });

  it("Start is available on a connected meter that is not capturing", () => {
    const pd = new PdCapture();
    const c = pd.controls(true);
    expect(c.canStart).toBe(true);
    expect(c.canStop).toBe(false);
  });

  it("Stop is available while capturing, Start is not", () => {
    const pd = new PdCapture();
    pd.handleEvent(started);
    expect(pd.capturing).toBe(true);
    const c = pd.controls(true);
    expect(c.canStart).toBe(false);
    expect(c.canStop).toBe(true);
    pd.handleEvent(stopped);
    expect(pd.capturing).toBe(false);
  });

  it("a session that ends stops the capture", () => {
    const pd = new PdCapture();
    pd.handleEvent(started);
    pd.handleEvent({ type: "status", data: { state: "disconnected", error: null } });
    expect(pd.capturing).toBe(false);
    expect(pd.controls(false).canStop).toBe(false);
  });
});

describe("rows (AC-TGE83C.2)", () => {
  it("appends every pd_rows batch to the store, in order", () => {
    const pd = new PdCapture();
    pd.handleEvent(started);
    pd.handleEvent({ type: "pd_rows", data: { rows: [row(0), row(1, "GoodCRC")] } });
    pd.handleEvent({ type: "pd_rows", data: { rows: [row(2, "PS_RDY")] } });
    expect(pd.store.length).toBe(3);
    expect(pd.store.row(2).name).toBe("PS_RDY");
  });

  it("ignores the measurement stream", () => {
    const pd = new PdCapture();
    pd.handleEvent({ type: "gap", data: { kind: "stream", from_ms: 0, to_ms: 10 } });
    expect(pd.store.length).toBe(0);
  });
});

describe("clear (AC-TGE83C.7)", () => {
  it("empties the store and keeps capturing", () => {
    const pd = new PdCapture();
    pd.handleEvent(started);
    pd.handleEvent({ type: "pd_rows", data: { rows: [row(0), row(1)] } });
    pd.clear();
    expect(pd.store.length).toBe(0);
    expect(pd.capturing).toBe(true);
  });
});

describe("Clear against rows still in flight (AC-TGE83C.7)", () => {
  it("drops rows numbered before the backend's clear point", () => {
    const pd = new PdCapture();
    pd.handleEvent({ type: "pd_rows", data: { rows: [row(0), row(1)] } } as BackendEvent);
    pd.clear(2);
    expect(pd.store.length).toBe(0);
    // A batch the backend sent before it cleared arrives late, then new rows.
    pd.handleEvent({ type: "pd_rows", data: { rows: [row(1), row(2), row(3)] } } as BackendEvent);
    expect(pd.store.length).toBe(2);
    expect(pd.store.row(0).index).toBe(2);
  });
});

describe("a new backend session (reconnect, FR TGE839)", () => {
  const connected: BackendEvent = {
    type: "status",
    data: {
      state: "connected",
      model: "KM003C",
      firmware_version: "2.0.6",
      bootloader_version: "",
      mode: "application",
      warnings: [],
      update_required: false,
      firmware_problems: [],
      serial: "SIM",
      rate_sps: 50,
      rate_locked: false,
      session_id: 2,
      min_supported_firmware: "1.7.0",
      known_latest_firmware: "2.0.7",
    },
  };

  it("empties the store, so the new session's rows (numbered from 0) cannot collide", () => {
    const pd = new PdCapture();
    pd.handleEvent({ type: "pd_rows", data: { rows: [row(0), row(1), row(2)] } });
    const before = pd.version;
    pd.handleEvent(connected);
    expect(pd.store.length).toBe(0);
    expect(pd.version).toBeGreaterThan(before);
    pd.handleEvent({ type: "pd_rows", data: { rows: [row(0), row(1)] } });
    expect(pd.store.length).toBe(2);
    expect(pd.store.row(0).index).toBe(0);
  });

  it("forgets the old session's clear point", () => {
    const pd = new PdCapture();
    pd.handleEvent({ type: "pd_rows", data: { rows: [row(0), row(1), row(2)] } });
    pd.clear(3);
    pd.handleEvent(connected);
    pd.handleEvent({ type: "pd_rows", data: { rows: [row(0), row(1)] } });
    expect(pd.store.length).toBe(2);
  });

  it("ends a clear still waiting for the old session's answer", () => {
    const pd = new PdCapture();
    pd.beginClear();
    pd.handleEvent(connected);
    pd.handleEvent({ type: "pd_rows", data: { rows: [row(0), row(1)] } });
    expect(pd.store.length).toBe(2);
    // The old session's answer comes late: it must not drop the new session's rows.
    pd.endClear(40);
    pd.handleEvent({ type: "pd_rows", data: { rows: [row(2)] } });
    expect(pd.store.length).toBe(3);
  });
});

describe("Clear while the backend's answer is on its way (AC-TGE83C.7)", () => {
  it("empties the table at once", () => {
    const pd = new PdCapture();
    pd.handleEvent({ type: "pd_rows", data: { rows: [row(0), row(1)] } });
    const before = pd.version;
    pd.beginClear();
    expect(pd.store.length).toBe(0);
    expect(pd.version).toBeGreaterThan(before);
  });

  it("drops stale rows that arrive before the answer", () => {
    const pd = new PdCapture();
    pd.handleEvent({ type: "pd_rows", data: { rows: [row(0), row(1)] } });
    pd.beginClear();
    pd.handleEvent({ type: "pd_rows", data: { rows: [row(2)] } });
    pd.endClear(3);
    expect(pd.store.length).toBe(0);
  });

  it("keeps new rows that arrive before the answer", () => {
    const pd = new PdCapture();
    pd.handleEvent({ type: "pd_rows", data: { rows: [row(0), row(1)] } });
    pd.beginClear();
    // The backend cleared at 2, then sent 2 and 3 before its answer reached the UI.
    pd.handleEvent({ type: "pd_rows", data: { rows: [row(1), row(2), row(3)] } });
    const before = pd.version;
    pd.endClear(2);
    expect(pd.store.length).toBe(2);
    expect(pd.store.row(0).index).toBe(2);
    expect(pd.version).toBeGreaterThan(before);
    pd.handleEvent({ type: "pd_rows", data: { rows: [row(4)] } });
    expect(pd.store.length).toBe(3);
  });

  it("a failed clear keeps every row that arrived meanwhile", () => {
    const pd = new PdCapture();
    pd.handleEvent({ type: "pd_rows", data: { rows: [row(0)] } });
    pd.beginClear();
    pd.handleEvent({ type: "pd_rows", data: { rows: [row(1), row(2)] } });
    pd.endClear(0);
    expect(pd.store.length).toBe(2);
  });
});

describe("emptied counter for the view's selection", () => {
  it("counts reconnects and clears, not new rows", () => {
    const pd = new PdCapture();
    const start = pd.emptied;
    pd.handleEvent({ type: "pd_rows", data: { rows: [row(0)] } } as BackendEvent);
    expect(pd.emptied).toBe(start);
    pd.handleEvent({
      type: "status",
      data: {
        state: "connected",
        model: "KM003C",
        firmware_version: "2.0.6",
        bootloader_version: "",
        mode: "application",
        warnings: [],
        update_required: false,
        firmware_problems: [],
        serial: "SIM",
        rate_sps: 50,
        rate_locked: false,
        session_id: 1,
        min_supported_firmware: "1.7.0",
        known_latest_firmware: "2.0.7",
      },
    } as BackendEvent);
    expect(pd.emptied).toBe(start + 1);
    pd.clear(0);
    expect(pd.emptied).toBe(start + 2);
  });
});

describe("Clear and the PD status samples (AC-TGE83D.4)", () => {
  it("a clear empties the status samples along with the rows", () => {
    const pd = new PdCapture();
    pd.handleEvent({
      type: "pd_status",
      data: { samples: [{ session_ms: 5, vbus_v: 5, ibus_a: 0.1, cc1_v: 1.6, cc2_v: 0 }] },
    } as BackendEvent);
    expect(pd.status.length).toBeGreaterThan(0);
    const version = pd.statusVersion;
    pd.clear(0);
    expect(pd.status.length).toBe(0);
    expect(pd.statusVersion).toBeGreaterThan(version);
  });
});

describe("the table's first index (FR TGE83E, AC .2)", () => {
  it("follows the clear point and goes back to 0 on a new session", () => {
    const pd = new PdCapture();
    expect(pd.firstIndex).toBe(0);
    pd.clear(34);
    expect(pd.firstIndex).toBe(34);
    pd.handleEvent({
      type: "status",
      data: {
        state: "connected",
        model: "KM003C",
        firmware_version: "2.0.6",
        bootloader_version: "",
        mode: "application",
        warnings: [],
        update_required: false,
        firmware_problems: [],
        serial: "SIM",
        rate_sps: 50,
        rate_locked: false,
        session_id: 1,
        min_supported_firmware: "1.7.0",
        known_latest_firmware: "2.0.7",
      },
    } as BackendEvent);
    expect(pd.firstIndex).toBe(0);
  });
});

describe("Open capture and Import vendor log (FR TGE83E)", () => {
  it("are available without a meter and on a meter that is not capturing", () => {
    const pd = new PdCapture();
    expect(pd.controls(false).canOpen).toBe(true);
    expect(pd.controls(false).openReason).toBeNull();
    expect(pd.controls(true).canOpen).toBe(true);
  });

  it("are disabled with a reason while a live capture runs", () => {
    const pd = new PdCapture();
    pd.handleEvent(started);
    const c = pd.controls(true);
    expect(c.canOpen).toBe(false);
    expect(c.openReason).toMatch(/stop the capture first/i);
    pd.handleEvent(stopped);
    expect(pd.controls(true).canOpen).toBe(true);
  });
});

describe("PD status samples in flight at a Clear (AC-4PJRVK.4)", () => {
  const status = (session_ms: number): BackendEvent =>
    ({
      type: "pd_status",
      data: { samples: [{ session_ms, vbus_v: 5, ibus_a: 0.1, cc1_v: 1.6, cc2_v: 0 }] },
    }) as BackendEvent;

  it("drops samples that arrive while the clear waits for its answer", () => {
    const pd = new PdCapture();
    pd.handleEvent(status(5));
    const ticket = pd.beginClear();
    const version = pd.statusVersion;
    pd.handleEvent(status(6));
    pd.handleEvent(status(7));
    expect(pd.status.length).toBe(0);
    expect(pd.statusVersion).toBe(version);
    pd.endClear(3, ticket);
    expect(pd.status.length).toBe(0);
  });

  it("takes samples again once the clear has its answer", () => {
    const pd = new PdCapture();
    const ticket = pd.beginClear();
    pd.handleEvent(status(6));
    pd.endClear(3, ticket);
    pd.handleEvent(status(8));
    expect(pd.status.length).toBe(1);
    expect(pd.status.columns.t[0]).toBe(8);
  });

  it("a new session ends a clear still waiting, so its samples are kept", () => {
    const pd = new PdCapture();
    pd.beginClear();
    pd.handleEvent({
      type: "status",
      data: { state: "connected", session_id: 2 },
    } as BackendEvent);
    pd.handleEvent(status(9));
    expect(pd.status.length).toBe(1);
  });
});

describe("a capture past the row limit (AC-4PJRVK.3)", () => {
  it("counts the rows the store dropped, and a clear or new session forgets them", () => {
    const pd = new PdCapture();
    const rows = Array.from({ length: PD_MAX_ROWS + 7 }, (_, i) => row(i));
    pd.handleEvent({ type: "pd_rows", data: { rows } });
    expect(pd.store.length).toBe(PD_MAX_ROWS);
    expect(pd.store.dropped).toBe(7);
    pd.clear(PD_MAX_ROWS + 7);
    expect(pd.store.dropped).toBe(0);
  });
});

function sample(session_ms: number): PdStatusSample {
  return { session_ms, vbus_v: 5, ibus_a: 0, cc1_v: 0, cc2_v: 0 };
}
const statusEvent = (...times: number[]): BackendEvent => ({
  type: "pd_status",
  data: { samples: times.map(sample) },
});
const connected = {
  type: "status",
  data: { state: "connected" },
} as unknown as BackendEvent;

describe("Clear against status samples in flight (FR 4PJRVK)", () => {
  it("drops a pre-clear batch that arrives after the clear ends, and keeps later samples", () => {
    const pd = new PdCapture();
    const ticket = pd.beginClear();
    pd.endClear(0, ticket, 1000);
    pd.handleEvent(statusEvent(900, 1000));
    expect(pd.status.length).toBe(0);
    pd.handleEvent(statusEvent(1000, 1010, 1020));
    expect(pd.status.length).toBe(2);
  });

  it("a new session forgets the floor, as its time may restart", () => {
    const pd = new PdCapture();
    pd.clear(0, 5000);
    pd.handleEvent(connected);
    pd.handleEvent(statusEvent(10));
    expect(pd.status.length).toBe(1);
  });

  it("keeps every sample when the clear had no status yet", () => {
    const pd = new PdCapture();
    pd.clear(0, 0);
    pd.handleEvent(statusEvent(0, 5));
    expect(pd.status.length).toBe(2);
  });
});

describe("An opened capture's dropped count (FR 4PJRVK)", () => {
  it("shows in the dropped notice though the backend sent only the kept rows", () => {
    const pd = new PdCapture();
    pd.open([row(15), row(16)], [], 15);
    expect(pd.store.length).toBe(2);
    expect(pd.store.dropped).toBe(15);
    expect(pdDroppedText(pd.store.dropped)).toMatch(/Dropped the oldest 15 records/);
  });
});
