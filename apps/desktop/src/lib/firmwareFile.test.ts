/**
 * View model for an inspected firmware file (FR V9SK53, AC .3): the
 * metadata rows and the English release notes the desktop app shows.
 */

import { describe, expect, it } from "vitest";
import type { FirmwareFileInfo } from "./backend";
import { firmwareFileRows, firmwareNotes, firmwareFileErrorText } from "./firmwareFile";

const info: FirmwareFileInfo = {
  vendor: "ChargerLab",
  container_version: "V1.0",
  model: "KM003C",
  version: "2.0.7",
  date: "2026.9.29",
  crc: "A175E4A0",
  size: 456896,
  release_notes_en: "1. Fix one.\r\n\n2. Fix two.  \n",
};

describe("firmwareFileRows", () => {
  it("lists model, version, date, size, CRC, container and vendor in order", () => {
    expect(firmwareFileRows(info)).toEqual([
      { label: "Model", value: "KM003C" },
      { label: "Version", value: "2.0.7" },
      { label: "Date", value: "2026.9.29" },
      { label: "Size", value: "456,896 bytes" },
      { label: "CRC-32", value: "A175E4A0" },
      { label: "Container", value: "V1.0" },
      { label: "Vendor", value: "ChargerLab" },
    ]);
  });

  it("shows a dash for an empty field", () => {
    const rows = firmwareFileRows({ ...info, date: "  " });
    expect(rows.find((r) => r.label === "Date")?.value).toBe("—");
  });
});

describe("firmwareNotes", () => {
  it("splits the English notes into trimmed, non-empty lines", () => {
    expect(firmwareNotes(info)).toEqual(["1. Fix one.", "2. Fix two."]);
  });

  it("is empty when the file carries no English notes", () => {
    expect(firmwareNotes({ ...info, release_notes_en: " \n " })).toEqual([]);
  });
});

describe("firmwareFileErrorText", () => {
  it("shows the backend's reason for a refused file", () => {
    expect(
      firmwareFileErrorText({
        kind: "usage",
        message: "This firmware file is for the KM002C, but the meter is a KM003C.",
        hint: null,
      }),
    ).toBe("This firmware file is for the KM002C, but the meter is a KM003C.");
  });

  it("falls back to the generic text for an unexpected error", () => {
    expect(firmwareFileErrorText(new Error("boom"))).toBe("Something went wrong inside the app.");
  });
});
