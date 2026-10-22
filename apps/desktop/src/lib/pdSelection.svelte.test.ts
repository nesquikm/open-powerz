/**
 * FR TGE83D (AC .2, .3): the PD selection shared by the packet table and the power
 * chart. A marker click selects from the chart (the app then shows the PD analyzer tab
 * and the table scrolls to the row); a table row selects from the table (the chart moves
 * its playhead there). `seq` counts the selections, so selecting the same record again
 * still scrolls or moves the view.
 */

import { describe, expect, it } from "vitest";
import type { BackendEvent } from "./backend";
import { PdCapture } from "./pdCapture.svelte";
import { PdSelection } from "./pdSelection.svelte";

describe("PdSelection (AC-TGE83D.2, AC-TGE83D.3)", () => {
  it("starts with nothing selected", () => {
    const s = new PdSelection();
    expect(s.index).toBeNull();
    expect(s.origin).toBeNull();
  });

  it("a marker click selects its record from the chart", () => {
    const s = new PdSelection();
    s.select(12, "chart");
    expect(s.index).toBe(12);
    expect(s.origin).toBe("chart");
  });

  it("a table row selects its record from the table", () => {
    const s = new PdSelection();
    s.select(12, "chart");
    s.select(3, "table");
    expect(s.index).toBe(3);
    expect(s.origin).toBe("table");
  });

  it("selecting the same record again is a new selection", () => {
    const s = new PdSelection();
    s.select(5, "chart");
    const first = s.seq;
    s.select(5, "chart");
    expect(s.seq).toBeGreaterThan(first);
    expect(s.index).toBe(5);
  });

  it("clear drops the selection", () => {
    const s = new PdSelection();
    s.select(5, "table");
    s.clear();
    expect(s.index).toBeNull();
    expect(s.origin).toBeNull();
  });
});

describe("PdSelection follows the capture's clears", () => {
  it("a Clear drops the selection, also with no PD view mounted", () => {
    const pd = new PdCapture();
    const s = new PdSelection(pd);
    s.select(7, "chart");
    pd.clear();
    expect(s.index).toBeNull();
    expect(s.origin).toBeNull();
  });

  it("a new session drops the selection", () => {
    const pd = new PdCapture();
    const s = new PdSelection(pd);
    s.select(3, "table");
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
    pd.handleEvent(connected);
    expect(s.index).toBeNull();
  });

  it("a selection made after the clear holds", () => {
    const pd = new PdCapture();
    const s = new PdSelection(pd);
    s.select(3, "table");
    pd.clear();
    s.select(0, "chart");
    expect(s.index).toBe(0);
    expect(s.origin).toBe("chart");
  });
});
