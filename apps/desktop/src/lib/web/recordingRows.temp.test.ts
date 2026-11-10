/**
 * FR 3B70S9: the recording rows type carries each row's TEMP.
 *
 * - AC-3B70S9.1: the `RecordingRows` sample type has a `temp_c` in degrees Celsius
 *   that is `number | null` (a type-level assertion: `bun run check` fails without it).
 * - AC-3B70S9.2: a CSV with TEMP on some rows and an empty TEMP on others, read through
 *   the wasm `readRecordingCsv` and the stepped `CsvRecording`, gives a number on the
 *   rows that have one and `null` on the others.
 *
 * The real built wasm module runs underneath (`bun run build:wasm`).
 */

import { beforeAll, describe, expect, expectTypeOf, it } from "vitest";
import type { MeterSample, PowerzWasm, RecordingRows } from "./driver";
import { loadWasm } from "./testWasm.test";

type RowSample = RecordingRows["samples"][number];

let w: PowerzWasm;

beforeAll(async () => {
  w = await loadWasm();
});

const CSV_HEADER = "ElapsedTime,Unix,VBUS,IBUS,DP,DM,CC1,CC2,TEMP,CHARGE,ENERGY,PWR";
const ROWS = [
  "0:00:00.000,1790000000.000000,5,1,0.6,0.01,1.68,0.002,25.5,0,0,5",
  "0:00:00.500,1790000000.500000,5.1,1.2,0.6,0.01,1.68,0.002,,0.0001,0.0005,6.12",
  "0:00:01.000,1790000001.000000,5.2,0.8,0.6,0.01,1.68,0.002,26.25,0.0002,0.001,4.16",
  "0:00:01.500,1790000001.500000,5.0,0.9,0.6,0.01,1.68,0.002,,0.0003,0.0015,4.5",
];
const EXPECTED_TEMPS: (number | null)[] = [25.5, null, 26.25, null];
const bytes = () => new TextEncoder().encode([CSV_HEADER, ...ROWS].join("\n") + "\n");

/** Each sample's `temp_c`, read through the typed field (no cast). */
const temps = (rows: RecordingRows): (number | null)[] => rows.samples.map((s) => s.temp_c);

describe("the recording rows type carries TEMP (AC-3B70S9.1)", () => {
  it("each sample has a temp_c that is a number or null", () => {
    expectTypeOf<RowSample>().toHaveProperty("temp_c");
    expectTypeOf<RowSample["temp_c"]>().toEqualTypeOf<number | null>();
  });

  it("the sample is still a meter sample", () => {
    expectTypeOf<RowSample>().toMatchTypeOf<MeterSample>();
  });
});

describe("the wasm reader gives TEMP per row (AC-3B70S9.2)", () => {
  it("readRecordingCsv: a number where the row has TEMP, null where it is empty", () => {
    const rows = w.readRecordingCsv(bytes());
    expect(rows.rows).toBe(ROWS.length);
    expect(temps(rows)).toEqual(EXPECTED_TEMPS);
    for (const [i, s] of rows.samples.entries()) {
      if (EXPECTED_TEMPS[i] === null) expect(s.temp_c).toBeNull();
      else expect(typeof s.temp_c).toBe("number");
    }
  });

  it("the stepped CsvRecording gives the same TEMP values", () => {
    const reader = new w.CsvRecording(bytes());
    while (reader.step(1)) {
      // one line per slice
    }
    const rows = reader.finish();
    expect(rows.rows).toBe(ROWS.length);
    expect(temps(rows)).toEqual(EXPECTED_TEMPS);
  });
});
