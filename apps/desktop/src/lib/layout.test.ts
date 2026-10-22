import { describe, expect, it } from "vitest";
import { LAYOUT_BREAKPOINT_PX, layoutFor } from "./layout";

// No horizontal scroll and no readout overflow or overlap at 900×600 … 2560×1440, disconnected
// and streaming, are checked in the browser by e2e/ui/layout.spec.ts; this checks the
// breakpoint the layout switches on.
describe("layoutFor (AC-AH2AEK.1)", () => {
  it("breaks at 1200 px", () => {
    expect(LAYOUT_BREAKPOINT_PX).toBe(1200);
  });

  it("puts the stats panel under the chart below 1200 px", () => {
    expect(layoutFor(900)).toBe("stacked");
    expect(layoutFor(1199)).toBe("stacked");
  });

  it("puts the stats panel beside the chart from 1200 px", () => {
    expect(layoutFor(1200)).toBe("wide");
    expect(layoutFor(2560)).toBe("wide");
  });
});
