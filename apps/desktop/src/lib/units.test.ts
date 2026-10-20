import { describe, expect, it } from "vitest";
import { formatVolts } from "./units";

describe("formatVolts", () => {
  it("renders three decimals and the unit", () => {
    expect(formatVolts(5)).toBe("5.000 V");
    expect(formatVolts(20.0049)).toBe("20.005 V");
  });
});
