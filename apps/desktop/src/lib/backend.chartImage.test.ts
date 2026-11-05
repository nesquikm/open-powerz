/**
 * FR AXHK2V (AC-AXHK2V.5): "Save image" hands the rendered file to the backend, which
 * asks where to save it in the native dialog and writes it.
 *
 * API pinned here, beyond the FR's Technical Design:
 *
 * - `saveChartImage(suggestedName, format, data)` invokes `save_chart_image` with
 *   `{ suggestedName, format, data }`, `data` the file's bytes as a plain number array
 *   (a PDF's are the PNG the backend wraps); it resolves to the saved path, or `null`
 *   when the dialog was cancelled.
 */

import { beforeEach, describe, expect, it, vi } from "vitest";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => {
  class Channel {
    onmessage: (msg: unknown) => void = () => {};
  }
  return { invoke, Channel, isTauri: () => true };
});

import { saveChartImage } from "./backend";

beforeEach(() => {
  invoke.mockReset();
});

describe("save_chart_image (AC-AXHK2V.5)", () => {
  it("AC-AXHK2V.5: sends the name, the format and the bytes, and answers the path", async () => {
    invoke.mockResolvedValue("/Users/me/powerz-chart-20261102-090507.png");
    const bytes = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0, 255]);
    await expect(saveChartImage("powerz-chart-20261102-090507.png", "png", bytes)).resolves.toBe(
      "/Users/me/powerz-chart-20261102-090507.png",
    );
    expect(invoke).toHaveBeenCalledTimes(1);
    const [command, args] = invoke.mock.calls[0] as [string, Record<string, unknown>];
    expect(command).toBe("save_chart_image");
    expect(args).toEqual({
      suggestedName: "powerz-chart-20261102-090507.png",
      format: "png",
      data: [0x89, 0x50, 0x4e, 0x47, 0, 255],
    });
  });

  it("AC-AXHK2V.5: a cancelled dialog answers null", async () => {
    invoke.mockResolvedValue(null);
    await expect(saveChartImage("a.svg", "svg", new Uint8Array([60]))).resolves.toBeNull();
  });
});
