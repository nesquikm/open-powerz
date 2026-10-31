/**
 * FR TGE83C: the PD capture commands of the Tauri backend. Start, Stop, details on
 * demand and Clear go through `invoke`; rows and the capture state arrive on the
 * session channel as `pd_rows` / `pd_state` events (see `src-tauri/tests/pd_view.rs`).
 */

import { beforeEach, describe, expect, it, vi } from "vitest";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => {
  class Channel {
    onmessage: (msg: unknown) => void = () => {};
  }
  return { invoke, Channel, isTauri: () => true };
});

import { pdClear, pdDetails, pdStart, pdStop, type PdDetails } from "./backend";

beforeEach(() => {
  invoke.mockReset();
});

describe("PD capture commands (AC-TGE83C.1, .4, .7)", () => {
  it("starts and stops the capture", async () => {
    invoke.mockResolvedValue(undefined);
    await pdStart();
    await pdStop();
    expect(invoke).toHaveBeenNthCalledWith(1, "pd_start");
    expect(invoke).toHaveBeenNthCalledWith(2, "pd_stop");
  });

  it("asks for one record's details by index", async () => {
    const details: PdDetails = {
      index: 7,
      raw: [0xa1, 0x11],
      fields: [
        {
          name: "Header",
          meaning: "0x11A1 Source_Capabilities",
          raw: 0x11a1,
          bytes: [0, 2],
          children: [],
        },
      ],
    };
    invoke.mockResolvedValue(details);
    await expect(pdDetails(7)).resolves.toEqual(details);
    expect(invoke).toHaveBeenCalledWith("pd_details", { index: 7 });
  });

  it("clearing resolves to the next record index and the status floor", async () => {
    invoke.mockResolvedValue({ next_index: 25, status_floor_ms: 1234 });
    await expect(pdClear()).resolves.toEqual({ next_index: 25, status_floor_ms: 1234 });
  });

  it("clears the capture", async () => {
    invoke.mockResolvedValue(undefined);
    await pdClear();
    expect(invoke).toHaveBeenCalledWith("pd_clear");
  });
});
