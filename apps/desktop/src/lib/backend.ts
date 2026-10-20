/** Shapes the Tauri backend sends to the UI (mirrors `src-tauri/src/{batch,worker,devices,ui_error}.rs`). */

import { Channel, invoke, isTauri } from "@tauri-apps/api/core";
import { MockBackend } from "./mockBackend";

/** The scripted demo backend, when the web build was made with `VITE_BACKEND=mock`. */
const mock: MockBackend | null = import.meta.env.VITE_BACKEND === "mock" ? new MockBackend() : null;

/** Whether a backend is there to talk to: the Tauri app, or the mock build. */
export function backendAvailable(): boolean {
  return mock !== null || isTauri();
}

/** One measurement. */
export interface Sample {
  t_ms: number;
  vbus_v: number;
  ibus_a: number;
  cc1_v: number;
  cc2_v: number;
  dp_v: number;
  dm_v: number;
}

/** A batch of samples plus the latest temperature (null when not reported). */
export interface Batch {
  samples: Sample[];
  temp_c: number | null;
}

export type ErrorKind =
  | "permission_denied"
  | "device_busy"
  | "timeout"
  | "protocol"
  | "disconnected"
  | "usage"
  | "internal";

/** A plain-language error. */
export interface UiError {
  kind: ErrorKind;
  message: string;
  hint: string | null;
}

const ERROR_KINDS: readonly ErrorKind[] = [
  "permission_denied",
  "device_busy",
  "timeout",
  "protocol",
  "disconnected",
  "usage",
  "internal",
];

function isUiError(value: unknown): value is UiError {
  if (typeof value !== "object" || value === null) return false;
  const v = value as Record<string, unknown>;
  return (
    typeof v.kind === "string" &&
    (ERROR_KINDS as readonly string[]).includes(v.kind) &&
    typeof v.message === "string" &&
    (v.hint === null || v.hint === undefined || typeof v.hint === "string")
  );
}

/** What the banner says about a failure that did not come from the backend. */
export const INTERNAL_MESSAGE = "Something went wrong inside the app.";

/** Turns a rejected invoke payload (a backend UiError) or any other thrown value into a
 * UiError. Anything else is a bug: its raw text goes to the console, never to the banner. */
export function toUiError(err: unknown): UiError {
  if (isUiError(err)) return { kind: err.kind, message: err.message, hint: err.hint ?? null };
  console.error("open-powerz: unexpected error", err);
  return { kind: "internal", message: INTERNAL_MESSAGE, hint: null };
}

/** A meter (or demo device) the UI can connect to. */
export interface DeviceListing {
  id: string;
  label: string;
  model: string;
  serial: string | null;
  bus: string | null;
  address: number | null;
  demo_scenario: string | null;
}

export interface ConnectedInfo {
  model: string;
  firmware_version: string;
  serial: string;
  rate_sps: number | null;
  rate_locked: boolean;
  session_id: number;
}

export type Status =
  | ({ state: "connected" } & ConnectedInfo)
  | { state: "rate"; rate_sps: number; rate_locked: boolean }
  | { state: "disconnected"; error: UiError | null };

export interface Gap {
  kind: "stream" | "outage";
  from_ms: number;
  to_ms: number;
}

/** Every event the backend sends over the session channel. */
export type BackendEvent =
  | { type: "batch"; data: Batch }
  | { type: "gap"; data: Gap }
  | { type: "status"; data: Status }
  | { type: "error"; data: UiError };

/** Lists the meters and demo devices available right now. */
export function listDevices(): Promise<DeviceListing[]> {
  if (mock) return mock.listDevices();
  return invoke<DeviceListing[]>("list_devices");
}

/** Subscribes to hotplug: `onChange` receives the full device list on every change.
 * Resolves to an unsubscribe function; the backend's watcher is replaced by the next
 * subscription and stops by itself once the window is gone. */
export async function watchDevices(
  onChange: (devices: DeviceListing[]) => void,
): Promise<() => void> {
  if (mock) return mock.watchDevices(onChange);
  let deliver = onChange;
  const channel = new Channel<DeviceListing[]>();
  channel.onmessage = (devices) => deliver(devices);
  await invoke("watch_devices", { onChange: channel });
  return () => {
    deliver = () => {};
  };
}

/** Opens a session on `deviceId`; every session event goes to `onEvent`. */
export async function connect(
  deviceId: string,
  rateSps: number,
  onEvent: (event: BackendEvent) => void,
): Promise<void> {
  if (mock) return mock.connect(deviceId, rateSps, onEvent);
  const channel = new Channel<BackendEvent>();
  channel.onmessage = onEvent;
  await invoke("connect", { deviceId, rateSps, onEvent: channel });
}

/** Ends the current session. */
export async function disconnect(): Promise<void> {
  if (mock) return mock.disconnect();
  await invoke("disconnect");
}

/** Changes the queue rate of the current session. */
export async function setRate(rateSps: number): Promise<void> {
  if (mock) return mock.setRate(rateSps);
  await invoke("set_rate", { rateSps });
}

/** App and protocol-library versions for the About dialog. */
export interface AppInfo {
  appVersion: string;
  protocolVersion: string;
}

/** Reads the app and protocol-library versions from the backend. */
export async function appInfo(): Promise<AppInfo> {
  if (mock) return mock.appInfo();
  const info = await invoke<{ app_version: string; protocol_version: string }>("app_info");
  return { appVersion: info.app_version, protocolVersion: info.protocol_version };
}
