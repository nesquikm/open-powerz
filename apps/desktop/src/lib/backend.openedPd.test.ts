/** AC-AXHK2K.5: the page's command that releases an opened PD file goes to the Tauri
 * backend. The slot is the app's, not a meter's, so it names no device. */

import { beforeEach, describe, expect, it, vi } from "vitest";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => {
  class Channel {
    onmessage: (msg: unknown) => void = () => {};
  }
  return { invoke, Channel, isTauri: () => true };
});

import { pdReleaseOpened } from "./backend";

beforeEach(() => {
  invoke.mockReset();
});

describe("pdReleaseOpened (AC-AXHK2K.5)", () => {
  it("invokes pd_release_opened", async () => {
    invoke.mockResolvedValue(undefined);
    await pdReleaseOpened();
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith("pd_release_opened");
  });
});
