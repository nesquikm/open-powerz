/**
 * FR JQ971R (AC .2, .3): a PD capture that Detect stopped says so. The backend's
 * capture-stopped event carries `reason: "trigger"` when a trigger session stopped the
 * capture; the store keeps a notice from it (shown in the analyzer's status area) until
 * the next start, a disconnect or another meter shown. A stop the user asked for shows
 * no notice. The captured rows stay.
 */

import { describe, expect, it } from "vitest";
import type { BackendEvent, PdRow } from "./backend";
import { PD_TRIGGER_STOP_NOTICE, PdCapture } from "./pdCapture.svelte";

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

const NOTICE =
  "The PD capture stopped when Detect opened a trigger session. Start it again once you are done in the Trigger tab.";

const started: BackendEvent = { type: "pd_state", data: { capturing: true } };
const stopped: BackendEvent = { type: "pd_state", data: { capturing: false } };
const stoppedByTrigger = {
  type: "pd_state",
  data: { capturing: false, reason: "trigger" },
} as BackendEvent;

/** A capture that ran, captured three rows, and was stopped by a trigger session. */
function stoppedByDetect(): PdCapture {
  const pd = new PdCapture();
  pd.handleEvent(started);
  pd.handleEvent({ type: "pd_rows", data: { rows: [row(0), row(1), row(2)] } });
  pd.handleEvent(stoppedByTrigger);
  return pd;
}

describe("the trigger-stop notice (AC-JQ971R.2)", () => {
  it("has the exact text the FR names", () => {
    expect(PD_TRIGGER_STOP_NOTICE).toBe(NOTICE);
  });

  it("shows after a stop with the reason trigger, and the rows stay", () => {
    const pd = stoppedByDetect();
    expect(pd.capturing).toBe(false);
    expect(pd.notice).toBe(NOTICE);
    expect(pd.store.length).toBe(3);
  });

  it("is absent before any stop", () => {
    const pd = new PdCapture();
    expect(pd.notice).toBeNull();
    pd.handleEvent(started);
    expect(pd.notice).toBeNull();
  });
});

describe("when the notice goes away (AC-JQ971R.3)", () => {
  it("a stop the user asked for shows no notice", () => {
    const pd = new PdCapture();
    pd.handleEvent(started);
    pd.handleEvent(stopped);
    expect(pd.notice).toBeNull();
  });

  it("goes away when a capture starts again", () => {
    const pd = stoppedByDetect();
    pd.handleEvent(started);
    expect(pd.notice).toBeNull();
    expect(pd.capturing).toBe(true);
  });

  it("goes away when the meter disconnects", () => {
    const pd = stoppedByDetect();
    pd.handleEvent({
      type: "status",
      data: { state: "disconnected", error: null },
    } as BackendEvent);
    expect(pd.notice).toBeNull();
  });

  it("goes away when the meter waits for its replug", () => {
    const pd = stoppedByDetect();
    pd.handleEvent({ type: "status", data: { state: "waiting", serial: "S" } } as BackendEvent);
    expect(pd.notice).toBeNull();
  });

  it("goes away when another meter is shown", () => {
    const pd = stoppedByDetect();
    const other = new PdCapture();
    pd.swapWith(other);
    expect(pd.notice).toBeNull();
    // The parked capture does not bring it back when the first meter returns.
    pd.swapWith(other);
    expect(pd.notice).toBeNull();
  });

  it("a later user stop after a restart shows no notice", () => {
    const pd = stoppedByDetect();
    pd.handleEvent(started);
    pd.handleEvent(stopped);
    expect(pd.notice).toBeNull();
  });
});
