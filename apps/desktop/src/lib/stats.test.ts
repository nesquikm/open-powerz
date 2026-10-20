import { describe, expect, it } from "vitest";
import type { StatsSnapshot } from "./backend";
import { formatSigned, formatStatsElapsed, statsView } from "./stats";

const MINUS = "−";

function snapshot(over: Partial<StatsSnapshot> = {}): StatsSnapshot {
  return {
    elapsed_ms: 3_723_456,
    samples: 1234,
    charge_ah: -0.12,
    energy_wh: -0.6,
    vbus: { min: 4.98, max: 5.0213, avg: 5.0004 },
    ibus: { min: -0.25, max: 0.031, avg: -0.12 },
    power: { min: -1.25, max: 0.155, avg: -0.6 },
    ...over,
  };
}

describe("formatSigned (AC-AH2AEJ.6)", () => {
  it("uses the true minus sign for negative values", () => {
    expect(formatSigned(-0.12, 3, "Ah")).toBe(`${MINUS}0.120 Ah`);
    expect(formatSigned(-12.5, 3, "Wh")).toBe(`${MINUS}12.500 Wh`);
  });

  it("shows positive values without a sign", () => {
    expect(formatSigned(0.12, 3, "Ah")).toBe("0.120 Ah");
    expect(formatSigned(5, 3, "V")).toBe("5.000 V");
  });

  it("never shows a negative zero", () => {
    expect(formatSigned(-0.0001, 3, "Ah")).toBe("0.000 Ah");
    expect(formatSigned(-0, 3, "Wh")).toBe("0.000 Wh");
  });

  it("never uses the ASCII hyphen as a minus", () => {
    expect(formatSigned(-3.5, 3, "W")).not.toContain("-");
  });
});

describe("formatStatsElapsed", () => {
  it("formats elapsed time as h:mm:ss with unpadded hours", () => {
    expect(formatStatsElapsed(0)).toBe("0:00:00");
    expect(formatStatsElapsed(65_999)).toBe("0:01:05");
    expect(formatStatsElapsed(3_723_456)).toBe("1:02:03");
    expect(formatStatsElapsed(36_000_000 + 61_000)).toBe("10:01:01");
  });
});

describe("statsView (AC-AH2AEJ.4, AC-AH2AEJ.6)", () => {
  it("shows every value with its unit and signed charge and energy", () => {
    const v = statsView(snapshot());
    expect(v.elapsed).toBe("1:02:03");
    expect(v.charge).toBe(`${MINUS}0.120 Ah`);
    expect(v.energy).toBe(`${MINUS}0.600 Wh`);
    expect(v.vbus).toEqual({ min: "4.980 V", max: "5.021 V", avg: "5.000 V" });
    expect(v.ibus).toEqual({
      min: `${MINUS}0.250 A`,
      max: "0.031 A",
      avg: `${MINUS}0.120 A`,
    });
    expect(v.power).toEqual({
      min: `${MINUS}1.250 W`,
      max: "0.155 W",
      avg: `${MINUS}0.600 W`,
    });
  });

  it("shows positive charge and energy for a normal load", () => {
    const v = statsView(snapshot({ charge_ah: 1.5, energy_wh: 7.5 }));
    expect(v.charge).toBe("1.500 Ah");
    expect(v.energy).toBe("7.500 Wh");
  });

  it("shows zeros and dashes, still with units, before any sample", () => {
    const empty = statsView(
      snapshot({
        elapsed_ms: 0,
        samples: 0,
        charge_ah: 0,
        energy_wh: 0,
        vbus: null,
        ibus: null,
        power: null,
      }),
    );
    expect(empty.elapsed).toBe("0:00:00");
    expect(empty.charge).toBe("0.000 Ah");
    expect(empty.energy).toBe("0.000 Wh");
    expect(empty.vbus).toEqual({ min: "— V", max: "— V", avg: "— V" });
    expect(empty.ibus).toEqual({ min: "— A", max: "— A", avg: "— A" });
    expect(empty.power).toEqual({ min: "— W", max: "— W", avg: "— W" });
    expect(statsView(null)).toEqual(empty);
  });
});
