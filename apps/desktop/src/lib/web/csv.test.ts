/**
 * FR QFTCX0, AC-QFTCX0.5: the in-browser CSV recorder writes the desktop recorder's
 * format. The expected strings are the golden output of `crates/powerz-record`
 * (`tests/record.rs` and the `sig8` / `format_elapsed` unit tests), restated here.
 */

import { describe, expect, it } from "vitest";
import { CSV_HEADER, CsvRecorder, formatElapsed, sig8 } from "./csv";

/** Wall-clock origin of the Rust recorder tests, ms since the Unix epoch. */
const UNIX0 = 1_790_000_000_000;

/** 5 V / 1 A, CC1 1.68 V, D+ 0.6 V (the Rust `five_one` record). */
function fiveOne(t_ms: number) {
  return { t_ms, vbus_v: 5, ibus_a: 1, cc1_v: 1.68, cc2_v: 0, dp_v: 0.6, dm_v: 0 };
}

const lines = (text: string) => text.split("\n").filter((l) => l.length > 0);

describe("CSV format helpers match powerz-record (AC-QFTCX0.5)", () => {
  it("has the vendor recorder header", () => {
    expect(CSV_HEADER).toBe("ElapsedTime,Unix,VBUS,IBUS,DP,DM,CC1,CC2,TEMP,CHARGE,ENERGY,PWR");
  });

  it("formats elapsed time as h:mm:ss.mmm with unpadded hours", () => {
    expect(formatElapsed(0)).toBe("0:00:00.000");
    expect(formatElapsed(7)).toBe("0:00:00.007");
    expect(formatElapsed(61_005)).toBe("0:01:01.005");
    expect(formatElapsed(3_723_456)).toBe("1:02:03.456");
    expect(formatElapsed(100 * 3_600_000 + 59_999)).toBe("100:00:59.999");
  });

  it("keeps eight significant digits as plain decimals", () => {
    expect(sig8(0)).toBe("0");
    expect(sig8(5)).toBe("5");
    expect(sig8(12.345678912)).toBe("12.345679");
    expect(sig8(-43.508 / 3_600_000)).toBe("-0.000012085556");
    expect(sig8(2.5e-12)).toBe("0.0000000000025");
    expect(sig8(123_456_789.4)).toBe("123456789");
    expect(sig8((1.0 * 0.1) / 3600.0)).toBe("0.000027777778");
    for (const x of [1e-7, -3.3e-9, 4.2e-15, 1.5e9]) expect(sig8(x)).not.toContain("e");
  });
});

describe("CsvRecorder rows match powerz-record (AC-QFTCX0.5)", () => {
  it("a new recorder holds only the header", () => {
    const r = new CsvRecorder();
    expect(r.rows).toBe(0);
    expect(r.text()).toBe(`${CSV_HEADER}\n`);
  });

  it("writes vendor columns with time and Unix from the first row", () => {
    const r = new CsvRecorder();
    r.push(fiveOne(10_000), UNIX0);
    // Later wall-clock readings are ignored: Unix = origin + device elapsed.
    r.push(fiveOne(10_100), UNIX0 + 9_999);
    expect(r.rows).toBe(2);
    const out = lines(r.text());
    expect(out).toEqual([
      CSV_HEADER,
      "0:00:00.000,1790000000.000,5,1,0.6,0,1.68,0,,0,0,5",
      `0:00:00.100,1790000000.100,5,1,0.6,0,1.68,0,,${sig8((1 * 0.1) / 3600)},${sig8((5 * 0.1) / 3600)},5`,
    ]);
    expect(r.text().endsWith("\n")).toBe(true);
  });

  it("counts charge and energy from the recording start", () => {
    const r = new CsvRecorder();
    r.push(fiveOne(3_600_000), UNIX0);
    r.push(fiveOne(3_603_600), UNIX0);
    const [, first, second] = lines(r.text()).map((l) => l.split(","));
    expect(first?.[0]).toBe("0:00:00.000");
    expect([first?.[9], first?.[10]]).toEqual(["0", "0"]);
    expect(second?.[0]).toBe("0:00:03.600");
    expect([second?.[9], second?.[10]]).toEqual(["0.001", "0.005"]);
    expect(second?.[1]).toBe("1790000003.600");
  });

  it("writes negative current and small line voltages as plain decimals, TEMP empty", () => {
    const r = new CsvRecorder();
    r.push(
      {
        t_ms: 0,
        vbus_v: 0.004021,
        ibus_a: -0.000059,
        cc1_v: 0.082,
        cc2_v: 0.029,
        dp_v: 0.045,
        dm_v: 0.044,
      },
      UNIX0,
    );
    const row = lines(r.text())[1]?.split(",") ?? [];
    expect(row).toHaveLength(12);
    expect(row.slice(2, 9)).toEqual([
      "0.004021",
      "-0.000059",
      "0.045",
      "0.044",
      "0.082",
      "0.029",
      "",
    ]);
    expect(row[11]).toBe(sig8(0.004021 * -0.000059));
    expect(lines(r.text())[1]).not.toContain("e");
  });

  it("writes TEMP when the row has one", () => {
    const r = new CsvRecorder();
    r.push(fiveOne(0), UNIX0, 31.25);
    expect(lines(r.text())[1]?.split(",")[8]).toBe("31.25");
  });

  it("a backwards step and a broken run add no charge", () => {
    const r = new CsvRecorder();
    r.push(fiveOne(50_000), UNIX0);
    r.push(fiveOne(53_600), UNIX0); // +3.6 s at 1 A: 0.001 Ah
    r.breakRun();
    r.push(fiveOne(60_000), UNIX0); // across the break: nothing
    const rows = lines(r.text())
      .slice(1)
      .map((l) => l.split(","));
    expect(rows.map((c) => c[9])).toEqual(["0", "0.001", "0.001"]);
    expect(rows[2]?.[0]).toBe("0:00:10.000");
  });
});
