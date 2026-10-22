import { describe, expect, it } from "vitest";
import { ABOUT_DISCLAIMER, aboutLines } from "./about";

describe("About dialog (AC-AH2AEK.8)", () => {
  it("states that the app is unofficial and not affiliated with ChargerLAB", () => {
    expect(ABOUT_DISCLAIMER).toMatch(/unofficial/i);
    expect(ABOUT_DISCLAIMER).toMatch(/not affiliated with ChargerLAB/);
  });

  it("shows the app and protocol-library versions", () => {
    const lines = aboutLines({ appVersion: "0.2.0", protocolVersion: "0.1.5" });
    expect(lines).toContain("open-powerz 0.2.0");
    expect(lines).toContain("powerz-protocol 0.1.5");
    expect(lines).toContain(ABOUT_DISCLAIMER);
  });
});
