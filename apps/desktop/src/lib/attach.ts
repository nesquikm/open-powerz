import type { AttachedSession, BackendEvent } from "./backend";

/**
 * Picks the running session up on load (FR VEDDFD). The backend hands the
 * session's events to the new page before the snapshot reaches it, and those
 * events are newer than the snapshot, so they are held and passed to
 * `onEvent` only after `adopt` has applied it (or after the attach failed or
 * found no session).
 */
export async function attachSession(
  attach: (onEvent: (event: BackendEvent) => void) => Promise<AttachedSession | null>,
  onEvent: (event: BackendEvent) => void,
  adopt: (snapshot: AttachedSession) => void,
): Promise<void> {
  let held: BackendEvent[] | null = [];
  try {
    const snapshot = await attach((event) => {
      if (held) held.push(event);
      else onEvent(event);
    });
    if (snapshot) adopt(snapshot);
  } catch (e) {
    // Start disconnected, as with no session to pick up.
    console.warn("open-powerz: attach failed", e);
  } finally {
    const queued = held ?? [];
    held = null;
    for (const event of queued) onEvent(event);
  }
}
