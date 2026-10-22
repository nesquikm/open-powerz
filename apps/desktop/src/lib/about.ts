/** Text for the About dialog (AC-AH2AEK.8). */

import type { AppInfo } from "./backend";

export const ABOUT_DISCLAIMER =
  "open-powerz is unofficial open-source software and is not affiliated with ChargerLAB.";

/** The About dialog's lines: app version, protocol-library version, disclaimer. */
export function aboutLines(info: AppInfo): string[] {
  return [
    `open-powerz ${info.appVersion}`,
    `powerz-protocol ${info.protocolVersion}`,
    ABOUT_DISCLAIMER,
  ];
}
