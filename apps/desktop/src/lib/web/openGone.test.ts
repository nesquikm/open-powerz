/**
 * FR R13Y5V, AC-R13Y5V.3: opening a WebUSB or WebHID device that the browser reports as
 * gone (a NotFoundError) fails as disconnected, with "The meter is disconnected." and
 * its hint, never as an internal error.
 *
 * Web HIL run WEB.hil2-20261108-2223 (W6): Connect on the dead menu entry rejected with
 * `NotFoundError: Failed to execute 'open' on 'USBDevice': The device was disconnected.`
 * and the page showed "Something went wrong inside the app."
 */

import { describe, expect, it } from "vitest";
import { FakeHidDevice, FakeUsbDevice } from "./fakeDevices.test";
import { disconnectedError } from "./transport";
import { openWebHid } from "./webhid";
import { openWebUsb } from "./webusb";

type UsbArg = Parameters<typeof openWebUsb>[0];
type HidArg = Parameters<typeof openWebHid>[0];

const DISCONNECTED = {
  kind: "disconnected",
  message: "The meter is disconnected.",
  hint: "Plug it in and connect again.",
};

function notFound(api: "USBDevice" | "HIDDevice"): DOMException {
  return new DOMException(
    `Failed to execute 'open' on '${api}': The device was disconnected.`,
    "NotFoundError",
  );
}

describe("Opening a meter the browser reports as gone (AC-R13Y5V.3)", () => {
  it("the expected error is the shared disconnected error", () => {
    expect(disconnectedError()).toEqual(DISCONNECTED);
  });

  it("WebUSB: a NotFoundError from open() rejects as disconnected", async () => {
    const dev = new FakeUsbDevice();
    dev.open = () => Promise.reject(notFound("USBDevice"));
    const err: unknown = await openWebUsb(dev as unknown as UsbArg, { debug: false }).then(
      () => null,
      (e: unknown) => e,
    );
    expect(err).toEqual(DISCONNECTED);
    expect(err).not.toBeInstanceOf(DOMException);
  });

  it("WebUSB: no further device calls after the failed open", async () => {
    const dev = new FakeUsbDevice();
    dev.open = () => {
      dev.calls.push("open");
      return Promise.reject(notFound("USBDevice"));
    };
    await expect(openWebUsb(dev as unknown as UsbArg, { debug: false })).rejects.toMatchObject({
      kind: "disconnected",
    });
    expect(dev.calls).toEqual(["open"]);
  });

  it("WebHID: a NotFoundError from open() rejects as disconnected", async () => {
    const dev = new FakeHidDevice();
    dev.open = () => Promise.reject(notFound("HIDDevice"));
    const err: unknown = await openWebHid(dev as unknown as HidArg).then(
      () => null,
      (e: unknown) => e,
    );
    expect(err).toEqual(DISCONNECTED);
  });
});
