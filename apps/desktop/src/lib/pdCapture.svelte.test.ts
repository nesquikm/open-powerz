/**
 * FR TGE83C (AC .1, .2, .7): the PD view's capture state. It takes the backend's
 * `pd_state` / `pd_rows` events off the session channel, keeps the rows in its store,
 * and says when Start and Stop are available and, when they are not, why.
 */

import { describe, expect, it } from "vitest";
import type { BackendEvent, PdRow } from "./backend";
import { PdCapture } from "./pdCapture.svelte";

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
      serial: "SIM",
      rate_sps: 50,
      rate_locked: false,
      session_id: 2,
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
        serial: "SIM",
        rate_sps: 50,
        rate_locked: false,
        session_id: 1,
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
