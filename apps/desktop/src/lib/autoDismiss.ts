/** How long an information notice stays before it dismisses itself (AC-AKNVND.8). */
export const NOTICE_TIMEOUT_MS = 15_000;

/** Why the countdown is held: the pointer is over the notices, or focus is in them. */
export type HoldReason = "pointer" | "focus";

/**
 * A notice's self-dismiss countdown (AC-AKNVND.8). While any hold is on the countdown
 * stops; when the last hold is released it starts again from the full timeout, so a
 * notice someone was reading never vanishes under their eyes.
 */
export class AutoDismiss {
  #onDismiss: () => void;
  #timer: ReturnType<typeof setTimeout> | null = null;
  #running = false;
  #holds = new Set<HoldReason>();

  constructor(onDismiss: () => void) {
    this.#onDismiss = onDismiss;
  }

  /** (Re)start the countdown for a new notice. */
  start(): void {
    this.#running = true;
    this.#arm();
  }

  /** The notice is gone: no countdown. */
  stop(): void {
    this.#running = false;
    this.#clear();
  }

  hold(reason: HoldReason): void {
    this.#holds.add(reason);
    this.#clear();
  }

  release(reason: HoldReason): void {
    this.#holds.delete(reason);
    if (this.#running) this.#arm();
  }

  #arm(): void {
    this.#clear();
    if (this.#holds.size > 0) return;
    this.#timer = setTimeout(() => {
      this.#timer = null;
      this.#running = false;
      this.#onDismiss();
    }, NOTICE_TIMEOUT_MS);
  }

  #clear(): void {
    if (this.#timer !== null) clearTimeout(this.#timer);
    this.#timer = null;
  }
}
