import { describe, expect, it } from "vitest";
import {
  defaultPdCaptureName,
  pdImportNoticeText,
  pdOpenDamageText,
  pdSaveNoticeText,
  recordsText,
} from "./pdSave";
import type { PdOpened } from "./backend";

describe("PD capture save (FR TGE83E)", () => {
  it("proposes a timestamped .pdcap name in local time", () => {
    expect(defaultPdCaptureName(new Date(2026, 9, 2, 7, 5, 9))).toBe(
      "powerz-pd-20261002-070509.pdcap",
    );
  });

  it("counts records in the singular and with separators", () => {
    expect(recordsText(1)).toBe("1 record");
    expect(recordsText(0)).toBe("0 records");
    expect(recordsText(1234)).toBe("1,234 records");
  });

  it("names the records and the file in the notice", () => {
    expect(pdSaveNoticeText({ path: "/tmp/a.pdcap", rows: 12 })).toBe(
      "Saved 12 records to /tmp/a.pdcap",
    );
  });
});

describe("PD capture open and import notices (FR TGE83E, AC .3 / AC .6)", () => {
  const opened = (rows: number, damaged_line: number | null): PdOpened =>
    ({
      path: "/tmp/cap.pdcap",
      rows: Array.from({ length: rows }),
      status: [],
      damaged_line,
    }) as unknown as PdOpened;

  it("names the records loaded and the line where a damaged file stopped", () => {
    expect(pdOpenDamageText(opened(120, 123))).toBe(
      "Loaded 120 records; the file is damaged at line 123",
    );
  });

  it("names the line of a file damaged on its first entry (AC-4PJRVK.6)", () => {
    expect(pdOpenDamageText(opened(0, 2))).toBe("Loaded 0 records; the file is damaged at line 2");
  });

  it("says nothing about a whole file", () => {
    expect(pdOpenDamageText(opened(5, null))).toBeNull();
  });

  it("says how many records came from the vendor log", () => {
    expect(pdImportNoticeText({ ...opened(1, null), path: "/tmp/log.sqlite" })).toBe(
      "Imported 1 record from the vendor log /tmp/log.sqlite",
    );
  });
});

describe("what a vendor log import left out (FR TGE83E, AC .6)", () => {
  const imported = (
    rows: number,
    oversized: number,
    truncated: boolean,
    skipped = 0,
  ): PdOpened => ({
    path: "/tmp/log.sqlite",
    rows: Array.from({ length: rows }) as PdOpened["rows"],
    status: [],
    dropped: 0,
    damaged_line: null,
    oversized,
    skipped,
    truncated,
  });

  it("says when only the first rows of a big log were imported", () => {
    expect(pdImportNoticeText(imported(3, 0, true))).toBe(
      "Imported 3 records from the vendor log /tmp/log.sqlite; the log has more rows than the import takes, only the first ones were imported",
    );
  });

  it("says how many rows with oversized record bytes were skipped", () => {
    expect(pdImportNoticeText(imported(2, 1, false))).toBe(
      "Imported 2 records from the vendor log /tmp/log.sqlite; 1 row with oversized record bytes was skipped",
    );
    expect(pdImportNoticeText(imported(2, 1200, false))).toBe(
      "Imported 2 records from the vendor log /tmp/log.sqlite; 1,200 rows with oversized record bytes were skipped",
    );
  });

  it("says how many rows without a whole record were skipped (AC-4PJRVK.6)", () => {
    expect(pdImportNoticeText(imported(2, 0, false, 1))).toBe(
      "Imported 2 records from the vendor log /tmp/log.sqlite; 1 row without a whole record was skipped",
    );
    expect(pdImportNoticeText(imported(2, 0, false, 1200))).toBe(
      "Imported 2 records from the vendor log /tmp/log.sqlite; 1,200 rows without a whole record were skipped",
    );
  });

  it("names both kinds of skipped rows (AC-4PJRVK.6)", () => {
    expect(pdImportNoticeText(imported(2, 3, false, 4))).toBe(
      "Imported 2 records from the vendor log /tmp/log.sqlite; 3 rows with oversized record bytes were skipped; 4 rows without a whole record were skipped",
    );
  });

  it("says nothing of skipped rows when there are none (AC-4PJRVK.6)", () => {
    expect(pdImportNoticeText(imported(2, 0, false, 0))).toBe(
      "Imported 2 records from the vendor log /tmp/log.sqlite",
    );
  });
});
