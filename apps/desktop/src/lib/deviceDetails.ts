/**
 * Device details and firmware warnings for the connected meter (FR V9SK52).
 * The backend decides which warnings apply; this module only words them.
 */

import type { ConnectedInfo, DeviceListing, DeviceMode, FirmwareWarningCode } from "./backend";

/** The mode in plain words. */
export function modeLabel(mode: DeviceMode): string {
  switch (mode) {
    case "application":
      return "Application";
    case "bootloader":
      return "Bootloader";
    default:
      return "Unknown";
  }
}

/** One label / value row of the device details. */
export interface DetailRow {
  label: string;
  value: string;
}

/** The update check in plain words: unavailable when HEAD was not read, and never
 * "update required" until that meaning is confirmed on a meter (AC-V9SK52.3). */
function updateCheckLabel(updateRequired: boolean | null): string {
  if (updateRequired === null) return "unavailable";
  return updateRequired ? "not recognised" : "up to date";
}

/** The device details rows of the connected meter; a missing value shows a dash,
 * and each firmware-status problem gets its own row. */
export function deviceDetailsRows(info: ConnectedInfo): DetailRow[] {
  const orDash = (s: string | undefined): string => (s === undefined || s.trim() === "" ? "—" : s);
  return [
    { label: "Model", value: orDash(info.model) },
    { label: "Mode", value: modeLabel(info.mode) },
    { label: "Firmware", value: orDash(info.firmware_version) },
    { label: "Bootloader", value: orDash(info.bootloader_version) },
    { label: "Serial", value: orDash(info.serial) },
    { label: "Update check", value: updateCheckLabel(info.update_required) },
    ...info.firmware_problems.map((value) => ({ label: "Firmware status", value })),
  ];
}

/** The text of one firmware warning for the connected meter. */
export function firmwareWarningText(code: FirmwareWarningCode, info: ConnectedInfo): string {
  const fw = info.firmware_version;
  switch (code) {
    case "too_old":
      return `Firmware ${fw} is older than ${info.min_supported_firmware}, the oldest supported version. Update the meter.`;
    case "outdated":
      return `Firmware ${fw} is older than ${info.known_latest_firmware}, the newest version known.`;
  }
}

/** The picked entry can be connected: anything but a meter in bootloader mode. */
export function canConnectTo(device: DeviceListing | null): boolean {
  return device !== null && device.mode !== "bootloader";
}
