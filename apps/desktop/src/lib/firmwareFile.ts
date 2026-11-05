/**
 * View model for an inspected firmware file (FR V9SK53, AC .3): the metadata rows and
 * the English release notes the desktop app shows.
 */

import type { FirmwareFileInfo } from "./backend";
import { formatCount } from "./format";

/** One labelled metadata row. */
export interface FirmwareFileRow {
  label: string;
  value: string;
}

const DASH = "—";

function text(value: string): string {
  const t = value.trim();
  return t === "" ? DASH : t;
}

function bytes(n: number): string {
  return `${formatCount(n)} bytes`;
}

/** The "Sent to the meter" row when the file stores bytes past its declared image
 * (AC-VQCKC6.2); none when the two agree. The container parser refuses a file that
 * stores fewer bytes than its image, so only "more" can occur. */
function sentRows(info: FirmwareFileInfo): FirmwareFileRow[] {
  const extra = info.stored_size - info.size;
  if (extra <= 0) return [];
  return [
    {
      label: "Sent to the meter",
      value: `${bytes(info.stored_size)} (${formatCount(extra)} more than the image; the update sends all the file stores)`,
    },
  ];
}

/** Model, version, date, size, CRC-32, container version and vendor, in that order; an
 * empty field shows a dash. A "Sent to the meter" row follows Size when the stored byte
 * count differs from the declared image size. */
export function firmwareFileRows(info: FirmwareFileInfo): FirmwareFileRow[] {
  return [
    { label: "Model", value: text(info.model) },
    { label: "Version", value: text(info.version) },
    { label: "Date", value: text(info.date) },
    { label: "Size", value: bytes(info.size) },
    ...sentRows(info),
    { label: "CRC-32", value: text(info.crc) },
    { label: "Container", value: text(info.container_version) },
    { label: "Vendor", value: text(info.vendor) },
  ];
}

/** The English release notes as trimmed, non-empty lines. */
export function firmwareNotes(info: FirmwareFileInfo): string[] {
  return info.release_notes_en
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line !== "");
}

/** Whether every note carries its own number ("1. ", "2、", "3) "), so bullets would be
 * a second marker. A version such as "2.0.7 was released." is not a number. */
export function notesNumbered(notes: string[]): boolean {
  return notes.length > 0 && notes.every((line) => /^\d+[.)、](?:\s+\S|[^\s\d])/.test(line));
}
