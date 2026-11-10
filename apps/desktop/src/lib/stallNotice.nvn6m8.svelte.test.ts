/** FR NVN6M8: the meter's view says when the meter stopped sending samples.
 *
 * - AC-NVN6M8.1 / AC-NVN6M8.4: a `stall` event from either backend (web edition or
 *   desktop worker) sets the session's `stalled` flag, and the meter's view shows the
 *   one notice text while it is set.
 * - AC-NVN6M8.2: `{ stalled: false }` (samples came back) takes the notice away; so does
 *   the end of the session. */

import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import type { BackendEvent } from "./backend";
import { Session } from "./session.svelte";
import { STALL_NOTICE } from "./stallNotice";

const TEXT =
  "The meter stopped sending samples. Another program may be polling it, such as powerz watch over the serial port; the stream comes back when it stops.";

function connected(): BackendEvent {
  return {
    type: "status",
    data: {
      state: "connected",
      model: "KM003C",
      firmware_version: "2.0.6",
      bootloader_version: "",
      mode: "application",
      warnings: [],
      update_required: false,
      firmware_problems: [],
      serial: "12345678",
      rate_sps: 50,
      rate_locked: false,
      session_id: 1,
      min_supported_firmware: "1.7.0",
      known_latest_firmware: "2.0.7",
    },
  };
}

const stall = (stalled: boolean): BackendEvent => ({ type: "stall", data: { stalled } });

function streaming(): Session {
  const s = new Session();
  s.beginConnect();
  s.handleEvent(connected());
  return s;
}

describe("the stall notice text (AC-NVN6M8.1)", () => {
  it("nvn6m8: says the meter stopped sending samples and why", () => {
    expect(STALL_NOTICE).toBe(TEXT);
  });
});

describe("the session's stalled flag (AC-NVN6M8.1, AC-NVN6M8.2, AC-NVN6M8.4)", () => {
  it("nvn6m8: is off for a streaming session", () => {
    expect(streaming().stalled).toBe(false);
  });

  it("nvn6m8: a stall event sets it, samples coming back clear it", () => {
    const s = streaming();
    s.handleEvent(stall(true));
    expect(s.stalled).toBe(true);
    // The session keeps streaming: the stall is no error and no state change.
    expect(s.state).toBe("streaming");
    expect(s.error).toBeNull();
    s.handleEvent(stall(false));
    expect(s.stalled).toBe(false);
  });

  it("nvn6m8: the session's end takes it away", () => {
    const s = streaming();
    s.handleEvent(stall(true));
    s.handleEvent({ type: "status", data: { state: "disconnected", error: null } });
    expect(s.stalled).toBe(false);
  });

  it("nvn6m8: a new connect starts without it", () => {
    const s = streaming();
    s.handleEvent(stall(true));
    s.beginConnect();
    expect(s.stalled).toBe(false);
  });
});

const SRC = fileURLToPath(new URL("..", import.meta.url));

function svelteFiles(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    if (name === "node_modules" || name === "wasm") return [];
    const path = join(dir, name);
    if (statSync(path).isDirectory()) return svelteFiles(path);
    return name.endsWith(".svelte") ? [path] : [];
  });
}

describe("the meter's view shows the notice (AC-NVN6M8.1, AC-NVN6M8.4)", () => {
  it("nvn6m8: a meter view component shows STALL_NOTICE while the session is stalled", () => {
    const users = svelteFiles(SRC).filter((f) => {
      const text = readFileSync(f, "utf8");
      return (
        /from\s+["'][./]*(?:lib\/)?stallNotice(?:\.ts)?["']/.test(text) && /STALL_NOTICE/.test(text)
      );
    });
    expect(users.length, "a .svelte file that imports STALL_NOTICE").toBeGreaterThan(0);
    const shown = users.some((f) => /\.stalled\b/.test(readFileSync(f, "utf8")));
    expect(shown, "the notice is tied to the session's stalled flag").toBe(true);
  });
});
