/**
 * View model for an inspected firmware file (FR V9SK53, AC .3): the metadata rows and
 * the English release notes the desktop app shows.
 */

import { toUiError, type FirmwareFileInfo } from "./backend";

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

/** Model, version, date, size, CRC-32, container version and vendor, in that order; an
 * empty field shows a dash. */
export function firmwareFileRows(info: FirmwareFileInfo): FirmwareFileRow[] {
  return [
    { label: "Model", value: text(info.model) },
    { label: "Version", value: text(info.version) },
    { label: "Date", value: text(info.date) },
    { label: "Size", value: `${info.size.toLocaleString("en-US")} bytes` },
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

/** The text shown when inspecting a file failed: the backend's reason, or the generic
 * text for an unexpected error. */
export function firmwareFileErrorText(err: unknown): string {
  return toUiError(err).message;
}
