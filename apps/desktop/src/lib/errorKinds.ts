/** Every `ErrorKind` the backend sends, as a value: shared by `backend.ts` and the web
 * driver without the web chunk importing `backend.ts` (and Tauri) at run time. */

import type { ErrorKind } from "./backend";

export const ERROR_KINDS: readonly ErrorKind[] = [
  "permission_denied",
  "device_busy",
  "timeout",
  "protocol",
  "disconnected",
  "usage",
  "out_of_sync",
  "flash_needs_consent",
  "cancelled",
  "settings_not_saved",
  "internal",
];
