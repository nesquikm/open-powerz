/**
 * FR C6VR9K, AC .5: the overview envelope's per-frame column loop is capped.
 *
 * API pinned here: `bridgeColumns(lo, hi, blocked, cap?)` visits at most `cap` columns
 * (it asks `blocked` about no more than `cap` of them) and leaves every column past them
 * as it was. Without `cap` it bridges every column, as before. The strip passes
 * `drawLoopCap(width, height)` (`./drawCap`).
 */

import { describe, expect, it } from "vitest";
import { bridgeColumns } from "./stripEnvelope";

function sparse(cols: number, every: number): { lo: Float64Array; hi: Float64Array } {
  const lo = new Float64Array(cols).fill(Infinity);
  const hi = new Float64Array(cols).fill(-Infinity);
  for (let c = 0; c < cols; c += every) {
    lo[c] = c;
    hi[c] = c;
  }
  return { lo, hi };
}

describe("bridgeColumns' cap (AC-C6VR9K.5)", () => {
  it("AC-C6VR9K.5: visits at most `cap` columns and leaves the rest alone", () => {
    const { lo, hi } = sparse(10_000, 10);
    let asked = 0;
    bridgeColumns(
      lo,
      hi,
      () => {
        asked++;
        return false;
      },
      100,
    );
    expect(asked).toBeLessThanOrEqual(100);
    // Columns well past the cap: untouched (the empty ones stay empty).
    for (let c = 200; c < 10_000; c++) {
      if (c % 10 === 0) {
        expect(lo[c]).toBe(c);
        expect(hi[c]).toBe(c);
      } else {
        expect(lo[c], `column ${c}`).toBe(Infinity);
        expect(hi[c], `column ${c}`).toBe(-Infinity);
      }
    }
    // Columns inside the cap are bridged as before.
    expect(hi[5]! >= lo[5]!).toBe(true);
  });

  it("without a cap, bridges every column as before", () => {
    const { lo, hi } = sparse(1_000, 10);
    bridgeColumns(lo, hi, () => false);
    for (let c = 0; c <= 990; c++) expect(hi[c]! >= lo[c]!, `column ${c}`).toBe(true);
  });
});
