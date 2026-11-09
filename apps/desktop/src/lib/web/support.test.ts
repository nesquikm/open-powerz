/**
 * FR QFTCX1, AC-QFTCX1.3: a browser without `navigator.usb` and `navigator.hid` is told
 * the web edition is not supported there; one with either API is supported.
 */

import { describe, expect, it } from "vitest";
import { detectSupport } from "./support";

describe("detectSupport (AC-QFTCX1.3)", () => {
  it("is supported with WebUSB and WebHID", () => {
    expect(detectSupport({ usb: {}, hid: {} })).toBe("supported");
  });

  it("is supported with WebUSB alone", () => {
    expect(detectSupport({ usb: {} })).toBe("supported");
  });

  it("is supported with WebHID alone", () => {
    expect(detectSupport({ hid: {} })).toBe("supported");
  });

  it("is not supported without either API (Safari, Firefox)", () => {
    expect(detectSupport({})).toBe("not-supported");
    expect(detectSupport({ usb: undefined, hid: undefined })).toBe("not-supported");
    expect(detectSupport({ usb: null, hid: null })).toBe("not-supported");
  });
});
