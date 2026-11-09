/**
 * FR QFTCWZ: the browser edition's transport seam. One interface, three
 * implementations: WebUSB (`webusb.ts`), WebHID (`webhid.ts`) and the demo meter
 * (`demoTransport.ts`). Every failure rejects with a `UiError`-shaped value.
 */

import type { ErrorKind, UiError } from "../backend";

/** A request/reply channel to one meter (or the demo meter). */
export interface WebTransport {
  /** Send `frame` and resolve with the reply; rejects `timeout` after `timeoutMs`. */
  transact(frame: Uint8Array, timeoutMs: number): Promise<Uint8Array>;
  /** The next reply without writing anything; rejects `timeout` after `timeoutMs`. */
  read(timeoutMs: number): Promise<Uint8Array>;
  /** Unstick the pipe (clear halts, discard stale IN data); optional per transport. */
  recover?(): Promise<void>;
  /** Release the device; later transfers reject as `disconnected`. */
  close(): Promise<void>;
}

/** A `UiError` value for a transport failure. */
export function transportError(
  kind: ErrorKind,
  message: string,
  hint: string | null = null,
): UiError {
  return { kind, message, hint };
}

export const disconnectedError = (): UiError =>
  transportError("disconnected", "The meter is disconnected.", "Plug it in and connect again.");

/** True for the `NotFoundError` the browser raises when the device is already gone. */
export function isNotFoundError(e: unknown): boolean {
  return typeof e === "object" && e !== null && (e as { name?: unknown }).name === "NotFoundError";
}

export const timeoutError = (): UiError =>
  transportError("timeout", "The meter did not answer in time.", null);

/** A request the app refuses as unsafe, in the desktop app's words. */
export const notSafeError = (): UiError =>
  transportError("usage", "The app refused a request that is not safe.", null);

/** `work` raced against a `timeoutMs` timer that rejects as `timeout`. */
export function withTimeout<T>(work: Promise<T>, timeoutMs: number): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const timer = setTimeout(() => reject(timeoutError()), timeoutMs);
    work.then(
      (v) => {
        clearTimeout(timer);
        resolve(v);
      },
      (e: unknown) => {
        clearTimeout(timer);
        reject(e);
      },
    );
  });
}
