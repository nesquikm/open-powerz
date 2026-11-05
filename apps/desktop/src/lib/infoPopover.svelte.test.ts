// FR CW66T1, AC .5: one shared store holds the open popover, so only one is open at a time.
import { beforeEach, describe, expect, it } from "vitest";
import { infoPopover } from "./infoPopover.svelte";

describe("AC-CW66T1.5: only one popover is open at a time", () => {
  beforeEach(() => infoPopover.close());

  it("starts with none open", () => {
    expect(infoPopover.openKey).toBeNull();
  });

  it("opening another closes the first", () => {
    infoPopover.open("info-section.chart");
    expect(infoPopover.openKey).toBe("info-section.chart");
    infoPopover.open("term-term.vbus-1");
    expect(infoPopover.openKey).toBe("term-term.vbus-1");
  });

  it("a second press of the same trigger closes it; another trigger's press swaps", () => {
    infoPopover.toggle("a");
    expect(infoPopover.openKey).toBe("a");
    infoPopover.toggle("b");
    expect(infoPopover.openKey).toBe("b");
    infoPopover.toggle("b");
    expect(infoPopover.openKey).toBeNull();
  });

  it("close() closes whichever is open", () => {
    infoPopover.open("a");
    infoPopover.close();
    expect(infoPopover.openKey).toBeNull();
  });
});
